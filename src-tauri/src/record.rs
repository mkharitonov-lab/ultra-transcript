//! Запись с микрофона с расшифровкой на ходу. Звук с устройства (cpal) приводится к 16 кГц
//! и пишется в WAV-файл в папке записи; параллельно поиск речи режет его на фразы, и каждая
//! сразу распознаётся — текст появляется в окне, пока идёт разговор. После остановки файл
//! проходит обычный конвейер целиком: шумоподавление, спикеры, протокол.
//!
//! Запись встречи добавляет к микрофону звук компьютера — собеседников из Zoom, Телемоста,
//! браузера. На Mac — глобальный Core Audio process tap всех процессов (systap.rs; macOS 14.4+,
//! отдельное разрешение «Запись системного звука»), на Windows — WASAPI loopback, который cpal
//! снимает с устройства вывода.
//! Оба потока сводятся в один моно-файл.

use crate::lang::tr;
use crate::media::SAMPLE_RATE;
use crate::speech::Engines;
use anyhow::{anyhow, Context, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SizedSample};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
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

/// Что записывать.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    /// Только микрофон: диктовка, разговор в комнате.
    #[default]
    Mic,
    /// Микрофон и звук компьютера: видеовстреча.
    Meeting,
}

/// Открытые потоки: микрофон (дорожка 0) и, для встречи, звук компьютера (дорожка 1).
/// Звук приходит в `rx` кусками `(дорожка, моно-отсчёты)` с частотой своей дорожки из `rates`.
/// Потоки живут, пока жив `Capture`.
pub struct Capture {
    _streams: Vec<cpal::Stream>,
    /// Глобальный tap звука компьютера; уничтожается после потоков.
    #[cfg(target_os = "macos")]
    _tap: Option<crate::systap::GlobalTap>,
    rx: Receiver<(usize, Vec<f32>)>,
    rates: Vec<u32>,
    pub device: String,
}

impl Capture {
    /// `device` — имя микрофона из `input_devices`; пусто — выбранный в системе.
    pub fn open(device: &str, source: Source) -> Result<Self> {
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
        let (tx, rx) = channel();
        let mut streams = vec![];
        let mut rates = vec![];
        let (stream, rate) = open_stream(&dev, supported, 0, tx.clone())?;
        stream.play().context(tr("не удалось начать запись", "could not start recording"))?;
        streams.push(stream);
        rates.push(rate);
        #[cfg(target_os = "macos")]
        let mut tap = None;
        let mut out: Option<cpal::Device> = None;
        if source == Source::Meeting {
            // Вход на устройстве вывода — это запись того, что оно играет.
            let system = tr("звук компьютера недоступен", "computer audio is unavailable");
            // На Mac — глобальный tap всех процессов: tap устройства вывода, который делает cpal,
            // не слышит Safari в звонке (см. systap.rs). Если не вышло — прежний путь.
            #[cfg(target_os = "macos")]
            match global_tap(&host) {
                Ok(found) => (tap, out) = (Some(found.0), Some(found.1)),
                Err(e) => eprintln!("глобальный tap звука компьютера: {e:#}"),
            }
            let (out, supported) = match out {
                Some(dev) => {
                    let supported = dev.default_input_config().context(system)?;
                    (dev, supported)
                }
                None => {
                    let dev = host.default_output_device().ok_or_else(|| anyhow!(system))?;
                    let supported = dev.default_output_config().context(system)?;
                    (dev, supported)
                }
            };
            let (stream, rate) = open_stream(&out, supported, 1, tx).with_context(|| {
                format!("{system}{}", if cfg!(target_os = "macos") { tr(" (нужна macOS 14.4 или новее)", " (macOS 14.4 or later is required)") } else { "" })
            })?;
            stream.play().context(system)?;
            streams.push(stream);
            rates.push(rate);
        }
        Ok(Self {
            _streams: streams,
            #[cfg(target_os = "macos")]
            _tap: tap,
            rx,
            rates,
            device: name,
        })
    }

