//! GLM-ASR-Nano (Zhipu AI): распознавание языковой моделью. Аудиоэнкодер (как у Whisper)
//! превращает фрагмент речи в эмбеддинги, LLM на 1,5 млрд параметров пишет по ним текст.
//! Работает в llama.cpp (mtmd) — том же, что и встроенный ИИ-помощник; на Mac — на Metal.
//! Таймкодов слов модель не даёт: слова распределяются по фрагменту, как у Whisper.

use crate::lang::tr;
use anyhow::{anyhow, Result};
use llama_cpp_2::context::params::LlamaContextParams;
use llama_cpp_2::model::params::LlamaModelParams;
use llama_cpp_2::model::LlamaModel;
use llama_cpp_2::mtmd::{mtmd_default_marker, MtmdBitmap, MtmdContext, MtmdContextParams, MtmdInputText};
use llama_cpp_2::sampling::LlamaSampler;
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

pub const MODEL_FILE: &str = "GLM-ASR-Nano-1.6B-2512-Q4_K.gguf";
pub const MMPROJ_FILE: &str = "mmproj-GLM-ASR-Nano-2512-Q8_0.gguf";

/// Запрос по чат-шаблону модели (chat_template.jinja); `{task}` — инструкция.
const PROMPT: &str = "<|user|>\n<|begin_of_audio|>{media}<|end_of_audio|><|user|>\n{task}<|assistant|>\n";

/// Языки GLM-ASR-Nano (по FLEURS: WER до 10 % у первых восьми, до 20 % у остальных).
pub const LANGUAGES: [(&str, &str); 17] = [
    ("it", "Italian"), ("en", "English"), ("ca", "Catalan"), ("uk", "Ukrainian"), ("nl", "Dutch"), ("es", "Spanish"),
    ("de", "German"), ("zh", "Chinese"), ("ja", "Japanese"), ("fr", "French"), ("ru", "Russian"), ("pt", "Portuguese"),
    ("ms", "Malay"), ("id", "Indonesian"), ("no", "Norwegian"), ("lt", "Lithuanian"), ("sl", "Slovenian"),
];

/// Инструкция модели. Со стандартной («Please transcribe this audio into text») русскую речь
/// модель часто переводит на английский: на 30 фразах FLEURS ru — 12 переводов. Инструкция на
/// самом языке записи переводов не даёт (WER 40 % против 47 % у английской с названием языка).
fn task(language: &str) -> String {
    match language {
        "ru" => "Распознай речь на русском языке".into(),
        "uk" => "Розпізнай мовлення українською мовою".into(),
        "" => "Please transcribe this audio into text".into(),
        code => match LANGUAGES.iter().find(|(c, _)| *c == code) {
            Some((_, name)) => format!("Please transcribe this audio into {name} text"),
            None => "Please transcribe this audio into text".into(),
        },
    }
}

/// Модель в памяти — общая, как у встроенного ИИ-помощника: её нужно выгрузить до выхода
/// (`unload`), иначе llama.cpp падает, освобождая видеопамять при завершении процесса.
/// Мьютекс заодно не даёт расшифровывать двумя потоками сразу.
static LOADED: Mutex<Option<Loaded>> = Mutex::new(None);

struct Loaded {
    dir: PathBuf,
    model: LlamaModel,
    mtmd: MtmdContext,
}

/// Выгрузить модель из памяти.
pub fn unload() {
    *LOADED.lock().unwrap() = None;
}

/// Распознавание GLM-ASR из папки модели. Модель загружается сразу, а выгруженная
/// (`unload`) — снова при следующем фрагменте; уходит из памяти вместе с этим значением.
pub struct GlmAsr {
    dir: PathBuf,
}

impl Drop for GlmAsr {
    fn drop(&mut self) {
        let mut guard = LOADED.lock().unwrap();
        if guard.as_ref().is_some_and(|l| l.dir == self.dir) {
            *guard = None;
        }
    }
}

impl GlmAsr {
    pub fn load(dir: &Path) -> Result<Self> {
        let mut guard = LOADED.lock().unwrap();
        if guard.as_ref().is_none_or(|l| l.dir != dir) {
            *guard = None;
            *guard = Some(Loaded::load(dir)?);
        }
        Ok(Self { dir: dir.to_path_buf() })
    }

