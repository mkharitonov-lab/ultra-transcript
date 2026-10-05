//! Каноническая модель данных. Всё остальное (очищенный текст, .docx, протокол)
//! — это представления этой структуры.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Transcript {
    pub id: String,
    pub title: String,
    pub source: String,
    pub created_at: String,
    pub duration: f32,
    /// Какой моделью распознано.
    #[serde(default)]
    pub asr_model: String,
    /// Каким движком разделено по спикерам.
    #[serde(default)]
    pub diar_model: String,
    /// Язык записи: "ru", "en"… Пусто — у записей прежних версий, они на русском.
    #[serde(default)]
    pub language: String,
    pub speakers: Vec<Speaker>,
    pub utterances: Vec<Utterance>,
    /// Заполненные поля шаблона протокола (ключи = плейсхолдеры шаблона).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub protocol: Option<serde_json::Value>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Speaker {
    /// Метка кластера диаризации: "S1", "S2"...
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub person_id: Option<i64>,
    /// Сходство с голосовым профилем `suggested` (или, в старых расшифровках, `person_id`).
    #[serde(default)]
    pub similarity: Option<f32>,
    /// На кого похож голос. Только подсказка: имя в расшифровку не ставится, пока
    /// пользователь не проверит голос на слух и не подтвердит.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suggested: Option<i64>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Utterance {
    pub id: usize,
    pub speaker: String,
    pub start: f32,
    pub end: f32,
    /// Как распознала модель.
    pub raw: String,
    /// После исправления терминов и ФИО (дословно).
    pub text: String,
    /// Без слов-паразитов и разговорных оборотов.
    pub clean: String,
}

impl Transcript {
    /// Русская ли запись: от этого зависит язык, на котором с ней работает LLM.
    pub fn is_russian(&self) -> bool {
        self.language.is_empty() || self.language == "ru"
    }

    pub fn speaker_name(&self, id: &str) -> String {
        self.speakers
            .iter()
            .find(|s| s.id == id)
            .map(|s| s.name.clone())
            .unwrap_or_else(|| id.to_string())
    }

    /// Текст с метками спикеров — вход для LLM.
    pub fn as_dialogue(&self, clean: bool) -> String {
        self.utterances
            .iter()
            .map(|u| {
                let t = if clean && !u.clean.is_empty() { &u.clean } else { &u.text };
                format!("{}: {}", self.speaker_name(&u.speaker), t)
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}

pub fn fmt_time(sec: f32) -> String {
    let s = sec.max(0.0) as u32;
    let (h, m, s) = (s / 3600, s / 60 % 60, s % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}
