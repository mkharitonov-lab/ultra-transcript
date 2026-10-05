//! Каталог моделей. Модели не входят в приложение: пользователь скачивает их с серверов
//! разработчиков (выпуски sherpa-onnx на GitHub, Hugging Face), у каждой своя лицензия.

use crate::audio;
use crate::diar::{self, DiarModel};
use crate::lang::tr;
use crate::speech::{self, AsrModel};
use anyhow::{bail, Result};
use serde::Serialize;
use std::io::{Read, Write};
use std::path::Path;

const BASE: &str = "https://github.com/k2-fsa/sherpa-onnx/releases/download";

#[derive(Clone, Serialize)]
pub struct Model {
    pub name: &'static str,
    /// core — нужны всегда; asr и diar — нужна только выбранная; llm — по желанию;
    /// denoise — скачивается вместе с обязательными, но без неё расшифровка тоже работает.
    pub kind: &'static str,
    pub asr: Option<AsrModel>,
    pub diar: Option<DiarModel>,
    pub title: &'static str,
    /// Для чего модель — на языке интерфейса.
    pub about: &'static str,
    /// Кто выпустил модель.
    pub publisher: &'static str,
    pub license: &'static str,
    pub url: String,
    /// Модель из нескольких файлов: они скачиваются в папку `target` (тогда `url` пуст).
    #[serde(skip)]
    pub files: Vec<String>,
    /// Что должно появиться в каталоге моделей после установки.
    pub target: &'static str,
    pub size_mb: u32,
    pub installed: bool,
    /// Нужна при текущих настройках (заполняет `with_required`).
    pub required: bool,
}

