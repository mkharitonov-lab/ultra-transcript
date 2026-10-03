//! Очередь задач, фоновый обработчик, запись с микрофона и слежение за папками.
//! Не зависит от UI: используется и приложением, и CLI.

use crate::diar::{DiarModel, Diarizer};
use crate::lang::{tr, Stage};
use crate::pipeline::{self, Live, Report};
use crate::record::{self, Capture, Listen, Wav};
use crate::speech::{AsrModel, Engines};
use crate::store::{Recording, Store};
use crate::{media, models};
use anyhow::{anyhow, Result};
use serde::Serialize;
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, sync_channel, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// Состояние задачи: поставлена, началась, перешла к новому этапу, закончилась.
#[derive(Clone, Debug, Serialize)]
pub struct Event {
    pub recording_id: String,
    pub job: JobKind,
    /// queued | processing | done | error
    pub status: String,
    /// Код этапа (см. `lang::Stage`) и его название на языке интерфейса.
    pub stage: String,
    pub title: String,
    pub progress: f32,
    pub message: String,
}

/// Текст, который появляется по ходу работы: распознанные фрагменты, черновик, протокол.
#[derive(Clone, Debug, Serialize)]
pub struct LiveEvent {
    pub recording_id: String,
    #[serde(flatten)]
    pub live: Live,
}

/// Ход записи с микрофона: сколько записано и насколько громко.
#[derive(Clone, Debug, Serialize)]
pub struct RecordEvent {
    pub recording_id: String,
    pub seconds: f32,
    /// Пиковая громкость за последние доли секунды, 0…1.
    pub peak: f32,
}

pub enum Signal {
    Job(Event),
    Live(LiveEvent),
    Record(RecordEvent),
}

pub type Emit = Arc<dyn Fn(Signal) + Send + Sync>;

pub enum Job {
    /// `asr` и `diar` — модели распознавания и диаризации для этой записи; None — из настроек.
    Transcribe { id: String, input: PathBuf, asr: Option<AsrModel>, diar: Option<DiarModel> },
    Protocol { id: String },
    Export { id: String },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum JobKind {
    /// Запись с микрофона — не задача очереди: идёт в своём потоке, пока её не остановят.
    Record,
    Transcribe,
    Protocol,
    Export,
}

impl Job {
    fn kind(&self) -> JobKind {
        match self {
            Job::Transcribe { .. } => JobKind::Transcribe,
            Job::Protocol { .. } => JobKind::Protocol,
            Job::Export { .. } => JobKind::Export,
        }
    }
}

/// Всё, что задача успела показать: окно, открытое посреди обработки, начинает не с пустого места.
#[derive(Clone, Debug, Serialize)]
pub struct LiveState {
    pub recording_id: String,
    pub job: JobKind,
    pub stage: String,
    pub title: String,
    pub progress: f32,
    pub lines: Vec<Line>,
    /// Сколько раз сохранялся черновик расшифровки.
    pub drafts: u32,
    /// Протокол, как его пишет LLM.
    pub protocol: String,
    /// Сколько секунд записано — для записи с микрофона.
    pub seconds: f32,
}

#[derive(Clone, Debug, Serialize)]
pub struct Line {
    pub start: f32,
    pub text: String,
}

/// Загруженные модели: живут между задачами, пока не сменят модель или не наступит простой.
#[derive(Default)]
struct Loaded {
    engines: Option<Engines>,
    diarizer: Option<Diarizer>,
}

/// Идущая запись с микрофона: флаги для её потока и сам поток.
struct Recorder {
    id: String,
    stop: Arc<AtomicBool>,
    /// Сохранить записанное и расшифровать (иначе — удалить).
    keep: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

pub struct Service {
    pub store: Arc<Store>,
    tx: Mutex<Sender<Job>>,
    emit: Emit,
    /// Что сейчас в работе: задача очереди и, отдельно, запись с микрофона.
    live: Mutex<Vec<LiveState>>,
    recorder: Mutex<Option<Recorder>>,
}

impl Service {
    pub fn start(store: Arc<Store>, emit: Emit, watch: bool) -> Arc<Self> {
        let (tx, rx) = channel::<Job>();
        let svc = Arc::new(Self {
            store: store.clone(),
            tx: Mutex::new(tx),
            emit,
            live: Mutex::new(vec![]),
            recorder: Mutex::new(None),
        });

        let worker = svc.clone();
        std::thread::spawn(move || {
            let mut loaded = Loaded::default();
            loop {
                match rx.recv_timeout(Duration::from_secs(300)) {
                    Ok(job) => worker.run(job, &mut loaded),
                    // 5 минут без задач — освобождаем память от моделей (≈1–8 ГБ).
                    Err(RecvTimeoutError::Timeout) => {
                        loaded = Loaded::default();
                        crate::local_llm::unload();
                    }
                    Err(RecvTimeoutError::Disconnected) => break,
                }
            }
        });

        // Прерванные расшифровки (приложение закрыли во время обработки) — снова в очередь.
        // Оборванная запись с микрофона — тоже: что успело записаться, расшифровывается.
        for r in store.recordings().unwrap_or_default() {
            if r.status == "recording" {
                let _ = Wav::repair(&PathBuf::from(&r.source));
                let _ = store.set_status(&r.id, "queued", "");
            }
            if r.status == "queued" || r.status == "processing" || r.status == "recording" {
                svc.send(Job::Transcribe { id: r.id, input: PathBuf::from(r.source), asr: None, diar: None });
            }
        }
        if watch {
            let w = svc.clone();
            std::thread::spawn(move || w.watch_folders());
        }
        svc
    }

