//! Каталог моделей: скачиваются внутри приложения при первом запуске.

use crate::speech;
use anyhow::{bail, Result};
use serde::Serialize;
use std::io::{Read, Write};
use std::path::Path;

const BASE: &str = "https://github.com/k2-fsa/sherpa-onnx/releases/download";

#[derive(Clone, Serialize)]
pub struct Model {
    pub name: &'static str,
    pub title: &'static str,
    pub url: String,
    /// Что должно появиться в каталоге моделей после установки.
    pub target: &'static str,
    pub size_mb: u32,
    pub installed: bool,
}

pub fn catalog(dir: &Path) -> Vec<Model> {
    let m = |name, title, url: String, target, size_mb| Model {
        name,
        title,
        installed: dir.join(target).exists(),
        url,
        target,
        size_mb,
    };
    vec![
        m("asr", "GigaAM v3 — распознавание русской речи с пунктуацией",
          format!("{BASE}/asr-models/{}.tar.bz2", speech::ASR_DIR), speech::ASR_DIR, 170),
        m("vad", "Silero VAD — поиск речи",
          format!("{BASE}/asr-models/{}", speech::VAD_FILE), speech::VAD_FILE, 1),
        m("segmentation", "Pyannote 3.0 — сегментация по спикерам",
          format!("{BASE}/speaker-segmentation-models/{}.tar.bz2", speech::SEGMENTATION_DIR),
          speech::SEGMENTATION_DIR, 7),
        m("embedding", "WeSpeaker ResNet34 — голосовые отпечатки",
          format!("{BASE}/speaker-recongition-models/{}", speech::EMBEDDING_FILE), speech::EMBEDDING_FILE, 27),
    ]
}

pub fn all_installed(dir: &Path) -> bool {
    catalog(dir).iter().all(|m| m.installed)
}

/// Скачивает модель; архивы .tar.bz2 распаковываются. `progress` — доля 0..1.
pub fn install(dir: &Path, m: &Model, progress: &dyn Fn(f32)) -> Result<()> {
    std::fs::create_dir_all(dir)?;
    let mut resp = reqwest::blocking::Client::builder().timeout(None).build()?.get(&m.url).send()?;
    if !resp.status().is_success() {
        bail!("{}: HTTP {}", m.url, resp.status());
    }
    let total = resp.content_length().unwrap_or(m.size_mb as u64 * 1_000_000) as f32;
    let tmp = dir.join(format!("{}.part", m.name));
    let mut file = std::fs::File::create(&tmp)?;
    let (mut buf, mut done) = (vec![0u8; 1 << 16], 0usize);
    loop {
        let n = resp.read(&mut buf)?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n])?;
        done += n;
        progress((done as f32 / total).min(1.0));
    }
    drop(file);
    if m.url.ends_with(".tar.bz2") {
        tar::Archive::new(bzip2::read::BzDecoder::new(std::fs::File::open(&tmp)?)).unpack(dir)?;
        std::fs::remove_file(&tmp)?;
    } else {
        std::fs::rename(&tmp, dir.join(m.target))?;
    }
    Ok(())
}
