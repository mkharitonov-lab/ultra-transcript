//! Строка меню приложения (macOS) на языке интерфейса. Пункты «Настройки», «Записать»,
//! «Добавить файлы» и «Новая папка» выполняет окно: ему уходит событие `menu` с названием пункта.

use crate::lang::tr;
use crate::tray::{app_name, show_window};
use tauri::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu};
use tauri::{AppHandle, Emitter, Wry};

/// Пункты, которые выполняет окно: (идентификатор, событие для окна).
const ACTIONS: [(&str, &str); 5] = [
    ("menu-about", "about"),
    ("menu-settings", "settings"),
    ("menu-record", "record"),
    ("menu-add", "add"),
    ("menu-folder", "folder"),
];

fn build(app: &AppHandle) -> tauri::Result<Menu<Wry>> {
    let name = app_name();
    let item = |id: &str, text: String, keys: Option<&str>| MenuItem::with_id(app, id, text, true, keys);
    let line = || PredefinedMenuItem::separator(app);
    let program = Submenu::with_items(
        app,
        name,
        true,
        &[
            &item("menu-about", format!("{} «{name}»", tr("О программе", "About")), None)?,
            &line()?,
            &item("menu-settings", tr("Настройки…", "Settings…").into(), Some("CmdOrCtrl+,"))?,
            &line()?,
            &PredefinedMenuItem::services(app, Some(tr("Службы", "Services")))?,
            &line()?,
            &PredefinedMenuItem::hide(app, Some(&format!("{} {name}", tr("Скрыть", "Hide"))))?,
            &PredefinedMenuItem::hide_others(app, Some(tr("Скрыть остальные", "Hide Others")))?,
            &PredefinedMenuItem::show_all(app, Some(tr("Показать все", "Show All")))?,
            &line()?,
            &PredefinedMenuItem::quit(app, Some(&format!("{} {name}", tr("Завершить", "Quit"))))?,
        ],
    )?;
    let file = Submenu::with_items(
        app,
        tr("Файл", "File"),
        true,
        &[
            &item("menu-record", tr("Записать", "Record").into(), Some("CmdOrCtrl+R"))?,
            &item("menu-add", tr("Добавить файлы…", "Add Files…").into(), Some("CmdOrCtrl+O"))?,
            &item("menu-folder", tr("Новая папка…", "New Folder…").into(), Some("CmdOrCtrl+Shift+N"))?,
            &line()?,
            &PredefinedMenuItem::close_window(app, Some(tr("Закрыть окно", "Close Window")))?,
        ],
    )?;
    let edit = Submenu::with_items(
        app,
        tr("Правка", "Edit"),
        true,
        &[
            &PredefinedMenuItem::undo(app, Some(tr("Отменить", "Undo")))?,
            &PredefinedMenuItem::redo(app, Some(tr("Повторить", "Redo")))?,
            &line()?,
            &PredefinedMenuItem::cut(app, Some(tr("Вырезать", "Cut")))?,
            &PredefinedMenuItem::copy(app, Some(tr("Копировать", "Copy")))?,
            &PredefinedMenuItem::paste(app, Some(tr("Вставить", "Paste")))?,
            &PredefinedMenuItem::select_all(app, Some(tr("Выбрать всё", "Select All")))?,
        ],
    )?;
    let window = Submenu::with_items(
        app,
        tr("Окно", "Window"),
        true,
        &[
            &PredefinedMenuItem::minimize(app, Some(tr("Свернуть", "Minimize")))?,
            &PredefinedMenuItem::maximize(app, Some(tr("Изменить масштаб", "Zoom")))?,
            &line()?,
            &PredefinedMenuItem::fullscreen(app, Some(tr("Во весь экран", "Enter Full Screen")))?,
        ],
    )?;
    Menu::with_items(app, &[&program, &file, &edit, &window])
}

/// Ставит меню; вызывается при запуске и при смене языка.
pub fn install(app: &AppHandle) {
    if let Ok(menu) = build(app) {
        let _ = app.set_menu(menu);
    }
}

pub fn on_event(app: &AppHandle, event: MenuEvent) {
    if let Some((_, action)) = ACTIONS.iter().find(|(id, _)| event.id().as_ref() == *id) {
        show_window(app);
        let _ = app.emit("menu", action);
    }
}