    fn send(&self, job: Job) {
        let _ = self.tx.lock().unwrap().send(job);
    }

    pub fn enqueue(&self, job: Job) {
        self.send(job);
    }

    /// Что в работе (задача очереди, запись с микрофона) и всё, что успело показаться.
    pub fn live(&self) -> Vec<LiveState> {
        self.live.lock().unwrap().clone()
    }

    /// Ставит файл в очередь на расшифровку; `rule_id` — если пришёл из отслеживаемой папки.
    pub fn import(&self, input: PathBuf, rule_id: Option<i64>) -> Result<String> {
        if !media::is_media(&input) {
            return Err(anyhow!("{}: {}", tr("этот формат не поддерживается", "this format is not supported"), input.display()));
        }
        let id = format!(
            "{}-{}",
            chrono::Local::now().format("%Y%m%d-%H%M%S"),
            &uuid::Uuid::new_v4().simple().to_string()[..6]
        );
        self.store.upsert_recording(&Recording {
            id: id.clone(),
            title: input.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default(),
            source: input.to_string_lossy().into_owned(),
            created_at: chrono::Local::now().format("%Y-%m-%d %H:%M").to_string(),
            duration: 0.0,
            status: "queued".into(),
            error: String::new(),
            rule_id,
            folder_id: None,
            archived: false,
            has_protocol: false,
        })?;
        self.notify(&id, JobKind::Transcribe, "queued", Stage::Queue, 0.0, "");
        self.send(Job::Transcribe { id: id.clone(), input, asr: None, diar: None });
        Ok(id)
    }

    fn notify(&self, id: &str, job: JobKind, status: &str, stage: Stage, progress: f32, message: &str) {
        {
            let mut live = self.live.lock().unwrap();
            let same = live.iter_mut().find(|l| l.recording_id == id);
            match (status, same) {
                ("processing", Some(l)) if l.job == job => {
                    (l.stage, l.title, l.progress) = (stage.code().into(), stage.title().into(), progress);
                }
                ("processing", same) => {
                    let fresh = LiveState {
                        recording_id: id.into(),
                        job,
                        stage: stage.code().into(),
                        title: stage.title().into(),
                        progress,
                        lines: vec![],
                        drafts: 0,
                        protocol: String::new(),
                        seconds: 0.0,
                    };
                    match same {
                        Some(l) => *l = fresh,
                        None => live.push(fresh),
                    }
                }
                ("done" | "error", _) => live.retain(|l| l.recording_id != id),
                _ => {}
            }
        }
        (self.emit)(Signal::Job(Event {
            recording_id: id.into(),
            job,
            status: status.into(),
            stage: stage.code().into(),
            title: stage.title().into(),
            progress,
            message: message.into(),
        }));
    }

