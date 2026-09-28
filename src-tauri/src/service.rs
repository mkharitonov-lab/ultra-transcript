//! Очередь задач, фоновый обработчик и слежение за папками. Не зависит от UI:
//! используется и приложением, и CLI.

use crate::diar::{DiarModel, Diarizer};
use crate::lang::{tr, Stage};
use crate::pipeline::{self, Live, Report};
use crate::speech::{AsrModel, Engines};
use crate::store::{Recording, Store};
use crate::{media, models};
use anyhow::{anyhow, Result};
use serde::Serialize;
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc::{channel, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
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

pub enum Signal {
    Job(Event),
    Live(LiveEvent),
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

pub struct Service {
    pub store: Arc<Store>,
    tx: Mutex<Sender<Job>>,
    emit: Emit,
    /// Задача, которая сейчас в работе.
    live: Mutex<Option<LiveState>>,
}

impl Service {
    pub fn start(store: Arc<Store>, emit: Emit, watch: bool) -> Arc<Self> {
        let (tx, rx) = channel::<Job>();
        let svc = Arc::new(Self { store: store.clone(), tx: Mutex::new(tx), emit, live: Mutex::new(None) });

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
        for r in store.recordings().unwrap_or_default() {
            if r.status == "queued" || r.status == "processing" {
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

    /// Задача в работе и всё, что она успела показать.
    pub fn live(&self) -> Option<LiveState> {
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
            match (status, live.as_mut()) {
                ("processing", Some(l)) if l.recording_id == id && l.job == job => {
                    (l.stage, l.title, l.progress) = (stage.code().into(), stage.title().into(), progress);
                }
                ("processing", _) => {
                    *live = Some(LiveState {
                        recording_id: id.into(),
                        job,
                        stage: stage.code().into(),
                        title: stage.title().into(),
                        progress,
                        lines: vec![],
                        drafts: 0,
                        protocol: String::new(),
                    });
                }
                ("done" | "error", Some(l)) if l.recording_id == id => *live = None,
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
        if let Some(l) = self.live.lock().unwrap().as_mut().filter(|l| l.recording_id == id) {
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

impl Drop for Reporter<'_> {
    /// Остаток протокола, который ждал своей очереди.
    fn drop(&mut self) {
        let rest = std::mem::take(&mut self.protocol.borrow_mut().0);
        if !rest.is_empty() {
            self.svc.show(self.id, Live::Protocol { text: rest, reset: false });
        }
    }
}
