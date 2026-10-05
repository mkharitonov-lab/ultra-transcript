//! Значок в строке меню. Закрытое окно не останавливает обработку: приложение уходит из дока
//! и остаётся значком. Подсказка у значка показывает, что сейчас в работе,
//! а о начале и конце расшифровки сообщают системные уведомления.

use crate::lang::tr;
use crate::notify;
use crate::service::{Event, JobKind, Service};
use crate::store::Store;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tauri::image::Image;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Emitter, Manager, RunEvent, Window};

const TRAY: &str = "main";

/// Идёт ли запись с микрофона — от этого зависит пункт меню значка.
static RECORDING: AtomicBool = AtomicBool::new(false);

/// Название приложения на языке интерфейса.
pub fn app_name() -> &'static str {
    tr("Ультра Транскрибатор", "Ultra Transcript")
}

fn menu(app: &AppHandle) -> tauri::Result<Menu<tauri::Wry>> {
    let open = format!("{} {}", tr("Открыть", "Open"), app_name());
    let show = MenuItem::with_id(app, "show", open, true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", tr("Выйти", "Quit"), true, None::<&str>)?;
    if RECORDING.load(Ordering::Relaxed) {
        let stop = MenuItem::with_id(app, "stop", tr("Остановить запись", "Stop Recording"), true, None::<&str>)?;
        return Menu::with_items(app, &[&show, &stop, &quit]);
    }
    let record = MenuItem::with_id(app, "record", tr("Записать с микрофона…", "Record from Microphone…"), true, None::<&str>)?;
    let meeting = MenuItem::with_id(app, "meeting", tr("Записать видеовстречу…", "Record Video Call…"), true, None::<&str>)?;
    Menu::with_items(app, &[&show, &record, &meeting, &quit])
}

/// Запись началась или кончилась — в меню значка другой пункт.
pub fn set_recording(app: &AppHandle, on: bool) {
    if RECORDING.swap(on, Ordering::Relaxed) == on {
        return;
    }
    if let Some(tray) = app.tray_by_id(TRAY) {
        let _ = tray.set_menu(menu(app).ok());
    }
}

/// Язык интерфейса сменили — меню значка и подсказка на новом языке.
pub fn relabel(app: &AppHandle) {
    if let Some(tray) = app.tray_by_id(TRAY) {
        let _ = tray.set_menu(menu(app).ok());
    }
    if let Some(status) = app.try_state::<Arc<Status>>() {
        status.relabel(app);
    }
}

pub fn build(app: &AppHandle) -> tauri::Result<()> {
    let tray = TrayIconBuilder::with_id(TRAY)
        // Отдельный одноцветный знак: у иконки приложения непрозрачный фон, шаблон из неё — сплошной квадрат.
        .icon(Image::from_bytes(include_bytes!("../icons/tray.png"))?)
        .icon_as_template(true)
        .menu(&menu(app)?)
        .on_menu_event(|app, ev| match ev.id.as_ref() {
            "show" => show_window(app),
            id @ ("record" | "meeting") => {
                show_window(app);
                let _ = app.emit("menu", id);
            }
            "stop" => {
                if let Some(svc) = app.try_state::<Arc<Service>>() {
                    if let Some(id) = svc.recording() {
                        let _ = svc.stop_recording(&id, true);
                    }
                }
            }
            "quit" => app.exit(0),
            _ => {}
        });
    // На macOS подсказку рисуем сами (см. hint.rs): системная у значка в фоне не появляется.
    #[cfg(target_os = "macos")]
    let tray = tray.on_tray_icon_event(crate::hint::on_tray_event);
    #[cfg(not(target_os = "macos"))]
    let tray = tray.tooltip(app_name());
    tray.build(app)?;
    show_status(app, app_name().into());
    Ok(())
}

/// Текст подсказки у значка: на macOS — своей (см. hint.rs), на других системах — системной.
fn show_status(app: &AppHandle, text: String) {
    #[cfg(target_os = "macos")]
    let _ = app.run_on_main_thread(move || crate::hint::set_text(&text));
    #[cfg(not(target_os = "macos"))]
    if let Some(tray) = app.tray_by_id(TRAY) {
        let _ = tray.set_tooltip(Some(text));
    }
}

/// Крестик прячет окно и убирает приложение из дока; обработка идёт дальше.
pub fn hide_window(window: &Window) {
    let _ = window.hide();
    #[cfg(target_os = "macos")]
    let _ = window.app_handle().set_dock_visibility(false);
}

pub fn show_window(app: &AppHandle) {
    #[cfg(target_os = "macos")]
    let _ = app.set_dock_visibility(true);
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.set_focus();
    }
}

/// Приложение открыли снова (Finder, Spotlight), пока оно работает в фоне, — показать окно.
#[cfg_attr(not(target_os = "macos"), allow(unused_variables))]
pub fn on_run_event(app: &AppHandle, event: RunEvent) {
    match event {
        #[cfg(target_os = "macos")]
        RunEvent::Reopen { .. } => show_window(app),
        // Модель выгружаем до выхода: иначе llama.cpp падает, освобождая видеопамять при завершении.
        // Идущая запись закрывается, чтобы файл был цел: расшифруется при следующем запуске.
        RunEvent::Exit => {
            if let Some(svc) = app.try_state::<Arc<Service>>() {
                svc.finish_recording();
            }
            crate::local_llm::unload();
        }
        _ => {}
    }
}

/// Переносит события очереди в подсказку у значка и в уведомления.
pub struct Status {
    store: Arc<Store>,
    state: Mutex<State>,
}

struct State {
    /// Запись в работе и имя её файла.
    current: Option<(String, String)>,
    /// Что сейчас в подсказке у значка.
    text: String,
}

