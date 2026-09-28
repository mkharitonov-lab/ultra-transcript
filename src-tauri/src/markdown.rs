//! Markdown — основной формат выгрузки: его одинаково хорошо читают люди и языковые модели.
//! Документ Word собирается из тех же данных по запросу (`docx.rs`).
//! Подписи — на языке записи.

use crate::transcript::{fmt_time, Transcript};
use serde_json::{Map, Value};

/// Расшифровка по спикерам. `verbatim` — дословный текст вместо очищенного.
pub fn transcript(t: &Transcript, verbatim: bool) -> String {
    let tr = |ru: &'static str, en: &'static str| if t.is_russian() { ru } else { en };
    let mut out = format!("# {}\n\n", line(&t.title));
    out.push_str(&format!("**{}:** {}  \n", tr("Дата", "Date"), t.created_at));
    out.push_str(&format!("**{}:** {}", tr("Длительность", "Duration"), fmt_time(t.duration)));
    if !t.speakers.is_empty() {
        let names: Vec<&str> = t.speakers.iter().map(|s| s.name.as_str()).collect();
        out.push_str(&format!("  \n**{}:** {}", tr("Участники", "Participants"), names.join(", ")));
    }
    out.push_str("\n\n---\n");
    let mut prev = "";
    for u in &t.utterances {
        let text = if verbatim || u.clean.is_empty() { &u.text } else { &u.clean };
        if text.trim().is_empty() {
            continue;
        }
        if u.speaker != prev {
            out.push_str(&format!("\n**{}** · {}\n", line(&t.speaker_name(&u.speaker)), fmt_time(u.start)));
            prev = &u.speaker;
        }
        out.push_str(&format!("\n{}\n", escape(text.trim())));
    }
    out
}

/// Протокол: поля в порядке `order` (метки шаблона), остальные — следом.
pub fn protocol(t: &Transcript, order: &[String]) -> Option<String> {
    let Some(Value::Object(fields)) = &t.protocol else { return None };
    let subject = ["тема", "topic", "название", "title"].iter().find_map(|k| text_of(fields, k));
    let heading = if t.is_russian() { "Протокол" } else { "Minutes" };
    let mut out = match &subject {
        Some((_, s)) => format!("# {heading}: {}\n", line(s)),
        None => format!("# {heading}: {}\n", line(&t.title)),
    };
    let mut keys: Vec<&String> = order.iter().filter(|k| fields.contains_key(*k)).collect();
    keys.extend(fields.keys().filter(|k| !order.contains(k)));
    // Короткие сведения в начале (дата, участники) — строками «Поле: значение»; дальше — разделы.
    let mut facts = true;
    let mut first = true;
    for key in keys {
        if subject.as_ref().is_some_and(|(k, _)| k == key) {
            continue;
        }
        let name = label(key);
        let value = &fields[key];
        match value {
            Value::Null => continue,
            Value::String(s) if facts && s.chars().count() <= 160 && !s.contains('\n') => {
                if !s.trim().is_empty() {
                    out.push_str(if first { "\n" } else { "  \n" });
                    out.push_str(&format!("**{name}:** {}", escape(s.trim())));
                    first = false;
                }
                continue;
            }
            _ => facts = false,
        }
        out.push_str(&format!("{}## {name}\n\n", if first { "\n" } else { "\n\n" }));
        first = false;
        match value {
            Value::String(s) if s.trim().is_empty() => out.push('—'),
            Value::String(s) => out.push_str(&escape(s.trim())),
            Value::Array(items) if items.is_empty() => out.push('—'),
            Value::Array(items) if items.iter().any(Value::is_object) => out.push_str(&table(items)),
            Value::Array(items) => {
                out.push_str(&items.iter().map(|it| format!("- {}", line(&scalar(it)))).collect::<Vec<_>>().join("\n"));
            }
            v => out.push_str(&escape(&scalar(v))),
        }
    }
    out.push('\n');
    Some(out)
}

fn text_of<'a>(fields: &'a Map<String, Value>, key: &str) -> Option<(String, &'a str)> {
    let s = fields.get(key)?.as_str()?.trim();
    (!s.is_empty()).then(|| (key.to_string(), s))
}

/// «краткое_содержание» → «Краткое содержание».
pub fn label(key: &str) -> String {
    let s = key.replace('_', " ");
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().chain(c).collect(),
        None => String::new(),
    }
}

fn table(rows: &[Value]) -> String {
    let mut cols: Vec<&String> = vec![];
    for r in rows.iter().filter_map(Value::as_object) {
        for k in r.keys() {
            if !cols.contains(&k) {
                cols.push(k);
            }
        }
    }
    let row = |cells: Vec<String>| format!("| {} |\n", cells.join(" | "));
    let mut out = row(cols.iter().map(|c| label(c)).collect());
    out.push_str(&row(cols.iter().map(|_| "---".to_string()).collect()));
    for r in rows {
        out.push_str(&row(cols.iter().map(|c| cell(&scalar(r.get(c.as_str()).unwrap_or(&Value::Null)))).collect()));
    }
    out.trim_end().to_string()
}

fn scalar(v: &Value) -> String {
    match v {
        Value::Null => String::new(),
        Value::String(s) => s.trim().to_string(),
        Value::Array(a) => a.iter().map(scalar).collect::<Vec<_>>().join(", "),
        Value::Object(o) => o.values().map(scalar).filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" — "),
        v => v.to_string(),
    }
}

