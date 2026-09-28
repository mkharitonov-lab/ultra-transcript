//! Работа с любыми аудио/видео через ffmpeg.

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
    let out = Command::new(ffmpeg())
        .args(["-nostdin", "-v", "error", "-i"])
        .arg(input)
        .args(["-vn", "-ac", "1", "-ar", &SAMPLE_RATE.to_string(), "-f", "f32le", "-"])
        .output()
        .context("не удалось запустить ffmpeg")?;
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
    let out = Command::new(ffmpeg())
        .args(["-nostdin", "-v", "error", "-y", "-i"])
        .arg(input)
        .args(["-vn", "-ac", "1", "-c:a", "libopus", "-application", "voip"])
        .args(["-b:a", &format!("{kbps}k")])
        .arg(output)
        .output()
        .context("не удалось запустить ffmpeg")?;
    if !out.status.success() {
        bail!("ffmpeg: {}", String::from_utf8_lossy(&out.stderr).trim());
    }
    Ok(())
}