    /// Текст фрагмента речи (16 кГц, моно). `language` — код языка записи; пусто — модель
    /// определяет язык сама. Пустой ответ — модель ничего не услышала.
    pub fn transcribe(&self, samples: &[f32], language: &str) -> Result<String> {
        let mut guard = LOADED.lock().unwrap();
        if guard.as_ref().is_none_or(|l| l.dir != self.dir) {
            *guard = None;
            *guard = Some(Loaded::load(&self.dir)?);
        }
        guard.as_ref().unwrap().transcribe(samples, language)
    }
}

impl Loaded {
    fn load(dir: &Path) -> Result<Self> {
        let fail = |e: String| anyhow!("{} GLM-ASR-Nano: {e}", tr("не удалось загрузить модель распознавания", "could not load the speech model"));
        let params = LlamaModelParams::default().with_n_gpu_layers(999);
        let model = LlamaModel::load_from_file(crate::local_llm::backend(), dir.join(MODEL_FILE), &params).map_err(|e| fail(e.to_string()))?;
        let mparams = MtmdContextParams {
            use_gpu: true,
            print_timings: false,
            n_threads: crate::speech::threads(),
            ..MtmdContextParams::default()
        };
        let mtmd = MtmdContext::init_from_file(&dir.join(MMPROJ_FILE).to_string_lossy(), &model, &mparams)
            .map_err(|e| fail(e.to_string()))?;
        if !mtmd.support_audio() {
            return Err(fail("no audio encoder".into()));
        }
        Ok(Self { dir: dir.to_path_buf(), model, mtmd })
    }

    fn transcribe(&self, samples: &[f32], language: &str) -> Result<String> {
        let bitmap = MtmdBitmap::from_audio_data(samples)?;
        let text = MtmdInputText {
            text: PROMPT.replace("{media}", mtmd_default_marker()).replace("{task}", &task(language)),
            add_special: true,
            parse_special: true,
        };
        let chunks = self.mtmd.tokenize(text, &[&bitmap])?;
        // Ответ: речь бывает быстрой, но не быстрее ~8 токенов в секунду; запас — на случай зацикливания.
        let seconds = samples.len() as f32 / crate::media::SAMPLE_RATE as f32;
        let max_answer = 32 + (seconds * 12.0) as usize;
        let n_ctx = chunks.total_tokens() + max_answer + 16;
        let params = LlamaContextParams::default()
            .with_n_ctx(NonZeroU32::new(n_ctx as u32))
            .with_n_batch(n_ctx as u32)
            .with_n_ubatch(n_ctx as u32)
            .with_n_threads(crate::speech::threads())
            .with_n_threads_batch(crate::speech::threads());
        let mut ctx = self.model.new_context(crate::local_llm::backend(), params)?;
        let mut pos = chunks.eval_chunks(&self.mtmd, &ctx, 0, 0, n_ctx as i32, true)?;

        let mut sampler = LlamaSampler::greedy();
        let mut decoder = encoding_rs::UTF_8.new_decoder();
        let mut out = String::new();
        let mut batch = llama_cpp_2::llama_batch::LlamaBatch::new(1, 1);
        for _ in 0..max_answer {
            let tok = sampler.sample(&ctx, -1);
            if self.model.is_eog_token(tok) {
                break;
            }
            sampler.accept(tok);
            out.push_str(&self.model.token_to_piece(tok, &mut decoder, false, None)?);
            batch.clear();
            batch.add(tok, pos, &[0], true)?;
            ctx.decode(&mut batch)?;
            pos += 1;
        }
        Ok(out.trim().to_string())
    }
}

#[cfg(test)]
mod tests {
    /// Живая модель: `GLM_ASR_DIR` — папка с весами, `GLM_ASR_WAVS` — аудиофайлы через двоеточие.
    #[test]
    #[ignore]
    fn transcribes_wavs() {
        let dir = std::env::var("GLM_ASR_DIR").unwrap();
        let glm = super::GlmAsr::load(dir.as_ref()).unwrap();
        for path in std::env::var("GLM_ASR_WAVS").unwrap().split(':') {
            let s = crate::media::decode(path.as_ref()).unwrap();
            let t = std::time::Instant::now();
            let text = glm.transcribe(&s, &std::env::var("GLM_ASR_LANG").unwrap_or_default()).unwrap();
            println!("{path}\t{:.2}\t{text}", t.elapsed().as_secs_f32() / (s.len() as f32 / 16000.0));
        }
    }
}
