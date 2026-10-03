//! Запись с микрофона с расшифровкой на ходу. Звук с устройства (cpal) приводится к 16 кГц
//! и пишется в WAV-файл в папке записи; параллельно поиск речи режет его на фразы, и каждая
//! сразу распознаётся — текст появляется в окне, пока идёт разговор. После остановки файл
//! проходит обычный конвейер целиком: шумоподавление, спикеры, редактура, протокол.

use crate::lang::tr;
use crate::media::SAMPLE_RATE;
use crate::speech::Engines;
use anyhow::{anyhow, Context, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SizedSample};
use std::fs::File;
use std::io::{Seek, SeekFrom, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
use std::time::{Duration, Instant};

/// Имя файла записи в папке записи: 16 кГц, моно, 16 бит.
pub const CAPTURE_FILE: &str = "capture.wav";

/// Микрофоны, с которых можно писать; первый — тот, что выбран в системе.
pub fn input_devices() -> Vec<String> {
    let host = cpal::default_host();
    let default = host.default_input_device().and_then(|d| d.description().ok()).map(|d| d.name().to_string());
    let mut names: Vec<String> = host
        .input_devices()
        .map(|it| it.filter_map(|d| d.description().ok().map(|d| d.name().to_string())).collect())
        .unwrap_or_default();
    if let Some(d) = default {
        names.retain(|n| *n != d);
        names.insert(0, d);
    }
    names
}

/// Открытый поток с микрофона: звук приходит в `rx` кусками, моно, с частотой `rate`.
/// Поток живёт, пока жив `Capture`.
pub struct Capture {
    _stream: cpal::Stream,
    pub rx: Receiver<Vec<f32>>,
    pub rate: u32,
    pub device: String,
}

impl Capture {
    /// `device` — имя микрофона из `input_devices`; пусто — выбранный в системе.
    pub fn open(device: &str) -> Result<Self> {
        let host = cpal::default_host();
        let dev = if device.is_empty() {
            host.default_input_device()
        } else {
            host.input_devices()?
                .find(|d| d.description().ok().is_some_and(|x| x.name() == device))
                .or_else(|| host.default_input_device())
        }
        .ok_or_else(|| anyhow!(tr("микрофон не найден", "no microphone found")))?;
        let name = dev.description().map(|d| d.name().to_string()).unwrap_or_default();
        let supported = dev.default_input_config().context(tr("микрофон недоступен", "the microphone is unavailable"))?;
        let format = supported.sample_format();
        let config: cpal::StreamConfig = supported.into();
        let (tx, rx) = channel();
        let channels = config.channels as usize;
        let stream = match format {
            cpal::SampleFormat::F32 => build::<f32>(&dev, &config, channels, tx)?,
            cpal::SampleFormat::I16 => build::<i16>(&dev, &config, channels, tx)?,
            cpal::SampleFormat::I32 => build::<i32>(&dev, &config, channels, tx)?,
            cpal::SampleFormat::U16 => build::<u16>(&dev, &config, channels, tx)?,
            other => anyhow::bail!("{}: {other:?}", tr("неподдерживаемый формат звука", "unsupported sample format")),
        };
        stream.play().context(tr("не удалось начать запись", "could not start recording"))?;
        Ok(Self { _stream: stream, rx, rate: config.sample_rate, device: name })
    }
}

/// Поток захвата: каналы сводятся в моно, кусок уходит в канал как есть (частота устройства).
fn build<T>(dev: &cpal::Device, config: &cpal::StreamConfig, channels: usize, tx: Sender<Vec<f32>>) -> Result<cpal::Stream>
where
    T: SizedSample,
    f32: FromSample<T>,
{
    let stream = dev.build_input_stream(
        config.clone(),
        move |data: &[T], _| {
            let mono: Vec<f32> = data
                .chunks(channels.max(1))
                .map(|frame| frame.iter().map(|&x| f32::from_sample_(x)).sum::<f32>() / frame.len() as f32)
                .collect();
            let _ = tx.send(mono);
        },
        |e| eprintln!("микрофон: {e}"),
        None,
    )?;
    Ok(stream)
}

// ---------- WAV ----------

/// WAV-файл записи. Размеры в заголовке проставляются при закрытии; до тех пор там
/// «неизвестно» (0xFFFFFFFF), и ffmpeg читает такой файл до конца — если приложение
/// закрыли посреди записи, файл всё равно пригоден (см. `repair`).
pub struct Wav {
    file: File,
    bytes: u32,
}

const HEADER: usize = 44;

impl Wav {
    pub fn create(path: &Path) -> Result<Self> {
        let mut file = File::create(path)?;
        file.write_all(&header(u32::MAX))?;
        Ok(Self { file, bytes: 0 })
    }

    pub fn write(&mut self, samples: &[f32]) -> Result<()> {
        let bytes: Vec<u8> = samples
            .iter()
            .flat_map(|&x| ((x.clamp(-1.0, 1.0) * 32767.0).round() as i16).to_le_bytes())
            .collect();
        self.file.write_all(&bytes)?;
        self.bytes = self.bytes.saturating_add(bytes.len() as u32);
        Ok(())
    }

    /// Секунд записано.
    pub fn seconds(&self) -> f32 {
        self.bytes as f32 / 2.0 / SAMPLE_RATE as f32
    }

    pub fn finish(mut self) -> Result<()> {
        self.file.seek(SeekFrom::Start(0))?;
        self.file.write_all(&header(self.bytes))?;
        self.file.flush()?;
        Ok(())
    }

    /// Проставляет размеры в заголовке по длине файла — для записи, оборванной закрытием приложения.
    pub fn repair(path: &Path) -> Result<()> {
        let len = std::fs::metadata(path)?.len();
        let data = len.saturating_sub(HEADER as u64).min(u32::MAX as u64) as u32 & !1;
        let mut file = std::fs::OpenOptions::new().write(true).open(path)?;
        file.write_all(&header(data))?;
        Ok(())
    }
}

/// Заголовок WAV: 16 кГц, моно, 16 бит; `data` — размер данных в байтах.
fn header(data: u32) -> [u8; HEADER] {
    let mut h = [0u8; HEADER];
    let rate = SAMPLE_RATE as u32;
    h[0..4].copy_from_slice(b"RIFF");
    h[4..8].copy_from_slice(&data.saturating_add(36).to_le_bytes());
    h[8..12].copy_from_slice(b"WAVE");
    h[12..16].copy_from_slice(b"fmt ");
    h[16..20].copy_from_slice(&16u32.to_le_bytes());
    h[20..22].copy_from_slice(&1u16.to_le_bytes()); // PCM
    h[22..24].copy_from_slice(&1u16.to_le_bytes()); // моно
    h[24..28].copy_from_slice(&rate.to_le_bytes());
    h[28..32].copy_from_slice(&(rate * 2).to_le_bytes());
    h[32..34].copy_from_slice(&2u16.to_le_bytes());
    h[34..36].copy_from_slice(&16u16.to_le_bytes());
    h[36..40].copy_from_slice(b"data");
    h[40..44].copy_from_slice(&data.to_le_bytes());
    h
}

// ---------- запись с распознаванием ----------

/// Куда запись сообщает о ходе дела.
pub trait Listen {
    /// Распознана фраза, начавшаяся на `start`-й секунде записи.
    fn text(&self, start: f32, text: String);
    /// Сколько секунд записано и пиковая громкость (0…1) за последнее время.
    fn tick(&self, seconds: f32, peak: f32);
}

const TICK: Duration = Duration::from_millis(200);

/// Пишет звук из `capture` в `wav`, распознавая фразы по мере появления, пока не поднят `stop`.
/// Возвращает длительность записи в секундах.
pub fn run(capture: &Capture, engines: &Engines, wav: &mut Wav, stop: &AtomicBool, listen: &dyn Listen) -> Result<f32> {
    let resampler = (capture.rate != SAMPLE_RATE as u32)
        .then(|| sherpa_onnx::LinearResampler::create(capture.rate as i32, SAMPLE_RATE))
        .map(|r| r.ok_or_else(|| anyhow!(tr("не удалось создать ресемплер", "could not create a resampler"))))
        .transpose()?;
    let vad = engines.vad()?;
    // VAD принимает окна по 512 отсчётов; хвост ждёт следующего куска.
    let mut pending: Vec<f32> = vec![];
    let (mut peak, mut last_tick) = (0.0f32, Instant::now());
    let recognize = |vad: &sherpa_onnx::VoiceActivityDetector| {
        while !vad.is_empty() {
            if let Some(seg) = vad.front() {
                let start = seg.start() as usize;
                let samples = seg.samples().to_vec();
                vad.pop();
                for s in engines.recognize(&[(start, samples)], |_, _| {}) {
                    if !s.words.is_empty() {
                        listen.text(s.start, crate::pipeline::capitalize(&s.text()));
                    }
                }
            } else {
                vad.pop();
            }
        }
    };
    loop {
        let chunk = match capture.rx.recv_timeout(TICK) {
            Ok(c) => Some(c),
            Err(RecvTimeoutError::Timeout) => None,
            Err(RecvTimeoutError::Disconnected) => anyhow::bail!(tr("микрофон отключился", "the microphone was disconnected")),
        };
        if let Some(chunk) = chunk {
            let samples = match &resampler {
                Some(r) => r.resample(&chunk, false),
                None => chunk,
            };
            wav.write(&samples)?;
            peak = samples.iter().fold(peak, |m, x| m.max(x.abs()));
            pending.extend_from_slice(&samples);
            let whole = pending.len() / 512 * 512;
            for w in pending[..whole].chunks(512) {
                vad.accept_waveform(w);
            }
            pending.drain(..whole);
            recognize(&vad);
        }
        if last_tick.elapsed() >= TICK {
            listen.tick(wav.seconds(), peak.min(1.0));
            (peak, last_tick) = (0.0, Instant::now());
        }
        if stop.load(Ordering::Relaxed) {
            break;
        }
    }
    // Хвост: что осталось в ресемплере и в поиске речи.
    if let Some(r) = &resampler {
        let tail = r.resample(&[], true);
        wav.write(&tail)?;
        pending.extend_from_slice(&tail);
    }
    vad.accept_waveform(&pending);
    vad.flush();
    recognize(&vad);
    Ok(wav.seconds())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wav_header_is_patched_on_finish_and_repair() {
        let dir = std::env::temp_dir().join(format!("ut-wav-{}", uuid::Uuid::new_v4().simple()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(CAPTURE_FILE);
        let mut w = Wav::create(&path).unwrap();
        w.write(&[0.0, 0.5, -0.5, 1.0]).unwrap();
        assert!((w.seconds() - 4.0 / 16000.0).abs() < 1e-6);
        // До закрытия размер «неизвестен».
        let raw = std::fs::read(&path).unwrap();
        assert_eq!(&raw[40..44], &u32::MAX.to_le_bytes());
        assert_eq!(raw.len(), HEADER + 8);
        w.finish().unwrap();
        let raw = std::fs::read(&path).unwrap();
        assert_eq!(&raw[40..44], &8u32.to_le_bytes());
        assert_eq!(&raw[4..8], &44u32.to_le_bytes());
        // Оборванный файл чинится по длине.
        std::fs::write(&path, [&header(u32::MAX)[..], &[0u8; 10][..]].concat()).unwrap();
        Wav::repair(&path).unwrap();
        let raw = std::fs::read(&path).unwrap();
        assert_eq!(&raw[40..44], &10u32.to_le_bytes());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