    /// Пишется ли звук компьютера.
    pub fn system(&self) -> bool {
        self.rates.len() > 1
    }
}

/// Глобальный tap и его агрегатное устройство как вход cpal (частное устройство видно
/// только нашему процессу — среди его входов).
#[cfg(target_os = "macos")]
pub(crate) fn global_tap(host: &cpal::Host) -> Result<(crate::systap::GlobalTap, cpal::Device)> {
    let tap = crate::systap::GlobalTap::new()?;
    // Новое устройство появляется в списке не сразу.
    for _ in 0..40 {
        if let Some(dev) = host.input_devices()?.find(|d| d.description().ok().is_some_and(|x| x.name() == tap.name)) {
            return Ok((tap, dev));
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    Err(anyhow!("aggregate device not found"))
}

fn open_stream(dev: &cpal::Device, supported: cpal::SupportedStreamConfig, lane: usize, tx: Sender<(usize, Vec<f32>)>) -> Result<(cpal::Stream, u32)> {
    let format = supported.sample_format();
    let config: cpal::StreamConfig = supported.into();
    let channels = config.channels as usize;
    let stream = match format {
        cpal::SampleFormat::F32 => build::<f32>(dev, &config, channels, lane, tx)?,
        cpal::SampleFormat::I16 => build::<i16>(dev, &config, channels, lane, tx)?,
        cpal::SampleFormat::I32 => build::<i32>(dev, &config, channels, lane, tx)?,
        cpal::SampleFormat::U16 => build::<u16>(dev, &config, channels, lane, tx)?,
        other => anyhow::bail!("{}: {other:?}", tr("неподдерживаемый формат звука", "unsupported sample format")),
    };
    Ok((stream, config.sample_rate))
}

/// Поток захвата: каналы сводятся в моно, кусок уходит в канал как есть (частота устройства).
fn build<T>(dev: &cpal::Device, config: &cpal::StreamConfig, channels: usize, lane: usize, tx: Sender<(usize, Vec<f32>)>) -> Result<cpal::Stream>
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
            let _ = tx.send((lane, mono));
        },
        move |e| eprintln!("{}: {e}", if lane == 0 { "микрофон" } else { "звук компьютера" }),
        None,
    )?;
    Ok(stream)
}

// ---------- сведение дорожек ----------

/// Насколько одна дорожка может отстать от другой, отсчётов 16 кГц. Дальше отставшая
/// считается замолчавшей (устройство отключили, звук перестал идти) и дополняется тишиной,
/// чтобы запись не вставала.
const MAX_LAG: usize = SAMPLE_RATE as usize / 2;

/// Сводит дорожки (уже 16 кГц) в одну: складывает отсчёт к отсчёту по мере того, как
/// приходят все дорожки.
struct Mixer {
    lanes: Vec<VecDeque<f32>>,
}

impl Mixer {
    fn new(lanes: usize) -> Self {
        Self { lanes: (0..lanes).map(|_| VecDeque::new()).collect() }
    }

    fn push(&mut self, lane: usize, samples: &[f32]) {
        self.lanes[lane].extend(samples);
    }

    /// Сведённое, что уже можно отдать; `all` — отдать всё, дополнив короткие дорожки тишиной.
    fn take(&mut self, all: bool) -> Vec<f32> {
        let longest = self.lanes.iter().map(VecDeque::len).max().unwrap_or(0);
        let floor = if all { longest } else { longest.saturating_sub(MAX_LAG) };
        for lane in &mut self.lanes {
            if lane.len() < floor {
                lane.resize(floor, 0.0);
            }
        }
        let n = self.lanes.iter().map(VecDeque::len).min().unwrap_or(0);
        let mut out = vec![0.0f32; n];
        for lane in &mut self.lanes {
            for (o, x) in out.iter_mut().zip(lane.drain(..n)) {
                *o += x;
            }
        }
        out
    }
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
    /// Сколько секунд записано и пиковая громкость (0…1) за последнее время: микрофона
    /// и, если пишется, звука компьютера.
    fn tick(&self, seconds: f32, peak: f32, system: Option<f32>);
}

