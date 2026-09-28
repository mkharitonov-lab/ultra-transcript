//! Работа с любыми аудио/видео через ffmpeg.

use crate::lang::tr;
use anyhow::{bail, Context, Result};
use std::path::{Path, PathBuf};
use std::process::Command;

pub const SAMPLE_RATE: i32 = 16_000;

pub const MEDIA_EXTENSIONS: &[&str] = &[
    "mp3", "m4a", "wav", "aac", "ogg", "opus", "flac", "wma", "amr", "aiff", "aif", "caf", "3gp",
    "mp4", "mov", "mkv", "avi", "webm", "wmv", "m4v", "mpeg", "mpg", "ts",
];

pub fn is_media(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| MEDIA_EXTENSIONS.contains(&e.to_lowercase().as_str()))
        .unwrap_or(false)
}

/// ffmpeg ищется рядом с исполняемым файлом (в поставке), затем в системе.
pub fn ffmpeg() -> PathBuf {
    let exe = if cfg!(windows) { "ffmpeg.exe" } else { "ffmpeg" };
    let mut candidates = vec![];
    if let Ok(cur) = std::env::current_exe() {
        if let Some(dir) = cur.parent() {
            candidates.push(dir.join(exe));
        }
    }
    candidates.push(PathBuf::from("/opt/homebrew/bin/ffmpeg"));
    candidates.push(PathBuf::from("/usr/local/bin/ffmpeg"));
    candidates
        .into_iter()
        .find(|p| p.exists())
        .unwrap_or_else(|| PathBuf::from(exe))
}

/// Декодирует любой файл в 16 кГц моно f32 — формат для всех моделей.
pub fn decode(input: &Path) -> Result<Vec<f32>> {
    decode_part(input, None)
}

/// Как `decode`, но только первые `secs` секунд файла.
pub fn decode_head(input: &Path, secs: u32) -> Result<Vec<f32>> {
    decode_part(input, Some(secs))
}

fn decode_part(input: &Path, secs: Option<u32>) -> Result<Vec<f32>> {
    let mut cmd = Command::new(ffmpeg());
    cmd.args(["-nostdin", "-v", "error", "-i"]).arg(input);
    if let Some(s) = secs {
        cmd.args(["-t", &s.to_string()]);
    }
    let out = cmd
        .args(["-vn", "-ac", "1", "-ar", &SAMPLE_RATE.to_string(), "-f", "f32le", "-"])
        .output()
        .context(tr(
            "не удалось запустить ffmpeg — установите его: brew install ffmpeg",
            "could not run ffmpeg — install it: brew install ffmpeg"
        ))?;
    if !out.status.success() {
        bail!("ffmpeg: {}", String::from_utf8_lossy(&out.stderr).trim());
    }
    Ok(out
        .stdout
        .chunks_exact(4)
        .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        .collect())
}

/// Сжатый архив записи: Opus, моно, битрейт в кбит/с (24 ≈ 11 МБ/час).
pub fn encode_archive(input: &Path, output: &Path, kbps: u32) -> Result<()> {
    encode_opus(input, output, kbps, None)
}

/// Как `encode_archive`, но только первые `secs` секунд файла.
pub fn encode_clip(input: &Path, output: &Path, kbps: u32, secs: u32) -> Result<()> {
    encode_opus(input, output, kbps, Some(secs))
}

fn encode_opus(input: &Path, output: &Path, kbps: u32, secs: Option<u32>) -> Result<()> {
    let mut cmd = Command::new(ffmpeg());
    cmd.args(["-nostdin", "-v", "error", "-y", "-i"]).arg(input);
    if let Some(s) = secs {
        cmd.args(["-t", &s.to_string()]);
    }
    let out = cmd
        .args(["-vn", "-ac", "1", "-c:a", "libopus", "-application", "voip"])
        .args(["-b:a", &format!("{kbps}k")])
        .arg(output)
        .output()
        .context(tr(
            "не удалось запустить ffmpeg — установите его: brew install ffmpeg",
            "could not run ffmpeg — install it: brew install ffmpeg"
        ))?;
    if !out.status.success() {
        bail!("ffmpeg: {}", String::from_utf8_lossy(&out.stderr).trim());
    }
    Ok(())
}
