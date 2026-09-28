pub mod docx;
pub mod llm;
pub mod media;
pub mod models;
pub mod pipeline;
pub mod service;
pub mod speech;
pub mod store;
pub mod transcript;

use serde::Serialize;
use service::{Job, Service};
use std::path::PathBuf;
use std::sync::Arc;
use speech::AsrModel;
use store::{Person, Recording, Rule, Settings, Store, Suggestion, Term};
use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Emitter, Manager, State, WindowEvent};
use transcript::Transcript;

type Svc<'a> = State<'a, Arc<Service>>;
type R<T> = Result<T, String>;

fn e(err: impl std::fmt::Display) -> String {
    format!("{err:#}")
}

#[derive(Serialize)]
struct AppInfo {
    data_dir: String,
    templates_dir: String,
    models: Vec<models::Model>,
}

#[tauri::command]
fn app_info(svc: Svc) -> AppInfo {
    AppInfo {
        data_dir: svc.store.dir.to_string_lossy().into(),
        templates_dir: pipeline::templates_dir(&svc.store).to_string_lossy().into(),
        models: models::catalog(&svc.store.models_dir()),
    }
}

#[derive(Clone, Serialize)]
struct ModelProgress {
    name: String,
    progress: f32,
    error: String,
}

/// Скачивает указанные модели; без списка — недостающие для выбранной модели распознавания.
#[tauri::command]
fn install_models(app: AppHandle, svc: Svc, names: Vec<String>) {
    let dir = svc.store.models_dir();
    let asr = svc.store.settings().asr_model;
    std::thread::spawn(move || {
        let wanted = |m: &models::Model| if names.is_empty() { m.asr.is_none_or(|a| a == asr) } else { names.iter().any(|n| n == m.name) };
        for m in models::catalog(&dir).into_iter().filter(|m| !m.installed && wanted(m)) {
            let emit = |p: f32, err: String| {
                let _ = app.emit("models", ModelProgress { name: m.name.into(), progress: p, error: err });
            };
            match models::install(&dir, &m, &|p| emit(p, String::new())) {
                Ok(()) => emit(1.0, String::new()),
                Err(err) => return emit(0.0, e(err)),
            }
        }
    });
}

// ---------- библиотека ----------

#[tauri::command]
fn list_recordings(svc: Svc) -> R<Vec<Recording>> {
    svc.store.recordings().map_err(e)
}

#[tauri::command]
fn import_files(svc: Svc, paths: Vec<String>) -> R<Vec<String>> {
    paths.into_iter().map(|p| svc.import(PathBuf::from(p), None).map_err(e)).collect()
}

/// Расшифровать заново — моделью из настроек или указанной.
#[tauri::command]
fn retry(svc: Svc, id: String, asr: Option<AsrModel>) -> R<()> {
    let r = svc.store.recording(&id).map_err(e)?;
    svc.store.set_status(&id, "queued", "").map_err(e)?;
    svc.enqueue(Job::Transcribe { id, input: PathBuf::from(r.source), asr });
    Ok(())
}

#[tauri::command]
fn delete_recording(svc: Svc, id: String) -> R<()> {
    svc.store.delete_recording(&id).map_err(e)
}

#[tauri::command]
fn get_transcript(svc: Svc, id: String) -> R<Transcript> {
    svc.store.load_transcript(&id).map_err(e)
}

/// Сохраняет правки и пересобирает файлы экспорта.
#[tauri::command]
fn save_transcript(svc: Svc, transcript: Transcript) -> R<()> {
    svc.store.save_transcript(&transcript).map_err(e)?;
    svc.enqueue(Job::Export { id: transcript.id });
    Ok(())
}

/// Назначает спикеру человека из справочника (или создаёт нового) и обучает голосовой профиль.
#[tauri::command]
fn assign_speaker(svc: Svc, id: String, speaker: String, person_id: Option<i64>, name: String) -> R<Transcript> {
    let store = &svc.store;
    let pid = match person_id {
        Some(pid) => Some(pid),
        None if !name.trim().is_empty() => {
            Some(store.save_person(&Person { name: name.trim().into(), ..Default::default() }).map_err(e)?)
        }
        None => None,
    };
    let people = store.people().map_err(e)?;
    let mut t = store.load_transcript(&id).map_err(e)?;
    for s in t.speakers.iter_mut().filter(|s| s.id == speaker) {
        s.person_id = pid;
        s.similarity = None;
        s.name = pid
            .and_then(|pid| people.iter().find(|p| p.id == Some(pid)).map(|p| p.name.clone()))
            .unwrap_or_else(|| if name.trim().is_empty() { s.id.replace('S', "Спикер ") } else { name.trim().into() });
    }
    store.bind_voice(&id, &speaker, pid).map_err(e)?;
    store.save_transcript(&t).map_err(e)?;
    svc.enqueue(Job::Export { id });
    Ok(t)
}

