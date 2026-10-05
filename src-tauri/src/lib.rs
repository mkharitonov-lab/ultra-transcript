#[cfg(target_os = "macos")]
mod appmenu;
pub mod audio;
pub mod diar;
pub mod docx;
pub mod exchange;
#[cfg(target_os = "macos")]
mod hint;
pub mod lang;
pub mod llm;
pub mod local_llm;
pub mod markdown;
pub mod media;
pub mod models;
mod notify;
pub mod pipeline;
pub mod record;
pub mod service;
pub mod speech;
pub mod store;
#[cfg(target_os = "macos")]
mod systap;
pub mod transcript;
mod tray;
pub mod voices;

use exchange::Directory;
use lang::{tr, Language};
use pipeline::{Doc, Format};
use serde::Serialize;
use service::{Job, Service, Signal};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use speech::AsrModel;
use store::{Folder, LlmProvider, Pending, Person, Recording, Rule, Settings, Stats, Store, Term};
use tauri::{AppHandle, Emitter, Manager, State, WindowEvent};
use transcript::Transcript;

type Svc<'a> = State<'a, Arc<Service>>;
type R<T> = Result<T, String>;

fn e(err: impl std::fmt::Display) -> String {
    format!("{err:#}")
}

#[derive(Serialize)]
struct AppInfo {
    version: String,
    data_dir: String,
    templates_dir: String,
    /// Каталог моделей; у нужных при текущих настройках — отметка `required`.
    models: Vec<models::Model>,
    /// Собрана ли библиотека NeMo-Speech.cpp для Nemotron 3.
    nemotron_runtime: bool,
    /// Язык системы ("ru" | "en"), если его удалось узнать.
    system_language: Option<&'static str>,
}

#[tauri::command]
fn app_info(app: AppHandle, svc: Svc) -> AppInfo {
    let settings = svc.store.settings();
    AppInfo {
        version: app.package_info().version.to_string(),
        data_dir: svc.store.dir.to_string_lossy().into(),
        templates_dir: pipeline::templates_dir(&svc.store).to_string_lossy().into(),
        models: models::with_required(&svc.store.models_dir(), settings.asr_model, settings.diar_model),
        nemotron_runtime: diar::nemotron::library_path().is_some(),
        system_language: lang::system(),
    }
}

/// Тексты лицензии приложения и сведений о сторонних компонентах — они вшиты в программу.
#[tauri::command]
fn legal(kind: String) -> R<&'static str> {
    match kind.as_str() {
        "license" => Ok(include_str!("../../LICENSE")),
        "notices" => Ok(include_str!("../../THIRD_PARTY_NOTICES.md")),
        _ => Err(format!("legal: {kind}")),
    }
}

#[derive(Clone, Serialize)]
struct ModelProgress {
    name: String,
    progress: f32,
    error: String,
}