pub fn catalog(dir: &Path) -> Vec<Model> {
    let m = |name, title, about, publisher, license, url: String, target, size_mb| Model {
        name,
        kind: "core",
        asr: None,
        diar: None,
        title,
        about,
        publisher,
        license,
        installed: dir.join(target).exists(),
        required: false,
        url,
        files: vec![],
        target,
        size_mb,
    };
    let asr = |name, a: AsrModel, title, about, publisher, size_mb| Model {
        kind: "asr",
        asr: Some(a),
        ..m(name, title, about, publisher, "MIT", format!("{BASE}/asr-models/{}.tar.bz2", a.dir()), a.dir(), size_mb)
    };
    let llm = |name, title, about, publisher, license, repo: &str, file, size_mb| Model {
        kind: "llm",
        ..m(name, title, about, publisher, license, format!("https://huggingface.co/{repo}/resolve/main/{file}"), file, size_mb)
    };
    let diar = |name, d: DiarModel, title, about, publisher, license, url: String, target, size_mb| Model {
        kind: "diar",
        diar: Some(d),
        ..m(name, title, about, publisher, license, url, target, size_mb)
    };
    // Отпечатки WeSpeaker с маской спикера в ONNX (экспорт проекта speakrs, сверен с pyannote) и PLDA
    // из pyannote community-1; ревизии зафиксированы.
    let speakrs = "https://huggingface.co/avencera/speakrs-models/resolve/a785ebdbe6313868088c36c93d9efa71c470bd34";
    let community = "https://huggingface.co/pyannote-community/speaker-diarization-community-1/resolve/8a527374977391da736e0daaef26855d949d9685";
    vec![
        asr("gigaam", AsrModel::Gigaam, "GigaAM v3",
            tr("Русская речь с пунктуацией", "Russian speech with punctuation"), tr("Сбер", "Sber"), 170),
        asr("whisper-turbo", AsrModel::WhisperTurbo, "Whisper large-v3-turbo",
            tr("Речь на разных языках", "Speech in many languages"), "OpenAI", 563),
        asr("whisper-large", AsrModel::WhisperLarge, "Whisper large-v3",
            tr("Речь на разных языках, точнее и медленнее turbo", "Speech in many languages, more accurate and slower than turbo"),
            "OpenAI", 1068),
        Model {
            license: "Apache 2.0",
            ..asr("t-one", AsrModel::Tone, "T-one",
                  tr("Русская речь, телефонные разговоры", "Russian speech, phone calls"), tr("Т-Банк", "T-Bank"), 128)
        },
        Model {
            license: "CC BY 4.0",
            ..asr("parakeet", AsrModel::Parakeet, "Parakeet TDT 0.6B v3",
                  tr("Речь на 25 европейских языках", "Speech in 25 European languages"), "NVIDIA", 487)
        },
        Model {
            license: "Apache 2.0",
            files: [crate::glm_asr::MODEL_FILE, crate::glm_asr::MMPROJ_FILE]
                .map(|f| format!("https://huggingface.co/concedo/GLM-ASR-Nano-2512-GGUF/resolve/c86e98608bc49264a1526d35a74174f99064240c/{f}"))
                .into(),
            url: String::new(),
            ..asr("glm-asr", AsrModel::GlmAsr, "GLM-ASR-Nano",
                  tr("Речь на 17 языках, распознаёт языковая модель", "Speech in 17 languages, recognized by a language model"),
                  "Zhipu AI", 1700)
        },
        m("vad", "Silero VAD", tr("Поиск речи в записи", "Finds speech in a recording"), "Silero", "MIT",
          format!("{BASE}/asr-models/{}", speech::VAD_FILE), speech::VAD_FILE, 1),
        m("segmentation", "pyannote segmentation 3.0", tr("Границы реплик", "Finds who speaks when"), "pyannote", "MIT",
          format!("{BASE}/speaker-segmentation-models/{}.tar.bz2", speech::SEGMENTATION_DIR),
          speech::SEGMENTATION_DIR, 7),
        m("embedding", "WeSpeaker ResNet34-LM", tr("Голосовые профили", "Voice profiles"), "WeSpeaker", "CC BY 4.0",
          format!("{BASE}/speaker-recongition-models/{}", speech::EMBEDDING_FILE), speech::EMBEDDING_FILE, 27),
        Model {
            kind: "denoise",
            ..m("denoiser", "DPDFNet", tr("Подавление шума", "Noise reduction"), "Ceva", "Apache 2.0",
                format!("{BASE}/speech-enhancement-models/{}", audio::DENOISER_FILE), audio::DENOISER_FILE, 9)
        },
        Model {
            files: ["wespeaker-fbank-b32.onnx", "wespeaker-resnet-frames-b32.onnx", "wespeaker-pool-classify-b3.onnx"]
                .map(|f| format!("{speakrs}/{f}"))
                .into_iter()
                .chain(["xvec_transform.npz", "plda.npz"].map(|f| format!("{community}/plda/{f}")))
                .collect(),
            ..diar("community1", DiarModel::Community1, "pyannote community-1",
                   tr("Разделение по спикерам", "Speaker separation"), "pyannote",
                   "CC BY 4.0", String::new(), diar::COMMUNITY1_DIR, 27)
        },
        diar("nemotron3", DiarModel::Nemotron3, "Nemotron 3 Diarization",
             tr("Разделение по спикерам", "Speaker separation"), "NVIDIA", "OpenMDW 1.1",
             format!("https://huggingface.co/nvidia/Nemotron-3-Diarization/resolve/f667ed73aee57d40cc39428eb768b4fd87a0a29e/{}",
                     diar::nemotron::MODEL_FILE),
             diar::nemotron::MODEL_FILE, 107),
        llm("gigachat-lightning", "GigaChat 3.1 Lightning",
            tr("ИИ-помощник", "AI assistant"),
            tr("Сбер", "Sber"), "MIT", "ai-sage/GigaChat3.1-10B-A1.8B-GGUF", "GigaChat3.1-10B-A1.8B-q4_K_M.gguf", 6470),
        llm("t-lite", "T-lite 2.1",
            tr("ИИ-помощник", "AI assistant"),
            tr("Т-Банк", "T-Bank"), "Apache 2.0", "t-tech/T-lite-it-2.1-GGUF", "T-lite-it-2.1-Q4_K_M.gguf", 5030),
        llm("yandexgpt-lite", "YandexGPT-5 Lite 8B",
            tr("ИИ-помощник", "AI assistant"),
            tr("Яндекс", "Yandex"),
            tr("лицензия Яндекса, с ограничениями", "Yandex license, restrictions apply"),
            "yandex/YandexGPT-5-Lite-8B-instruct-GGUF", "YandexGPT-5-Lite-8B-instruct-Q4_K_M.gguf", 4920),
        llm("qwen3-4b", "Qwen3 4B Instruct",
            tr("ИИ-помощник", "AI assistant"),
            "Alibaba", "Apache 2.0", "unsloth/Qwen3-4B-Instruct-2507-GGUF", "Qwen3-4B-Instruct-2507-Q4_K_M.gguf", 2500),
    ]
}

