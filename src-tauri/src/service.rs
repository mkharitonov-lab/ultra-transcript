//! Очередь задач, фоновый обработчик и слежение за папками. Не зависит от UI:
//! используется и приложением, и CLI.

use crate::pipeline;
use crate::speech::Engines;
use crate::store::{Recording, Store};
use crate::{media, models};
use anyhow::{anyhow, Result};
use serde::Serialize;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[derive(Clone, Debug, Serialize)]
pub struct Event {
    pub recording_id: String,
    /// queued | processing | done | error
    pub status: String,
    pub stage: String,
    pub progress: f32,
    pub message: String,
}

pub type Emit = Arc<dyn Fn(Event) + Send + Sync>;

pub enum Job {
    Transcribe { id: String, input: PathBuf },
    Protocol { id: String },
    Export { id: String },
}

pub struct Service {
    pub store: Arc<Store>,
    tx: Mutex<Sender<Job>>,
    emit: Emit,
}

impl Service {
    pub fn start(store: Arc<Store>, emit: Emit, watch: bool) -> Arc<Self> {
        let (tx, rx) = channel::<Job>();
        let svc = Arc::new(Self { store: store.clone(), tx: Mutex::new(tx), emit });

        let worker = svc.clone();
        std::thread::spawn(move || {
            let mut engines: Option<(f32, Engines)> = None;
            for job in rx {
                worker.run(job, &mut engines);
            }
        });

        // Прерванные задачи (приложение закрыли во время обработки) — снова в очередь.
        for r in store.recordings().unwrap_or_default() {
            if r.status == "queued" || r.status == "processing" {
                svc.send(Job::Transcribe { id: r.id, input: PathBuf::from(r.source) });
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

    /// Ставит файл в очередь на расшифровку; `rule_id` — если пришёл из отслеживаемой папки.
    pub fn import(&self, input: PathBuf, rule_id: Option<i64>) -> Result<String> {
        if !media::is_media(&input) {
            return Err(anyhow!("неподдерживаемый формат: {}", input.display()));
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
        })?;
        self.notify(&id, "queued", "", 0.0, "");
        self.send(Job::Transcribe { id: id.clone(), input });
        Ok(id)
    }

    fn notify(&self, id: &str, status: &str, stage: &str, progress: f32, message: &str) {
        (self.emit)(Event {
            recording_id: id.into(),
            status: status.into(),
            stage: stage.into(),
            progress,
            message: message.into(),
        });
    }

    fn run(&self, job: Job, engines: &mut Option<(f32, Engines)>) {
        let (id, transcribing) = match &job {
            Job::Transcribe { id, .. } => (id.clone(), true),
            Job::Protocol { id } | Job::Export { id } => (id.clone(), false),
        };
        let _ = self.store.set_status(&id, "processing", "");
        self.notify(&id, "processing", "", 0.0, "");
        match self.process(job, engines) {
            Ok(warnings) => {
                let msg = warnings.join("\n");
                let _ = self.store.set_status(&id, "done", &msg);
                self.notify(&id, "done", "", 1.0, &msg);
            }
            Err(e) => {
                // Сбой протокола или экспорта не портит готовую расшифровку — это предупреждение.
                let msg = format!("{e:#}");
                let status = if transcribing { "error" } else { "done" };
                let _ = self.store.set_status(&id, status, &msg);
                self.notify(&id, status, "", 0.0, &msg);
            }
        }
    }

    fn process(&self, job: Job, engines: &mut Option<(f32, Engines)>) -> Result<Vec<String>> {
        let store = &self.store;
        let mut warnings = vec![];
        let (id, mut t) = match job {
            Job::Transcribe { id, input } => {
                let threshold = store.settings().cluster_threshold;
                if engines.as_ref().is_none_or(|(th, _)| *th != threshold) {
                    anyhow::ensure!(
                        models::all_installed(&store.models_dir()),
                        "модели не установлены — откройте «Настройки → Модели»"
                    );
                    self.notify(&id, "processing", "Загрузка моделей", 0.0, "");
                    *engines = Some((threshold, Engines::load(&store.models_dir(), threshold)?));
                }
                let eng = &engines.as_ref().unwrap().1;
                let progress = |stage: &str, p: f32| self.notify(&id, "processing", stage, p, "");
                let (t, w) = pipeline::transcribe(store, eng, &id, &input, &progress)?;
                warnings.extend(w);
                (id, t)
            }
            Job::Protocol { id } => {
                let mut t = store.load_transcript(&id)?;
                self.notify(&id, "processing", "Составление протокола", 0.0, "");
                pipeline::make_protocol(store, &mut t)?;
                (id, t)
            }
            Job::Export { id } => (id.clone(), store.load_transcript(&id)?),
        };
        let rule = store.recording(&id)?.rule_id.and_then(|rid| {
            store.rules().ok()?.into_iter().find(|r| r.id == Some(rid))
        });
        if let Some(r) = &rule {
            if r.protocol && t.protocol.is_none() {
                self.notify(&id, "processing", "Составление протокола", 0.0, "");
                if let Err(e) = pipeline::make_protocol(store, &mut t) {
                    warnings.push(format!("Протокол: {e:#}"));
                }
            }
        }
        self.notify(&id, "processing", "Сохранение файлов", 0.0, "");
        pipeline::export(store, &t, rule.as_ref())?;
        Ok(warnings)
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
