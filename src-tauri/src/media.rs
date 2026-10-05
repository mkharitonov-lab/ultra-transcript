//! Работа с любыми аудио/видео через ffmpeg.

use crate::lang::tr;
use anyhow::{bail, Context, Result};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

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

/// ffmpeg ищется рядом с исполняемым файлом — там он лежит в поставке (собирается
/// `scripts/build-ffmpeg.sh`, в приложение попадает через `bundle.externalBin`), затем в системе.
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

/// Запуск ffmpeg; на Windows — без окна консоли, иначе оно мелькает при каждом вызове.
#[cfg_attr(not(windows), allow(unused_mut))]
fn command() -> Command {
    let mut cmd = Command::new(ffmpeg());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
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
    let mut cmd = command();
    cmd.args(["-nostdin", "-v", "error", "-i"]).arg(input);
    if let Some(s) = secs {
        cmd.args(["-t", &s.to_string()]);
    }
    let out = cmd
        .args(["-vn", "-ac", "1", "-ar", &SAMPLE_RATE.to_string(), "-f", "f32le", "-"])
        .output()
        .context(no_ffmpeg())?;
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
    let mut cmd = command();
    cmd.args(["-nostdin", "-v", "error", "-y", "-i"]).arg(input);
    if let Some(s) = secs {
        cmd.args(["-t", &s.to_string()]);
    }
    let out = opus(&mut cmd, output, kbps).output().context(no_ffmpeg())?;
    if !out.status.success() {
        bail!("ffmpeg: {}", String::from_utf8_lossy(&out.stderr).trim());
    }
    Ok(())
}

/// Сжатая копия уже декодированного звука (16 кГц моно f32) — в формате архива.
pub fn encode_samples(samples: &[f32], output: &Path, kbps: u32) -> Result<()> {
    let mut cmd = command();
    cmd.args(["-v", "error", "-y", "-f", "f32le", "-ar", &SAMPLE_RATE.to_string(), "-ac", "1", "-i", "-"]);
    let mut child = opus(&mut cmd, output, kbps)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .context(no_ffmpeg())?;
    let mut stdin = child.stdin.take().context("ffmpeg: stdin")?;
    // Если ffmpeg завершился раньше времени, запись оборвётся — причину он сообщит сам.
    for chunk in samples.chunks(1 << 16) {
        let bytes: Vec<u8> = chunk.iter().flat_map(|v| v.to_le_bytes()).collect();
        if stdin.write_all(&bytes).is_err() {
            break;
        }
    }
    drop(stdin);
    let out = child.wait_with_output()?;
    if !out.status.success() {
        bail!("ffmpeg: {}", String::from_utf8_lossy(&out.stderr).trim());
    }
    Ok(())
}

fn opus<'a>(cmd: &'a mut Command, output: &Path, kbps: u32) -> &'a mut Command {
    cmd.args(["-vn", "-ac", "1", "-c:a", "libopus", "-application", "voip"]).args(["-b:a", &format!("{kbps}k")]).arg(output)
}

fn no_ffmpeg() -> &'static str {
    tr(
        "не удалось запустить ffmpeg — он входит в приложение; попробуйте переустановить его",
        "could not run ffmpeg — it ships with the app; try reinstalling it"
    )
}