/// Скачивает указанные модели; без списка — недостающие для выбранных моделей распознавания
/// и диаризации и шумоподавление.
#[tauri::command]
fn install_models(app: AppHandle, svc: Svc, names: Vec<String>) {
    let dir = svc.store.models_dir();
    let settings = svc.store.settings();
    let (asr, diar) = (settings.asr_model, settings.diar_model);
    std::thread::spawn(move || {
        let wanted = |m: &models::Model| {
            if names.is_empty() { m.needed_for(asr, diar) || m.kind == "denoise" } else { names.iter().any(|n| n == m.name) }
        };
        for m in models::catalog(&dir).into_iter().filter(|m| !m.installed && wanted(m)) {
            let emit = |p: f32, err: String| {
                let _ = app.emit("models", ModelProgress { name: m.name.into(), progress: p, error: err });
            };
            match models::install(&dir, &m, &|p| emit(p.min(0.999), String::new())) {
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
fn retry(svc: Svc, id: String, asr: Option<AsrModel>, diar: Option<diar::DiarModel>) -> R<()> {
    let r = svc.store.recording(&id).map_err(e)?;
    svc.store.set_status(&id, "queued", "").map_err(e)?;
    svc.enqueue(Job::Transcribe { id, input: PathBuf::from(r.source), asr, diar });
    Ok(())
}

#[tauri::command]
fn delete_recording(svc: Svc, id: String) -> R<()> {
    if svc.recording().as_deref() == Some(id.as_str()) {
        return Err(tr("сначала остановите запись", "stop the recording first").into());
    }
    svc.store.delete_recording(&id).map_err(e)
}

#[tauri::command]
fn rename_recording(svc: Svc, id: String, title: String) -> R<()> {
    svc.store.rename_recording(&id, &title).map_err(e)?;
    // Файлы пересобираются, когда есть что пересобирать: у идущей записи расшифровки ещё нет.
    if svc.store.load_transcript(&id).is_ok() {
        svc.enqueue(Job::Export { id });
    }
    Ok(())
}

#[tauri::command]
fn archive_recordings(svc: Svc, ids: Vec<String>, archived: bool) -> R<()> {
    svc.store.set_archived(&ids, archived).map_err(e)
}

#[tauri::command]
fn move_recordings(svc: Svc, ids: Vec<String>, folder: Option<i64>) -> R<()> {
    svc.store.move_recordings(&ids, folder).map_err(e)
}

#[tauri::command]
fn list_folders(svc: Svc) -> R<Vec<Folder>> {
    svc.store.folders().map_err(e)
}

#[tauri::command]
fn save_folder(svc: Svc, id: Option<i64>, name: String) -> R<i64> {
    svc.store.save_folder(id, &name).map_err(e)
}

#[tauri::command]
fn delete_folder(svc: Svc, id: i64) -> R<()> {
    svc.store.delete_folder(id).map_err(e)
}

/// Поиск по тексту расшифровок.
#[tauri::command]
async fn search(svc: Svc<'_>, query: String) -> R<Vec<pipeline::Hit>> {
    let store = svc.store.clone();
    blocking(move || pipeline::search(&store, &query)).await
}

#[tauri::command]
fn stats(svc: Svc) -> R<Stats> {
    svc.store.stats().map_err(e)
}

#[tauri::command]
fn get_transcript(svc: Svc, id: String) -> R<Transcript> {
    svc.store.load_transcript(&id).map_err(e)
}

/// Что сейчас в работе (задача очереди, запись с микрофона) и всё, что успело показаться.
#[tauri::command]
fn live(svc: Svc) -> Vec<service::LiveState> {
    svc.live()
}

// ---------- запись с микрофона ----------

/// Начинает запись; устройства открываются до ответа, так что без них — ошибка сразу.
/// `source` — только микрофон или встреча (микрофон и звук компьютера).
#[tauri::command]
async fn start_recording(svc: Svc<'_>, source: Option<record::Source>) -> R<String> {
    let svc = svc.inner().clone();
    blocking(move || svc.start_recording(source.unwrap_or_default())).await
}

/// Останавливает запись: `keep` — сохранить и расшифровать, иначе удалить.
#[tauri::command]
fn stop_recording(svc: Svc, id: String, keep: bool) -> R<()> {
    svc.stop_recording(&id, keep).map_err(e)
}

#[tauri::command]
async fn input_devices() -> R<Vec<String>> {
    blocking(|| Ok(record::input_devices())).await
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
    if let Some(pid) = pid {
        store.confirm_person(pid).map_err(e)?; // находку LLM указали спикером — значит, она верна
    }
    let people = store.people().map_err(e)?;
    let mut t = store.load_transcript(&id).map_err(e)?;
    let russian = t.is_russian();
    for s in t.speakers.iter_mut().filter(|s| s.id == speaker) {
        s.person_id = pid;
        s.similarity = None;
        s.name = pid
            .and_then(|pid| people.iter().find(|p| p.id == Some(pid)).map(|p| p.name.clone()))
            .unwrap_or_else(|| match (name.trim(), s.id.trim_start_matches('S').parse()) {
                ("", Ok(n)) => pipeline::speaker_label(n, russian),
                ("", _) => s.id.clone(),
                (name, _) => name.into(),
            });
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

// ---------- выгрузка ----------

/// Собирает документ (расшифровку или протокол) в Markdown или Word и возвращает путь к файлу:
/// `to` — куда сохранить; без него файл остаётся в папке записи.
#[tauri::command]
async fn export_file(svc: Svc<'_>, id: String, doc: Doc, format: Format, to: Option<String>, verbatim: bool) -> R<String> {
    let store = svc.store.clone();
    blocking(move || {
        let t = store.load_transcript(&id)?;
        let path = pipeline::write_doc(&store, &t, doc, format, verbatim, to.as_deref().map(Path::new))?;
        Ok(path.to_string_lossy().into_owned())
    })
    .await
}

/// Текст документа в Markdown — для буфера обмена.
#[tauri::command]
fn export_text(svc: Svc, id: String, doc: Doc, verbatim: bool) -> R<String> {
    let t = svc.store.load_transcript(&id).map_err(e)?;
    pipeline::markdown(&svc.store, &t, doc, verbatim).map_err(e)
}

/// Текст из буфера обмена — для «Вставить» в меню приложения.
#[tauri::command]
fn clipboard_text() -> R<String> {
    let mut cmd = if cfg!(target_os = "macos") {
        std::process::Command::new("pbpaste")
    } else if cfg!(windows) {
        let mut c = std::process::Command::new("powershell");
        c.args(["-NoProfile", "-Command", "[Console]::OutputEncoding=[Text.Encoding]::UTF8; Get-Clipboard -Raw"]);
        c
    } else {
        let mut c = std::process::Command::new("xclip");
        c.args(["-selection", "clipboard", "-o"]);
        c
    };
    // Приложение может быть запущено с любой локалью — текст нужен в UTF-8.
    let out = cmd.env("LANG", "en_US.UTF-8").output().map_err(e)?;
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
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
/// Сколько находок LLM ждут проверки — для индикаторов в меню.
#[tauri::command]
fn pending_counts(svc: Svc) -> R<Pending> {
    svc.store.pending().map_err(e)
}

/// Выгрузка справочника в .xlsx или .csv; возвращает число записей.
#[tauri::command]
async fn export_directory(svc: Svc<'_>, kind: Directory, path: String) -> R<usize> {
    let store = svc.store.clone();
    blocking(move || exchange::export(&store, kind, Path::new(&path))).await
}

#[tauri::command]
async fn import_directory(svc: Svc<'_>, kind: Directory, path: String) -> R<exchange::Report> {
    let store = svc.store.clone();
    blocking(move || exchange::import(&store, kind, Path::new(&path))).await
}

// ---------- голоса ----------

#[tauri::command]
async fn list_voices(svc: Svc<'_>, person_id: i64) -> R<Vec<voices::VoiceSample>> {
    let store = svc.store.clone();
    blocking(move || voices::samples(&store, person_id)).await
}

/// Образец голоса из аудио- или видеофайла.
#[tauri::command]
async fn add_voice(svc: Svc<'_>, person_id: i64, path: String) -> R<()> {
    let store = svc.store.clone();
    blocking(move || voices::add_from_file(&store, person_id, Path::new(&path))).await
}

#[tauri::command]
fn delete_voice(svc: Svc, id: i64) -> R<()> {
    svc.store.delete_voice(id).map_err(e)
}

/// Долгая работа (файлы, ffmpeg, модели) — вне главного потока, чтобы окно не подвисало.
async fn blocking<T: Send + 'static>(f: impl FnOnce() -> anyhow::Result<T> + Send + 'static) -> R<T> {
    tauri::async_runtime::spawn_blocking(f).await.map_err(e)?.map_err(e)
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
fn save_settings(app: AppHandle, svc: Svc, settings: Settings) -> R<()> {
    if !settings.llm_enabled || settings.llm_provider == LlmProvider::Api {
        local_llm::unload(); // освободить память
    }
    svc.store.save_settings(&settings).map_err(e)?;
    apply_theme(&app, settings.theme);
    Ok(())
}

/// Язык интерфейса, который выбрало окно: «как в системе» оно определяет точнее, чем ядро.
/// На этом языке ядро пишет этапы, уведомления, меню значка и ошибки.
#[tauri::command]
fn set_language(app: AppHandle, language: Language) {
    lang::set(language);
    tray::relabel(&app);
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.set_title(tray::app_name());
    }
    #[cfg(target_os = "macos")]
    {
        appmenu::install(&app);
        lang::match_system_dialogs(&app.config().identifier);
    }
}

/// Тема ставится нативно, на всё приложение: WebView подхватывает её через `prefers-color-scheme`,
/// а вместе с ним — системные диалоги, меню и элементы форм.
fn apply_theme(app: &AppHandle, theme: store::Theme) {
    app.set_theme(match theme {
        store::Theme::System => None,
        store::Theme::Light => Some(tauri::Theme::Light),
        store::Theme::Dark => Some(tauri::Theme::Dark),
    });
}

#[tauri::command]
fn delete_model(svc: Svc, name: String) -> R<()> {
    let dir = svc.store.models_dir();
    let m = models::find(&dir, &name).ok_or_else(|| tr("нет такой модели", "no such model").to_string())?;
    if m.kind == "llm" {
        local_llm::unload();
    }
    models::remove(&dir, &m).map_err(e)
}

#[tauri::command]
async fn test_llm(settings: Settings) -> R<String> {
    tauri::async_runtime::spawn_blocking(move || {
        llm::chat_json(&settings, "Reply with JSON {\"ok\": true}", "ping", r#"{"type":"object","properties":{"ok":{"type":"boolean"}},"required":["ok"]}"#)
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
            let settings = store.settings();
            lang::set(settings.language);
            apply_theme(app.handle(), settings.theme);
            pipeline::ensure_templates(&store)?;
            // Значок и уведомления — до запуска очереди: прерванные задачи продолжаются сразу.
            tray::build(app.handle())?;
            notify::init(app.handle())?;
            #[cfg(target_os = "macos")]
            {
                appmenu::install(app.handle());
                app.on_menu_event(appmenu::on_event);
            }
            let handle = app.handle().clone();
            let status = Arc::new(tray::Status::new(store.clone()));
            app.manage(status.clone());
            let svc = Service::start(store, Arc::new(move |signal| match signal {
                Signal::Job(ev) => {
                    let _ = handle.emit("job", &ev);
                    status.update(&handle, &ev);
                }
                Signal::Live(ev) => {
                    let _ = handle.emit("live", &ev);
                }
                Signal::Record(ev) => {
                    let _ = handle.emit("record", &ev);
                }
            }), true);
            app.manage(svc);
            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                tray::hide_window(window);
            }
        })
        .invoke_handler(tauri::generate_handler![
            app_info, legal, install_models, list_recordings, import_files, retry, delete_recording,
            rename_recording, archive_recordings, move_recordings, list_folders, save_folder, delete_folder,
            search, stats, get_transcript, live, save_transcript, assign_speaker, make_protocol,
            start_recording, stop_recording, input_devices,
            export_file, export_text, clipboard_text,
            list_terms, save_term, delete_term, list_people, save_person, delete_person,
            pending_counts, export_directory, import_directory, list_voices, add_voice, delete_voice,
            list_rules, save_rule, delete_rule, get_settings, save_settings, set_language, test_llm, reveal, delete_model,
        ])
        .build(tauri::generate_context!())
        .expect("error while running tauri application")
        .run(tray::on_run_event);
}