/// Текст не должен превращаться в разметку: заголовок, список или цитату в начале строки, выделение звёздочками.
fn escape(s: &str) -> String {
    s.lines()
        .map(|l| {
            let l = l.replace('*', "\\*").replace('_', "\\_").replace('`', "\\`");
            let indent = l.len() - l.trim_start().len();
            let t = &l[indent..];
            // «1. пункт», «2) пункт»: экранируется точка или скобка после числа.
            let digits = t.chars().take_while(char::is_ascii_digit).count();
            let after = &t[digits..];
            if digits > 0 && after.starts_with(['.', ')']) && (after.len() == 1 || after[1..].starts_with(' ')) {
                return format!("{}{}\\{after}", &l[..indent], &t[..digits]);
            }
            let heading = t.starts_with('#') && t.trim_start_matches('#').starts_with(' ');
            let bullet = t.starts_with(['-', '+']) && (t.len() == 1 || t[1..].starts_with(' '));
            if heading || bullet || t.starts_with('>') {
                format!("{}\\{t}", &l[..indent])
            } else {
                l
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Однострочное значение: заголовок, пункт списка.
fn line(s: &str) -> String {
    escape(&s.split_whitespace().collect::<Vec<_>>().join(" "))
}

fn cell(s: &str) -> String {
    escape(s).replace('|', "\\|").replace('\n', "<br>")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transcript::{Speaker, Utterance};
    use serde_json::json;

    fn sample() -> Transcript {
        let u = |id, speaker: &str, start, text: &str, clean: &str| Utterance {
            id,
            speaker: speaker.into(),
            start,
            end: start + 5.0,
            raw: text.into(),
            text: text.into(),
            clean: clean.into(),
        };
        Transcript {
            id: "r1".into(),
            title: "Планёрка".into(),
            created_at: "2026-09-28 10:00".into(),
            duration: 3725.0,
            speakers: vec![
                Speaker { id: "S1".into(), name: "Петров Сергей".into(), ..Default::default() },
                Speaker { id: "S2".into(), name: "Спикер 2".into(), ..Default::default() },
            ],
            utterances: vec![
                u(0, "S1", 0.0, "Ну, начнём.", "Начнём."),
                u(1, "S1", 8.0, "Смета готова.", "Смета готова."),
                u(2, "S2", 65.0, "- 5 * 3 = 15", "- 5 * 3 = 15"),
            ],
            ..Default::default()
        }
    }

    #[test]
    fn transcript_groups_utterances_by_speaker() {
        let md = transcript(&sample(), false);
        assert!(md.starts_with("# Планёрка\n\n**Дата:** 2026-09-28 10:00  \n**Длительность:** 1:02:05  \n**Участники:** Петров Сергей, Спикер 2\n\n---\n"));
        assert_eq!(md.matches("**Петров Сергей** · 0:00").count(), 1);
        assert!(md.contains("\nНачнём.\n\nСмета готова.\n\n**Спикер 2** · 1:05\n"));
        // Текст реплики не становится списком и курсивом.
        assert!(md.contains("\n\\- 5 \\* 3 = 15\n"));
        assert_eq!(escape("1. Первое\n2024 год. -5 градусов, #тег\n# Заголовок\n> цитата"), "1\\. Первое\n2024 год. -5 градусов, #тег\n\\# Заголовок\n\\> цитата");
        assert!(transcript(&sample(), true).contains("Ну, начнём."));
    }

    #[test]
    fn protocol_follows_template_order() {
        let mut t = sample();
        assert_eq!(protocol(&t, &[]), None);
        t.protocol = Some(json!({
            "решения": ["Подать заявку", "Согласовать смету"],
            "тема": "Заявка в Минпромторг",
            "дата": "28.09.2026",
            "участники": "Петров С., Смирнова А.",
            "поручения": [{"что": "Подготовить заявку", "ответственный": "Петров", "срок": "10.10"}],
            "открытые_вопросы": [],
            "краткое_содержание": "Обсудили сроки.\nРешили подавать.",
        }));
        let order: Vec<String> =
            ["тема", "дата", "участники", "краткое_содержание", "решения", "поручения"].map(String::from).into();
        let md = protocol(&t, &order).unwrap();
        assert_eq!(
            md,
            "# Протокол: Заявка в Минпромторг\n\n**Дата:** 28.09.2026  \n**Участники:** Петров С., Смирнова А.\n\n\
             ## Краткое содержание\n\nОбсудили сроки.\nРешили подавать.\n\n\
             ## Решения\n\n- Подать заявку\n- Согласовать смету\n\n\
             ## Поручения\n\n| Что | Ответственный | Срок |\n| --- | --- | --- |\n| Подготовить заявку | Петров | 10.10 |\n\n\
             ## Открытые вопросы\n\n—\n"
        );
        // Короткий текст после первого раздела — тоже раздел, а не строка, прилипшая к списку.
        t.protocol = Some(json!({"повестка": ["Смета"], "итог": "Смета согласована."}));
        assert_eq!(protocol(&t, &[]).unwrap(), "# Протокол: Планёрка\n\n## Повестка\n\n- Смета\n\n## Итог\n\nСмета согласована.\n");
    }
}