    fn show(&self, id: &str, item: Live) {
        if let Some(l) = self.live.lock().unwrap().iter_mut().find(|l| l.recording_id == id) {
            match &item {
                Live::Text { start, text } => l.lines.push(Line { start: *start, text: text.clone() }),
                Live::Draft => l.drafts += 1,
                Live::Protocol { text, reset } => {
                    if *reset {
                        l.protocol.clear();
                    }
                    l.protocol.push_str(text);
                }
            }
        }
        (self.emit)(Signal::Live(LiveEvent { recording_id: id.into(), live: item }));
    }

    fn run(&self, job: Job, loaded: &mut Loaded) {
        let kind = job.kind();
        let id = match &job {
            Job::Transcribe { id, .. } | Job::Protocol { id } | Job::Export { id } => id.clone(),
        };
        // Состояние записи в базе — это состояние её расшифровки: по нему прерванная расшифровка
        // продолжается после перезапуска. Протокол виден по событиям, а пересборка файлов
        // после правки проходит незаметно.
        if kind == JobKind::Transcribe {
            let _ = self.store.set_status(&id, "processing", "");
        }
        if kind != JobKind::Export {
            self.notify(&id, kind, "processing", Stage::Queue, 0.0, "");
        }
        match self.process(job, loaded) {
            Ok(_) if kind == JobKind::Export => {}
            Ok(warnings) => {
                let msg = warnings.join("\n");
                if kind == JobKind::Transcribe {
                    self.drop_capture(&id);
                }
                let _ = self.store.set_status(&id, "done", &msg);
                self.notify(&id, kind, "done", Stage::Queue, 1.0, &msg);
            }
            Err(e) => {
                // Сбой протокола или экспорта не портит готовую расшифровку — это предупреждение.
                let msg = format!("{e:#}");
                let status = if kind == JobKind::Transcribe { "error" } else { "done" };
                let _ = self.store.set_status(&id, status, &msg);
                self.notify(&id, kind, status, Stage::Queue, 0.0, &msg);
            }
        }
    }

    fn process(&self, job: Job, loaded: &mut Loaded) -> Result<Vec<String>> {
        let store = &self.store;
        let kind = job.kind();
        let mut warnings = vec![];
        let (id, mut t) = match job {
            Job::Transcribe { id, input, asr, diar } => {
                let settings = store.settings();
                let (asr, diar) = (asr.unwrap_or(settings.asr_model), diar.unwrap_or(settings.diar_model));
                let dir = store.models_dir();
                if let Some(m) = models::required(&dir, asr, diar).into_iter().find(|m| !m.installed) {
                    anyhow::bail!(
                        "{} {} {}",
                        tr("модель", "the model"),
                        m.title,
                        tr("не скачана — откройте «Настройки → Модели»", "is not downloaded — open Settings → Models")
                    );
                }
                // Модели меняются по отдельности: смена диаризации не перезагружает распознавание.
                let language = asr.language(&settings.speech_language);
                if loaded.engines.as_ref().is_none_or(|e| e.asr != asr || e.language != language) {
                    self.notify(&id, kind, "processing", Stage::LoadModels, 0.0, "");
                    loaded.engines = None; // освободить память до загрузки новых моделей
                    loaded.engines = Some(Engines::load(&dir, asr, &settings.speech_language)?);
                }
                if loaded.diarizer.as_ref().is_none_or(|d| d.model != diar) {
                    self.notify(&id, kind, "processing", Stage::LoadModels, 0.0, "");
                    loaded.diarizer = None;
                    loaded.diarizer = Some(Diarizer::load(&dir, diar)?);
                }
                let (Some(eng), Some(diarizer)) = (loaded.engines.as_ref(), loaded.diarizer.as_mut()) else {
                    unreachable!("модели загружены выше")
                };
                let (t, w) = pipeline::transcribe(store, eng, diarizer, &id, &input, &self.reporter(&id, kind))?;
                warnings.extend(w);
                (id, t)
            }
            Job::Protocol { id } => {
                let mut t = store.load_transcript(&id)?;
                pipeline::make_protocol(store, &mut t, &self.reporter(&id, kind))?;
                (id, t)
            }
            Job::Export { id } => (id.clone(), store.load_transcript(&id)?),
        };
        let rule = store.recording(&id)?.rule_id.and_then(|rid| {
            store.rules().ok()?.into_iter().find(|r| r.id == Some(rid))
        });
        if let Some(r) = &rule {
            if r.protocol && t.protocol.is_none() && kind == JobKind::Transcribe {
                if let Err(e) = pipeline::make_protocol(store, &mut t, &self.reporter(&id, kind)) {
                    warnings.push(format!("{}: {e:#}", tr("Протокол не составлен", "The minutes were not written")));
                }
            }
        }
        if kind != JobKind::Export {
            self.notify(&id, kind, "processing", Stage::Export, 0.0, "");
        }
        pipeline::export(store, &t, rule.as_ref())?;
        Ok(warnings)
    }

