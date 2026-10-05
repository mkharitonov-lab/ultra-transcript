//! Встроенная LLM: llama.cpp внутри процесса (на Mac — Metal). Без отдельной
//! установки, сервера и сетевых портов. Модель загружается при первом запросе
//! и остаётся в памяти, пока не выбрана другая.

use crate::lang::tr;
use anyhow::{anyhow, bail, Result};
use llama_cpp_2::context::params::LlamaContextParams;
use llama_cpp_2::llama_backend::LlamaBackend;
use llama_cpp_2::llama_batch::LlamaBatch;
use llama_cpp_2::model::params::LlamaModelParams;
use llama_cpp_2::model::{AddBos, LlamaChatMessage, LlamaModel};
use llama_cpp_2::sampling::LlamaSampler;
use llama_cpp_2::token::data::LlamaTokenData;
use llama_cpp_2::token::data_array::LlamaTokenDataArray;
use llama_cpp_2::token::LlamaToken;
use serde_json::Value;
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

/// Ответ ограничен грамматикой JSON — модель физически не может выдать невалидный JSON.
const JSON_GRAMMAR: &str = r#"
root   ::= object
value  ::= object | array | string | number | ("true" | "false" | "null") ws
object ::= "{" ws ( string ":" ws value ("," ws string ":" ws value)* )? "}" ws
array  ::= "[" ws ( value ("," ws value)* )? "]" ws
string ::= "\"" ( [^"\\\x7F\x00-\x1F] | "\\" (["\\/bfnrt] | "u" [0-9a-fA-F]{4}) )* "\"" ws
number ::= ("-"? ([0-9] | [1-9] [0-9]{0,15})) ("." [0-9]+)? ([eE] [-+]? [0-9] [1-9]{0,15})? ws
ws     ::= | " " | "\n" [ \t]{0,20}
"#;

const MAX_ANSWER: usize = 6144;
const MAX_CTX: usize = 32_768;

struct Loaded {
    path: PathBuf,
    model: LlamaModel,
    template: String,
    bos: String,
    eos: String,
}

static LOADED: Mutex<Option<Loaded>> = Mutex::new(None);

pub(crate) fn backend() -> &'static LlamaBackend {
    static B: OnceLock<LlamaBackend> = OnceLock::new();
    B.get_or_init(|| {
        let b = LlamaBackend::init().expect("llama.cpp backend");
        if std::env::var_os("UT_DEBUG").is_none() {
            // Логи llama.cpp и mtmd (GLM-ASR) уходят в tracing, а его в приложении никто не слушает.
            llama_cpp_2::send_logs_to_tracing(llama_cpp_2::LogOptions::default());
        }
        b
    })
}

fn threads() -> i32 {
    std::thread::available_parallelism().map(|n| n.get() as i32).unwrap_or(4).clamp(2, 8)
}

/// Выгрузить модель из памяти (например, после смены модели в настройках).
pub fn unload() {
    *LOADED.lock().unwrap() = None;
}

/// Максимум символов текста, который разумно подавать модели одним запросом.
pub const PROMPT_CHARS: usize = 36_000;

/// `schema` — JSON Schema ответа: из неё строится грамматика, так что структура
/// (поля, массивы, объекты) соблюдается даже маленькой моделью.
/// `on_text` получает ответ по кускам, пока модель его пишет.
pub fn chat_json(path: &Path, system: &str, user: &str, schema: &str, on_text: &dyn Fn(&str)) -> Result<Value> {
    let mut guard = LOADED.lock().unwrap();
    if guard.as_ref().is_none_or(|l| l.path != path) {
        *guard = None;
        anyhow::ensure!(
            path.exists(),
            "{} {} {}",
            tr("модель", "the model"),
            path.file_name().unwrap_or_default().to_string_lossy(),
            tr("не скачана — откройте «Настройки → ИИ-помощник»", "is not downloaded — open Settings → AI assistant")
        );
        let params = LlamaModelParams::default().with_n_gpu_layers(999);
        let model = LlamaModel::load_from_file(backend(), path, &params)
            .map_err(|e| anyhow!("{}: {e}", tr("не удалось загрузить языковую модель", "could not load the language model")))?;
        let piece = |t| {
            model
                .token_to_piece_bytes(t, 64, true, None)
                .map(|b| String::from_utf8_lossy(&b).into_owned())
                .unwrap_or_default()
        };
        let (bos, eos) = (piece(model.token_bos()), piece(model.token_eos()));
        let template = model.meta_val_str("tokenizer.chat_template").unwrap_or_default();
        *guard = Some(Loaded { path: path.to_path_buf(), model, template, bos, eos });
    }
    let l = guard.as_ref().unwrap();
    let prompt = render_prompt(l, system, user)?;
    let grammar = llama_cpp_2::json_schema_to_grammar(schema).unwrap_or_else(|_| JSON_GRAMMAR.to_string());
    let answer = generate(l, &prompt, &grammar, on_text)?;
    serde_json::from_str(&answer)
        .map_err(|e| anyhow!("{}: {e}", tr("языковая модель не дописала ответ", "the language model did not finish its answer")))
}

