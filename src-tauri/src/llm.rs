//! LLM через OpenAI-совместимый API. Локальная модель (Ollama, llama-server,
//! LM Studio) и внешний сервис с custom endpoint — один и тот же код.

use crate::store::Settings;
use anyhow::{anyhow, bail, Context, Result};
use serde_json::{json, Value};
use std::time::Duration;

pub fn chat_json(s: &Settings, system: &str, user: &str) -> Result<Value> {
    let url = format!("{}/chat/completions", s.llm_base_url.trim_end_matches('/'));
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(600))
        .build()?;
    let mut req = client.post(&url).json(&json!({
        "model": s.llm_model,
        "temperature": 0.1,
        "response_format": {"type": "json_object"},
        "messages": [
            {"role": "system", "content": system},
            {"role": "user", "content": user},
        ],
    }));
    if !s.llm_api_key.is_empty() {
        req = req.bearer_auth(&s.llm_api_key);
    }
    let resp = req.send().with_context(|| format!("LLM недоступна ({url})"))?;
    let status = resp.status();
    let body: Value = resp.json()?;
    if !status.is_success() {
        bail!("LLM вернула {status}: {body}");
    }
    let content = body["choices"][0]["message"]["content"]
        .as_str()
        .ok_or_else(|| anyhow!("пустой ответ LLM"))?;
    parse_json(content)
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
        _ => bail!("LLM не вернула JSON"),
    }
}