/// Каталог с отметкой, какие модели нужны при выбранных моделях распознавания и диаризации.
pub fn with_required(dir: &Path, asr: AsrModel, diar: DiarModel) -> Vec<Model> {
    let mut all = catalog(dir);
    for m in &mut all {
        m.required = m.needed_for(asr, diar);
    }
    all
}

impl Model {
    /// Нужна ли модель, чтобы расшифровывать выбранными моделями распознавания и диаризации.
    pub fn needed_for(&self, asr: AsrModel, diar: DiarModel) -> bool {
        self.kind == "core" || self.asr == Some(asr) || self.diar == Some(diar)
    }
}

/// Модели, без которых нельзя расшифровать выбранными моделями распознавания и диаризации.
pub fn required(dir: &Path, asr: AsrModel, diar: DiarModel) -> Vec<Model> {
    catalog(dir).into_iter().filter(|m| m.needed_for(asr, diar)).collect()
}

pub fn ready(dir: &Path, asr: AsrModel, diar: DiarModel) -> bool {
    required(dir, asr, diar).iter().all(|m| m.installed)
}

pub fn find(dir: &Path, name: &str) -> Option<Model> {
    catalog(dir).into_iter().find(|m| m.name == name)
}

pub fn remove(dir: &Path, m: &Model) -> Result<()> {
    let p = dir.join(m.target);
    if p.is_dir() {
        std::fs::remove_dir_all(p)?;
    } else if p.exists() {
        std::fs::remove_file(p)?;
    }
    Ok(())
}

/// Скачивает модель (с докачкой после обрыва); архивы .tar.bz2 распаковываются.
/// `progress` — доля 0..1.
pub fn install(dir: &Path, m: &Model, progress: &dyn Fn(f32)) -> Result<()> {
    std::fs::create_dir_all(dir)?;
    let client = reqwest::blocking::Client::builder().timeout(None).build()?;
    let tmp = dir.join(format!("{}.part", m.name));
    if !m.files.is_empty() {
        // Файлы собираются во временной папке: готовая папка появляется, только когда скачано всё.
        std::fs::create_dir_all(&tmp)?;
        let n = m.files.len() as f32;
        for (i, url) in m.files.iter().enumerate() {
            let name = url.rsplit('/').next().unwrap_or(url);
            if !tmp.join(name).exists() {
                let part = tmp.join(format!("{name}.part"));
                fetch(&client, url, &part, m.size_mb, &|p| progress((i as f32 + p) / n))?;
                std::fs::rename(&part, tmp.join(name))?;
            }
        }
        std::fs::rename(&tmp, dir.join(m.target))?;
        return Ok(());
    }
    fetch(&client, &m.url, &tmp, m.size_mb, progress)?;
    if m.url.ends_with(".tar.bz2") {
        tar::Archive::new(bzip2::read::BzDecoder::new(std::fs::File::open(&tmp)?)).unpack(dir)?;
        std::fs::remove_file(&tmp)?;
    } else {
        std::fs::rename(&tmp, dir.join(m.target))?;
    }
    Ok(())
}

/// Скачивает `url` в `tmp`, продолжая с места обрыва.
fn fetch(client: &reqwest::blocking::Client, url: &str, tmp: &Path, size_mb: u32, progress: &dyn Fn(f32)) -> Result<()> {
    let have = std::fs::metadata(tmp).map(|x| x.len()).unwrap_or(0);
    let mut req = client.get(url);
    if have > 0 {
        req = req.header("Range", format!("bytes={have}-"));
    }
    let mut resp = req.send()?;
    if !resp.status().is_success() {
        bail!("{} {url}: HTTP {}", tr("не удалось скачать", "could not download"), resp.status());
    }
    let resumed = resp.status() == reqwest::StatusCode::PARTIAL_CONTENT;
    let done0 = if resumed { have as usize } else { 0 };
    let total = (resp.content_length().map(|n| n as usize + done0))
        .unwrap_or(size_mb as usize * 1_000_000) as f32;
    let mut file = std::fs::OpenOptions::new().create(true).append(resumed).write(true).truncate(!resumed).open(tmp)?;
    let (mut buf, mut done) = (vec![0u8; 1 << 16], done0);
    loop {
        let n = resp.read(&mut buf)?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n])?;
        done += n;
        progress((done as f32 / total).min(1.0));
    }
    Ok(())
}