/// Промпт по родному чат-шаблону модели (Jinja из GGUF). Если шаблон не
/// разбирается — встроенные шаблоны llama.cpp.
fn render_prompt(l: &Loaded, system: &str, user: &str) -> Result<String> {
    let jinja = || -> Result<String, minijinja::Error> {
        let mut env = minijinja::Environment::new();
        minijinja_contrib::add_to_environment(&mut env);
        env.set_unknown_method_callback(minijinja_contrib::pycompat::unknown_method_callback);
        env.add_function("raise_exception", |msg: String| -> Result<String, minijinja::Error> {
            Err(minijinja::Error::new(minijinja::ErrorKind::InvalidOperation, msg))
        });
        env.add_function("strftime_now", |fmt: String| chrono::Local::now().format(&fmt).to_string());
        env.add_template("chat", &l.template)?;
        env.get_template("chat")?.render(minijinja::context! {
            messages => vec![
                minijinja::context! { role => "system", content => system },
                minijinja::context! { role => "user", content => user },
            ],
            add_generation_prompt => true,
            enable_thinking => false,
            bos_token => l.bos,
            eos_token => l.eos,
        })
    };
    if !l.template.is_empty() {
        if let Ok(p) = jinja() {
            return Ok(p);
        }
    }
    let tmpl = l
        .model
        .chat_template(None)
        .map_err(|e| anyhow!("{}: {e}", tr("у модели нет шаблона диалога", "the model has no chat template")))?;
    let msgs = [("system", system), ("user", user)]
        .iter()
        .map(|(r, c)| LlamaChatMessage::new(r.to_string(), c.to_string()))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(l.model.apply_chat_template(&tmpl, &msgs, true)?)
}

fn generate(l: &Loaded, prompt: &str, grammar_src: &str, on_text: &dyn Fn(&str)) -> Result<String> {
    let model = &l.model;
    let add_bos = if !l.bos.is_empty() && !prompt.starts_with(&l.bos) && model.meta_val_str("tokenizer.ggml.add_bos_token").is_ok_and(|v| v == "true") {
        AddBos::Always
    } else {
        AddBos::Never
    };
    let tokens = model.str_to_token(prompt, add_bos)?;
    let limit = (model.n_ctx_train() as usize).min(MAX_CTX);
    if tokens.len() + 512 > limit {
        bail!(
            "{} ({})",
            tr("текст слишком длинный для встроенной модели", "the text is too long for the built-in model"),
            tokens.len()
        );
    }
    let n_ctx = (tokens.len() + MAX_ANSWER).min(limit);
    let params = LlamaContextParams::default()
        .with_n_ctx(NonZeroU32::new(n_ctx as u32))
        .with_n_batch(512)
        .with_n_threads(threads())
        .with_n_threads_batch(threads());
    let mut ctx = model.new_context(backend(), params)?;
    let t0 = std::time::Instant::now();

    let mut batch = LlamaBatch::new(512, 1);
    for (i, chunk) in tokens.chunks(512).enumerate() {
        batch.clear();
        for (j, t) in chunk.iter().enumerate() {
            let pos = i * 512 + j;
            batch.add(*t, pos as i32, &[0], pos == tokens.len() - 1)?;
        }
        ctx.decode(&mut batch)?;
    }

    // Жадный выбор с JSON-грамматикой. Проверять грамматикой весь словарь на каждом
    // шаге дорого (−40% скорости), поэтому сначала проверяем только лучший токен.
    let mut grammar = LlamaSampler::grammar(model, grammar_src, "root")?;
    let mut decoder = encoding_rs::UTF_8.new_decoder();
    let mut out = String::new();
    let mut pos = tokens.len();
    loop {
        let idx = batch.n_tokens() - 1;
        let logits = ctx.get_logits_ith(idx);
        let best = (0..logits.len()).max_by(|&a, &b| logits[a].total_cmp(&logits[b])).unwrap_or(0);
        let mut one = LlamaTokenDataArray::new(
            vec![LlamaTokenData::new(LlamaToken::new(best as i32), logits[best], 0.0)],
            false,
        );
        one.apply_sampler(&grammar);
        let tok = if one.data[0].logit().is_finite() {
            LlamaToken::new(best as i32)
        } else {
            let mut all = ctx.token_data_array_ith(idx);
            all.apply_sampler(&grammar);
            all.sample_token_greedy()
        };
        if model.is_eog_token(tok) {
            break;
        }
        grammar.accept(tok);
        let piece = model.token_to_piece(tok, &mut decoder, false, None)?;
        on_text(&piece);
        out.push_str(&piece);
        if pos >= n_ctx - 1 {
            bail!(tr("ответ языковой модели получился слишком длинным", "the language model's answer is too long"));
        }
        batch.clear();
        batch.add(tok, pos as i32, &[0], true)?;
        ctx.decode(&mut batch)?;
        pos += 1;
    }
    if std::env::var_os("UT_DEBUG").is_some() {
        let gen = pos - tokens.len();
        eprintln!("[llm] промпт {} ток., ответ {} ток., {:.1} с ({:.1} ток/с)",
            tokens.len(), gen, t0.elapsed().as_secs_f32(), gen as f32 / t0.elapsed().as_secs_f32());
    }
    Ok(out)
}