const TICK: Duration = Duration::from_millis(200);

/// Пишет звук из `capture` в `wav`, распознавая фразы по мере появления, пока не поднят `stop`.
/// Возвращает длительность записи в секундах.
pub fn run(capture: &Capture, engines: &Engines, wav: &mut Wav, stop: &AtomicBool, listen: &dyn Listen) -> Result<f32> {
    let resamplers = capture
        .rates
        .iter()
        .map(|&rate| {
            (rate != SAMPLE_RATE as u32)
                .then(|| sherpa_onnx::LinearResampler::create(rate as i32, SAMPLE_RATE))
                .map(|r| r.ok_or_else(|| anyhow!(tr("не удалось создать ресемплер", "could not create a resampler"))))
                .transpose()
        })
        .collect::<Result<Vec<_>>>()?;
    let mut mixer = Mixer::new(capture.rates.len());
    let vad = engines.vad()?;
    // VAD принимает окна по 512 отсчётов; хвост ждёт следующего куска.
    let mut pending: Vec<f32> = vec![];
    let (mut peaks, mut last_tick) = (vec![0.0f32; capture.rates.len()], Instant::now());
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
        if let Some((lane, chunk)) = chunk {
            let samples = match &resamplers[lane] {
                Some(r) => r.resample(&chunk, false),
                None => chunk,
            };
            peaks[lane] = samples.iter().fold(peaks[lane], |m, x| m.max(x.abs()));
            mixer.push(lane, &samples);
            let samples = mixer.take(false);
            wav.write(&samples)?;
            pending.extend_from_slice(&samples);
            let whole = pending.len() / 512 * 512;
            for w in pending[..whole].chunks(512) {
                vad.accept_waveform(w);
            }
            pending.drain(..whole);
            recognize(&vad);
        }
        if last_tick.elapsed() >= TICK {
            listen.tick(wav.seconds(), peaks[0].min(1.0), peaks.get(1).map(|p| p.min(1.0)));
            peaks.iter_mut().for_each(|p| *p = 0.0);
            last_tick = Instant::now();
        }
        if stop.load(Ordering::Relaxed) {
            break;
        }
    }
    // Хвост: что осталось в ресемплерах, в сведении и в поиске речи.
    for (lane, r) in resamplers.iter().enumerate() {
        if let Some(r) = r {
            mixer.push(lane, &r.resample(&[], true));
        }
    }
    let tail = mixer.take(true);
    wav.write(&tail)?;
    pending.extend_from_slice(&tail);
    vad.accept_waveform(&pending);
    vad.flush();
    recognize(&vad);
    Ok(wav.seconds())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mixer_sums_lanes_and_does_not_wait_for_a_silent_one() {
        let mut m = Mixer::new(2);
        m.push(0, &[0.1, 0.2, 0.3]);
        assert!(m.take(false).is_empty(), "вторая дорожка ещё не пришла");
        m.push(1, &[0.5, 0.5]);
        let out = m.take(false);
        assert_eq!(out.len(), 2);
        assert!((out[0] - 0.6).abs() < 1e-6 && (out[1] - 0.7).abs() < 1e-6);
        // Вторая дорожка замолчала: первая уходит с задержкой не больше MAX_LAG.
        m.push(0, &vec![0.0; MAX_LAG + 99]);
        assert_eq!(m.take(false).len(), 100);
        assert_eq!(m.take(true).len(), MAX_LAG);
        assert!(m.take(true).is_empty());
        // Одна дорожка — как есть.
        let mut one = Mixer::new(1);
        one.push(0, &[0.25; 7]);
        assert_eq!(one.take(false), vec![0.25; 7]);
    }

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