    // ---------- запись с микрофона ----------

    /// Запись, которая идёт сейчас.
    pub fn recording(&self) -> Option<String> {
        self.recorder.lock().unwrap().as_ref().map(|r| r.id.clone())
    }

    /// Начинает запись с микрофона из настроек; возвращает идентификатор новой записи.
    /// Микрофон открывается до возврата — если его нет, ошибка приходит сразу.
    pub fn start_recording(self: &Arc<Self>) -> Result<String> {
        let mut slot = self.recorder.lock().unwrap();
        if slot.is_some() {
            anyhow::bail!(tr("запись уже идёт", "a recording is already in progress"));
        }
        let settings = self.store.settings();
        let dir = self.store.models_dir();
        if let Some(m) = models::required(&dir, settings.asr_model, DiarModel::Off).into_iter().find(|m| !m.installed) {
            anyhow::bail!(
                "{} {} {}",
                tr("модель", "the model"),
                m.title,
                tr("не скачана — откройте «Настройки → Модели»", "is not downloaded — open Settings → Models")
            );
        }
        let now = chrono::Local::now();
        let id = format!("{}-{}", now.format("%Y%m%d-%H%M%S"), &uuid::Uuid::new_v4().simple().to_string()[..6]);
        let rec_dir = self.store.recording_dir(&id);
        std::fs::create_dir_all(&rec_dir)?;
        let source = rec_dir.join(record::CAPTURE_FILE);

        // Микрофон открывается в потоке записи (поток захвата живёт там), а итог ждём здесь.
        let (ready_tx, ready_rx) = sync_channel::<Result<String>>(1);
        let stop = Arc::new(AtomicBool::new(false));
        let keep = Arc::new(AtomicBool::new(true));
        let svc = self.clone();
        let (tid, tstop, tkeep, tsource) = (id.clone(), stop.clone(), keep.clone(), source.clone());
        let thread = std::thread::spawn(move || {
            let capture = match Capture::open(&settings.input_device) {
                Ok(c) => {
                    let _ = ready_tx.send(Ok(c.device.clone()));
                    c
                }
                Err(e) => return drop(ready_tx.send(Err(e))),
            };
            svc.record(&tid, capture, &tsource, &tstop, &tkeep);
        });
        match ready_rx.recv() {
            Ok(Ok(_)) => {}
            Ok(Err(e)) => {
                let _ = std::fs::remove_dir_all(&rec_dir);
                return Err(e);
            }
            Err(_) => {
                let _ = std::fs::remove_dir_all(&rec_dir);
                anyhow::bail!(tr("не удалось начать запись", "could not start recording"));
            }
        }
        self.store.upsert_recording(&Recording {
            id: id.clone(),
            title: format!("{} {}", tr("Запись", "Recording"), now.format("%d.%m.%Y %H:%M")),
            source: source.to_string_lossy().into_owned(),
            created_at: now.format("%Y-%m-%d %H:%M").to_string(),
            duration: 0.0,
            status: "recording".into(),
            error: String::new(),
            rule_id: None,
            folder_id: None,
            archived: false,
            has_protocol: false,
        })?;
        self.notify(&id, JobKind::Record, "processing", Stage::LoadModels, 0.0, "");
        *slot = Some(Recorder { id: id.clone(), stop, keep, thread: Some(thread) });
        Ok(id)
    }

