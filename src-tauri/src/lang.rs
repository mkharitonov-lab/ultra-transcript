//! Язык строк, которые рождаются в ядре: этапы обработки, уведомления, меню значка, ошибки,
//! имена спикеров и подписи в документах. Язык интерфейса выбирается в настройках.

use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, Ordering};

/// Настройка языка интерфейса.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Language {
    /// Как в системе.
    #[default]
    System,
    Ru,
    En,
}

static ENGLISH: AtomicBool = AtomicBool::new(false);

/// Применить настройку языка.
pub fn set(language: Language) {
    let english = match language {
        Language::Ru => false,
        Language::En => true,
        // Язык системы неизвестен — окно сообщит свой (см. `set_language`), до тех пор русский.
        Language::System => system() == Some("en"),
    };
    ENGLISH.store(english, Ordering::Relaxed);
}

pub fn is_english() -> bool {
    ENGLISH.load(Ordering::Relaxed)
}

/// Строка на языке интерфейса.
pub fn tr<'a>(ru: &'a str, en: &'a str) -> &'a str {
    if is_english() {
        en
    } else {
        ru
    }
}

/// Язык системы: "ru", если русский есть среди языков, выбранных в системе, иначе "en".
/// None — узнать не удалось. На macOS окно знает только язык, с которым запущено приложение,
/// поэтому спрашиваем у системы.
pub fn system() -> Option<&'static str> {
    let pick = |russian: bool| if russian { "ru" } else { "en" };
    #[cfg(target_os = "macos")]
    if let Ok(out) = std::process::Command::new("defaults").args(["read", "-g", "AppleLanguages"]).output() {
        let list = String::from_utf8_lossy(&out.stdout).to_lowercase();
        if out.status.success() && list.contains('"') {
            return Some(pick(list.split('"').skip(1).step_by(2).any(|l| l.starts_with("ru"))));
        }
    }
    ["LC_ALL", "LC_MESSAGES", "LANG"]
        .iter()
        .find_map(|k| std::env::var(k).ok().filter(|v| !v.is_empty() && v != "C" && v != "POSIX"))
        .map(|v| pick(v.to_lowercase().starts_with("ru")))
}

/// Системные окна приложения (выбор файла, уведомления) — на языке интерфейса. macOS выбирает
/// их язык при запуске приложения, так что перемена видна после перезапуска.
/// `app_id` — идентификатор приложения: язык записывается в его собственные настройки.
#[cfg(target_os = "macos")]
pub fn match_system_dialogs(app_id: &str) {
    use std::process::Command;
    let bundled = std::env::current_exe().is_ok_and(|p| p.to_string_lossy().contains(".app/Contents/MacOS/"));
    if !bundled {
        return;
    }
    let want = if is_english() { "en" } else { "ru" };
    let now = Command::new("defaults").args(["read", app_id, "AppleLanguages"]).output();
    let same = now.is_ok_and(|o| String::from_utf8_lossy(&o.stdout).split('"').nth(1) == Some(want));
    if !same {
        let _ = Command::new("defaults").args(["write", app_id, "AppleLanguages", "-array", want]).status();
    }
}

/// Этапы обработки записи. В события уходит код этапа и его название на языке интерфейса.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    Queue,
    LoadModels,
    Record,
    Prepare,
    Denoise,
    Recognize,
    Diarize,
    Identify,
    Terms,
    Enrich,
    Condense,
    Protocol,
    Export,
}

impl Stage {
    pub fn code(self) -> &'static str {
        match self {
            Self::Queue => "",
            Self::LoadModels => "load",
            Self::Record => "record",
            Self::Prepare => "prepare",
            Self::Denoise => "denoise",
            Self::Recognize => "recognize",
            Self::Diarize => "diarize",
            Self::Identify => "identify",
            Self::Terms => "terms",
            Self::Enrich => "enrich",
            Self::Condense => "condense",
            Self::Protocol => "protocol",
            Self::Export => "export",
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Self::Queue => "",
            Self::LoadModels => tr("Загрузка моделей", "Loading models"),
            Self::Record => tr("Запись", "Recording"),
            Self::Prepare => tr("Подготовка аудио", "Preparing audio"),
            Self::Denoise => tr("Шумоподавление", "Reducing noise"),
            Self::Recognize => tr("Распознавание речи", "Recognizing speech"),
            Self::Diarize => tr("Разделение по спикерам", "Separating speakers"),
            Self::Identify => tr("Распознавание голосов", "Identifying voices"),
            Self::Terms => tr("Исправление терминов", "Fixing terms"),
            Self::Enrich => tr("Пополнение справочников", "Updating glossary and people"),
            Self::Condense => tr("Конспект длинной записи", "Condensing a long recording"),
            Self::Protocol => tr("Составление протокола", "Writing the minutes"),
            Self::Export => tr("Сохранение файлов", "Saving files"),
        }
    }
}