#[tauri::command]
fn make_protocol(svc: Svc, id: String) {
    svc.enqueue(Job::Protocol { id });
}

// ---------- справочники ----------

#[tauri::command]
fn list_terms(svc: Svc) -> R<Vec<Term>> {
    svc.store.terms().map_err(e)
}
#[tauri::command]
fn save_term(svc: Svc, term: Term) -> R<i64> {
    svc.store.save_term(&term).map_err(e)
}
#[tauri::command]
fn delete_term(svc: Svc, id: i64) -> R<()> {
    svc.store.delete_term(id).map_err(e)
}
#[tauri::command]
fn list_people(svc: Svc) -> R<Vec<Person>> {
    svc.store.people().map_err(e)
}
#[tauri::command]
fn save_person(svc: Svc, person: Person) -> R<i64> {
    svc.store.save_person(&person).map_err(e)
}
#[tauri::command]
fn delete_person(svc: Svc, id: i64) -> R<()> {
    svc.store.delete_person(id).map_err(e)
}
#[tauri::command]
fn delete_voiceprints(svc: Svc, person_id: i64) -> R<()> {
    svc.store.delete_voices(person_id).map_err(e)
}
#[tauri::command]
fn list_suggestions(svc: Svc) -> R<Vec<Suggestion>> {
    svc.store.suggestions().map_err(e)
}
#[tauri::command]
fn resolve_suggestion(svc: Svc, id: i64, accept: bool) -> R<()> {
    svc.store.resolve_suggestion(id, accept).map_err(e)
}

// ---------- папки и настройки ----------

#[tauri::command]
fn list_rules(svc: Svc) -> R<Vec<Rule>> {
    svc.store.rules().map_err(e)
}
#[tauri::command]
fn save_rule(svc: Svc, rule: Rule) -> R<i64> {
    svc.store.save_rule(&rule).map_err(e)
}
#[tauri::command]
fn delete_rule(svc: Svc, id: i64) -> R<()> {
    svc.store.delete_rule(id).map_err(e)
}
#[tauri::command]
fn get_settings(svc: Svc) -> Settings {
    svc.store.settings()
}
#[tauri::command]
fn save_settings(svc: Svc, settings: Settings) -> R<()> {
    svc.store.save_settings(&settings).map_err(e)
}

#[tauri::command]
async fn test_llm(settings: Settings) -> R<String> {
    tauri::async_runtime::spawn_blocking(move || {
        llm::chat_json(&settings, "Ответь JSON {\"ok\": true}", "Проверка связи")
            .map(|v| v.to_string())
            .map_err(e)
    })
    .await
    .map_err(e)?
}

/// Показать файл в Finder / Проводнике (или открыть, если `open`).
#[tauri::command]
fn reveal(path: String, open: bool) -> R<()> {
    let mut cmd = if cfg!(target_os = "macos") {
        let mut c = std::process::Command::new("open");
        if !open {
            c.arg("-R");
        }
        c.arg(&path);
        c
    } else {
        let mut c = std::process::Command::new("explorer");
        c.arg(if open { path } else { format!("/select,{path}") });
        c
    };
    cmd.spawn().map(|_| ()).map_err(e)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let store = Arc::new(Store::open()?);
            pipeline::ensure_templates(&store)?;
            let handle = app.handle().clone();
            let svc = Service::start(store, Arc::new(move |ev| { let _ = handle.emit("job", ev); }), true);
            app.manage(svc);

            // Приложение живёт в трее: закрытие окна не останавливает фоновую обработку папок.
            let show = MenuItem::with_id(app, "show", "Открыть Ultra Transcript", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "Выйти", true, None::<&str>)?;
            TrayIconBuilder::new()
                .icon(app.default_window_icon().unwrap().clone())
                .icon_as_template(true)
                .menu(&Menu::with_items(app, &[&show, &quit])?)
                .on_menu_event(|app, ev| match ev.id.as_ref() {
                    "show" => {
                        if let Some(w) = app.get_webview_window("main") {
                            let _ = w.show();
                            let _ = w.set_focus();
                        }
                    }
                    "quit" => app.exit(0),
                    _ => {}
                })
                .build(app)?;
            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .invoke_handler(tauri::generate_handler![
            app_info, install_models, list_recordings, import_files, retry, delete_recording,
            get_transcript, save_transcript, assign_speaker, make_protocol,
            list_terms, save_term, delete_term, list_people, save_person, delete_person,
            delete_voiceprints, list_suggestions, resolve_suggestion,
            list_rules, save_rule, delete_rule, get_settings, save_settings, test_llm, reveal,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