    /// Поток записи: модели, затем звук в файл и фразы в окно, пока не остановят.
    fn record(&self, id: &str, capture: Capture, source: &PathBuf, stop: &AtomicBool, keep: &AtomicBool) {
        let settings = self.store.settings();
        let outcome = (|| -> Result<f32> {
            let engines = Engines::load(&self.store.models_dir(), settings.asr_model, &settings.speech_language)?;
            let mut wav = Wav::create(source)?;
            self.notify(id, JobKind::Record, "processing", Stage::Record, 0.0, "");
            let listen = RecordListener { svc: self, id };
            let seconds = record::run(&capture, &engines, &mut wav, stop, &listen);
            wav.finish()?;
            seconds
        })();
        drop(capture);
        // Запись закончилась: место освобождается до того, как окно узнает об итоге.
        if let Some(r) = self.recorder.lock().unwrap().take() {
            if let Some(t) = r.thread {
                drop(t); // свой же поток — ждать нельзя
            }
        }
        match outcome {
            Ok(_) if !keep.load(Ordering::Relaxed) => {
                let _ = self.store.delete_recording(id);
                self.notify(id, JobKind::Record, "done", Stage::Queue, 1.0, "");
            }
            Ok(_) => {
                let _ = self.store.set_status(id, "queued", "");
                self.notify(id, JobKind::Record, "done", Stage::Queue, 1.0, "");
                self.notify(id, JobKind::Transcribe, "queued", Stage::Queue, 0.0, "");
                self.send(Job::Transcribe { id: id.into(), input: source.clone(), asr: None, diar: None });
            }
            Err(e) => {
                let msg = format!("{e:#}");
                let _ = Wav::repair(source);
                let _ = self.store.set_status(id, "error", &msg);
                self.notify(id, JobKind::Record, "error", Stage::Queue, 0.0, &msg);
            }
        }
    }

    /// Останавливает запись; `keep` — сохранить и расшифровать, иначе удалить.
    /// Возвращается сразу: поток записи закончит сам и поставит расшифровку в очередь.
    pub fn stop_recording(&self, id: &str, keep: bool) -> Result<()> {
        let slot = self.recorder.lock().unwrap();
        let r = slot.as_ref().filter(|r| r.id == id).ok_or_else(|| anyhow!(tr("запись уже остановлена", "the recording has already stopped")))?;
        r.keep.store(keep, Ordering::Relaxed);
        r.stop.store(true, Ordering::Relaxed);
        Ok(())
    }

    /// Перед выходом: закончить запись, чтобы файл был цел; расшифровка пойдёт при следующем запуске.
    pub fn finish_recording(&self) {
        let r = self.recorder.lock().unwrap().take();
        if let Some(mut r) = r {
            r.stop.store(true, Ordering::Relaxed);
            if let Some(t) = r.thread.take() {
                let _ = t.join();
            }
        }
    }

    fn tick(&self, id: &str, seconds: f32, peak: f32) {
        if let Some(l) = self.live.lock().unwrap().iter_mut().find(|l| l.recording_id == id) {
            l.seconds = seconds;
        }
        (self.emit)(Signal::Record(RecordEvent { recording_id: id.into(), seconds, peak }));
    }