impl Status {
    pub fn new(store: Arc<Store>) -> Self {
        Self { store, state: Mutex::new(State { current: None, text: app_name().into() }) }
    }

    /// Без задачи в подсказке название приложения — на новом языке; с задачей язык сменится со следующим событием.
    fn relabel(&self, app: &AppHandle) {
        let text = {
            let mut st = self.state.lock().unwrap();
            if st.current.is_none() {
                st.text = app_name().into();
            }
            st.text.clone()
        };
        show_status(app, text);
    }

    pub fn update(&self, app: &AppHandle, ev: &Event) {
        if ev.job == JobKind::Record {
            set_recording(app, ev.status == "processing");
        }
        if ev.status == "queued" {
            return; // значок показывает только то, что уже в работе
        }
        // Под замком только решаем, что показать: трей и окно отвечают из главного потока.
        let (text, notice) = {
            let mut st = self.state.lock().unwrap();
            let started = st.current.as_ref().is_none_or(|(id, _)| *id != ev.recording_id);
            if started {
                st.current = Some((ev.recording_id.clone(), self.file_name(&ev.recording_id)));
            }
            let file = st.current.as_ref().map_or(String::new(), |(_, f)| f.clone());
            if ev.status != "processing" {
                st.current = None;
            }
            let text = tooltip(&file, ev);
            let changed = text != st.text;
            if changed {
                st.text.clone_from(&text);
            }
            (changed.then_some(text), notice(&file, ev, started))
        };
        if let Some(text) = text {
            show_status(app, text);
        }
        // Одна метка на запись: «готова» заменяет «началась» в Центре уведомлений.
        if let Some((title, body)) = notice.filter(|_| !in_front(app) && self.store.settings().notifications) {
            notify::show(app, &ev.recording_id, title, &body);
        }
    }

    /// Имя файла записи; у записи с микрофона файла с именем нет — её название.
    fn file_name(&self, id: &str) -> String {
        let Ok(r) = self.store.recording(id) else { return String::new() };
        if Path::new(&r.source).starts_with(self.store.recording_dir(id)) {
            return r.title;
        }
        Path::new(&r.source).file_name().map_or(String::new(), |n| n.to_string_lossy().into_owned())
    }
}

/// Подсказка у значка: файл, этап и доля готовности.
fn tooltip(file: &str, ev: &Event) -> String {
    if ev.status != "processing" {
        return app_name().into();
    }
    let stage = if ev.title.is_empty() { tr("Обработка", "Processing") } else { &ev.title };
    let progress = if ev.progress > 0.0 { format!(" · {}%", (ev.progress * 100.0).round() as u32) } else { String::new() };
    lines([app_name(), file, &format!("{stage}{progress}")])
}

/// Уведомления — только о расшифровке: протокол и пересборка файлов запускаются из окна.
fn notice(file: &str, ev: &Event, started: bool) -> Option<(&'static str, String)> {
    if ev.job != JobKind::Transcribe {
        return None;
    }
    match ev.status.as_str() {
        "processing" if started => Some((tr("Расшифровка началась", "Transcription started"), file.into())),
        "done" => Some((tr("Расшифровка готова", "Transcript is ready"), lines([file, &ev.message]))),
        "error" => Some((tr("Не удалось расшифровать", "Transcription failed"), lines([file, &ev.message]))),
        _ => None,
    }
}

/// Окно на экране и в фокусе — ход работы и так виден, уведомлять не нужно.
fn in_front(app: &AppHandle) -> bool {
    app.get_webview_window("main")
        .is_some_and(|w| w.is_visible().unwrap_or(false) && w.is_focused().unwrap_or(false))
}

fn lines<const N: usize>(parts: [&str; N]) -> String {
    parts.into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(job: JobKind, status: &str, stage: &str, progress: f32, message: &str) -> Event {
        Event {
            recording_id: "r1".into(),
            job,
            status: status.into(),
            stage: String::new(),
            title: stage.into(),
            progress,
            message: message.into(),
        }
    }

    #[test]
    fn tooltip_shows_file_stage_and_percent() {
        let t = |status, stage, p| tooltip("планёрка.m4a", &ev(JobKind::Transcribe, status, stage, p, ""));
        assert_eq!(t("processing", "Разделение по спикерам", 0.426), "Ультра Транскрибатор\nпланёрка.m4a\nРазделение по спикерам · 43%");
        assert_eq!(t("processing", "Подготовка аудио", 0.0), "Ультра Транскрибатор\nпланёрка.m4a\nПодготовка аудио");
        assert_eq!(t("processing", "", 0.0), "Ультра Транскрибатор\nпланёрка.m4a\nОбработка");
        assert_eq!(t("done", "", 1.0), "Ультра Транскрибатор");
    }

    #[test]
    fn notifies_only_about_transcription() {
        let n = |job, status, msg, started| notice("планёрка.m4a", &ev(job, status, "", 0.0, msg), started);
        assert_eq!(n(JobKind::Transcribe, "processing", "", true), Some(("Расшифровка началась", "планёрка.m4a".into())));
        assert_eq!(n(JobKind::Transcribe, "processing", "", false), None);
        assert_eq!(n(JobKind::Transcribe, "done", "", false), Some(("Расшифровка готова", "планёрка.m4a".into())));
        assert_eq!(
            n(JobKind::Transcribe, "error", "ffmpeg не найден", false),
            Some(("Не удалось расшифровать", "планёрка.m4a\nffmpeg не найден".into()))
        );
        assert_eq!(n(JobKind::Protocol, "processing", "", true), None);
        assert_eq!(n(JobKind::Export, "done", "", false), None);
    }
}
