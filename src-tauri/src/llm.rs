//! LLM: встроенная (llama.cpp в процессе) или внешняя через OpenAI-совместимый API
//! (Ollama, LM Studio, облачный сервис с custom endpoint). Ответ всегда JSON.

use crate::lang::tr;
use crate::store::{self, LlmProvider, Settings};
use crate::{local_llm, models};
use anyhow::{anyhow, bail, Context, Result};
use serde_json::{json, Value};
use std::cell::RefCell;
use std::io::{BufRead, BufReader};
use std::time::Duration;

/// `schema` — JSON Schema ответа (строкой, чтобы сохранить порядок полей).
pub fn chat_json(s: &Settings, system: &str, user: &str, schema: &str) -> Result<Value> {
    chat(s, system, user, schema, None)
}

/// Как `chat_json`, но ответ виден, пока он пишется: `on_text` получает его по кускам.
/// Куски вместе — JSON без рассуждений модели и обёрток.
pub fn chat_json_live(s: &Settings, system: &str, user: &str, schema: &str, on_text: &dyn Fn(&str)) -> Result<Value> {
    chat(s, system, user, schema, Some(on_text))
}

fn chat(s: &Settings, system: &str, user: &str, schema: &str, on_text: Option<&dyn Fn(&str)>) -> Result<Value> {
    match s.llm_provider {
        LlmProvider::Builtin => {
            let dir = store::models_dir();
            let m = models::find(&dir, &s.llm_local_model)
                .ok_or_else(|| anyhow!(tr("не выбрана встроенная модель", "no built-in model is selected")))?;
            local_llm::chat_json(&dir.join(m.target), system, user, schema, on_text.unwrap_or(&|_| {}))
        }
        LlmProvider::Api => api_chat_json(s, system, user, on_text),
    }
}

/// Сколько символов текста подавать одним запросом: у встроенных моделей контекст меньше.
pub fn prompt_budget(s: &Settings) -> usize {
    match s.llm_provider {
        LlmProvider::Builtin => local_llm::PROMPT_CHARS,
        LlmProvider::Api => 200_000,
    }
}

fn api_chat_json(s: &Settings, system: &str, user: &str, on_text: Option<&dyn Fn(&str)>) -> Result<Value> {
    let url = format!("{}/chat/completions", s.llm_base_url.trim_end_matches('/'));
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(600))
        .build()?;
    let mut req = client.post(&url).json(&json!({
        "model": s.llm_model,
        "temperature": 0.1,
        "response_format": {"type": "json_object"},
        "stream": on_text.is_some(),
        "messages": [
            {"role": "system", "content": system},
            {"role": "user", "content": user},
        ],
    }));
    if !s.llm_api_key.is_empty() {
        req = req.bearer_auth(&s.llm_api_key);
    }
    let resp = req
        .send()
        .with_context(|| format!("{} ({url})", tr("языковая модель недоступна", "the language model is unreachable")))?;
    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().unwrap_or_default();
        bail!("{} {status}: {}", tr("языковая модель ответила", "the language model answered"), body.trim());
    }
    let empty = || anyhow!(tr("языковая модель вернула пустой ответ", "the language model returned an empty answer"));
    let Some(on_text) = on_text else {
        let body: Value = resp.json()?;
        return parse_json(body["choices"][0]["message"]["content"].as_str().ok_or_else(empty)?);
    };
    // Поток событий: строки «data: {…}», в конце «data: [DONE]».
    let shown = JsonOnly::default();
    let mut content = String::new();
    for line in BufReader::new(resp).lines() {
        let line = line?;
        let Some(data) = line.strip_prefix("data:").map(str::trim) else { continue };
        if data == "[DONE]" {
            break;
        }
        let Ok(event) = serde_json::from_str::<Value>(data) else { continue };
        if let Some(piece) = event["choices"][0]["delta"]["content"].as_str() {
            content.push_str(piece);
            shown.feed(&content, on_text);
        }
    }
    if content.is_empty() {
        return Err(empty());
    }
    parse_json(&content)
}

/// Из ответа, который ещё пишется, показывает только JSON: рассуждения модели
/// (`<think>…</think>`) и текст перед первой скобкой пропускаются.
#[derive(Default)]
struct JsonOnly {
    /// Сколько байт ответа уже показано.
    sent: RefCell<usize>,
}

impl JsonOnly {
    fn feed(&self, content: &str, on_text: &dyn Fn(&str)) {
        let body = match (content.rfind("<think>"), content.rfind("</think>")) {
            (Some(open), Some(close)) if close < open => return, // модель снова рассуждает
            (Some(_), None) => return,
            (_, Some(close)) => close + "</think>".len(),
            (None, None) => 0,
        };
        let Some(start) = content[body..].find('{').map(|i| body + i) else { return };
        let mut sent = self.sent.borrow_mut();
        let from = (*sent).max(start);
        if from < content.len() {
            on_text(&content[from..]);
            *sent = content.len();
        }
    }
}

/// Достаёт JSON из ответа, даже если модель добавила рассуждения или ```-обёртку.
fn parse_json(text: &str) -> Result<Value> {
    let text = match text.rfind("</think>") {
        Some(i) => &text[i + 8..],
        None => text,
    };
    let (a, b) = (text.find('{'), text.rfind('}'));
    match (a, b) {
        (Some(a), Some(b)) if b > a => Ok(serde_json::from_str(&text[a..=b])?),
        _ => bail!(tr("языковая модель ответила не в том формате", "the language model answered in a wrong format")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shows_only_json_while_the_answer_is_written() {
        let out = RefCell::new(String::new());
        let shown = JsonOnly::default();
        let mut content = String::new();
        for piece in ["<think>", "надо {подумать}", "</think>", "\n```json\n", "{\"тема\"", ": \"Сме", "та\"}"] {
            content.push_str(piece);
            shown.feed(&content, &|t| out.borrow_mut().push_str(t));
        }
        assert_eq!(*out.borrow(), "{\"тема\": \"Смета\"}");
        assert_eq!(parse_json(&content).unwrap()["тема"], "Смета");
    }
}