    /// Запись с микрофона расшифрована: исходный WAV (115 МБ в час) больше не нужен —
    /// источником остаётся архив `audio.ogg`.
    fn drop_capture(&self, id: &str) {
        let Ok(r) = self.store.recording(id) else { return };
        let source = PathBuf::from(&r.source);
        if source.file_name().is_some_and(|n| n == record::CAPTURE_FILE) && source.starts_with(self.store.recording_dir(id)) {
            let archive = self.store.recording_dir(id).join("audio.ogg");
            if archive.exists() && self.store.set_source(id, &archive).is_ok() {
                let _ = std::fs::remove_file(&source);
            }
        }
    }

    fn reporter<'a>(&'a self, id: &'a str, job: JobKind) -> Reporter<'a> {
        Reporter { svc: self, id, job, protocol: RefCell::new((String::new(), Instant::now())) }
    }

    /// Опрос папок: файл берётся в работу, когда его размер перестал меняться.
    fn watch_folders(&self) {
        let mut seen: HashMap<PathBuf, u64> = HashMap::new();
        loop {
            for rule in self.store.rules().unwrap_or_default().into_iter().filter(|r| r.enabled) {
                let Ok(entries) = std::fs::read_dir(&rule.input_dir) else { continue };
                for e in entries.flatten() {
                    let path = e.path();
                    let hidden = path
                        .file_name()
                        .and_then(|n| n.to_str())
                        .is_none_or(|n| n.starts_with('.') || n.starts_with("~$"));
                    if hidden || !media::is_media(&path) {
                        continue;
                    }
                    let Ok(size) = e.metadata().map(|m| m.len()) else { continue };
                    if size == 0 || self.store.is_processed(&path, size) {
                        continue;
                    }
                    if seen.get(&path) == Some(&size) {
                        seen.remove(&path);
                        if let Ok(id) = self.import(path.clone(), rule.id) {
                            let _ = self.store.mark_processed(&path, size, &id);
                        }
                    } else {
                        seen.insert(path, size);
                    }
                }
            }
            std::thread::sleep(Duration::from_secs(5));
        }
    }
}

/// Передаёт ход работы конвейера в события. Протокол LLM пишет по слову — чтобы не засыпать окно
/// событиями, куски копятся и уходят не чаще раза в `PROTOCOL_PACE`.
struct Reporter<'a> {
    svc: &'a Service,
    id: &'a str,
    job: JobKind,
    /// Неотправленный текст протокола и время последней отправки.
    protocol: RefCell<(String, Instant)>,
}

const PROTOCOL_PACE: Duration = Duration::from_millis(80);

impl Report for Reporter<'_> {
    fn stage(&self, stage: Stage, progress: f32) {
        self.svc.notify(self.id, self.job, "processing", stage, progress, "");
    }

    fn live(&self, live: Live) {
        let Live::Protocol { text, reset } = live else { return self.svc.show(self.id, live) };
        let mut held = self.protocol.borrow_mut();
        if reset {
            held.0.clear();
            held.1 = Instant::now();
            return self.svc.show(self.id, Live::Protocol { text, reset });
        }
        held.0.push_str(&text);
        if held.1.elapsed() >= PROTOCOL_PACE {
            held.1 = Instant::now();
            self.svc.show(self.id, Live::Protocol { text: std::mem::take(&mut held.0), reset: false });
        }
    }
}

/// Передаёт ход записи с микрофона в события окна.
struct RecordListener<'a> {
    svc: &'a Service,
    id: &'a str,
}

impl Listen for RecordListener<'_> {
    fn text(&self, start: f32, text: String) {
        self.svc.show(self.id, Live::Text { start, text });
    }

    fn tick(&self, seconds: f32, peak: f32) {
        self.svc.tick(self.id, seconds, peak);
    }
}

impl Drop for Reporter<'_> {
    /// Остаток протокола, который ждал своей очереди.
    fn drop(&mut self) {
        let rest = std::mem::take(&mut self.protocol.borrow_mut().0);
        if !rest.is_empty() {
            self.svc.show(self.id, Live::Protocol { text: rest, reset: false });
        }
    }
}
