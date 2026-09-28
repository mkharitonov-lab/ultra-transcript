//! Предобработка звука для распознавания: шумоподавление и выравнивание громкости голосов.
//! Обработанный звук идёт только в поиск речи и распознавание. Диаризация, голосовые
//! отпечатки и архив записи работают с исходным: после шумоподавления отпечатки смещаются
//! (на реальной записи диаризация дробила людей на лишних спикеров), а переменное усиление
//! искажает их ещё сильнее.

use crate::lang::tr;
use crate::media::SAMPLE_RATE;
use crate::store::Settings;
use anyhow::{bail, Context, Result};
use sherpa_onnx::{OfflineSpeechDenoiser, OfflineSpeechDenoiserConfig, OfflineSpeechDenoiserModelConfig};
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;

/// DPDFNet — облегчённая DeepFilterNet: убирает и ровный шум, и стук клавиатуры.
pub const DENOISER_FILE: &str = "dpdfnet_baseline.onnx";

const SR: usize = SAMPLE_RATE as usize;

/// Звук для поиска речи и распознавания — без шума и с выровненной громкостью, что включено
/// в настройках; `None` — обработка выключена. Сбой шумоподавления не останавливает
/// расшифровку: тогда только выравнивание, а в `warnings` — причина.
/// `progress` получает долю выполненного шумоподавления.
pub fn for_recognition(
    samples: &[f32],
    settings: &Settings,
    models: &Path,
    progress: &dyn Fn(f32),
    warnings: &mut Vec<String>,
) -> Option<Vec<f32>> {
    if !settings.denoise && !settings.level_volume {
        return None;
    }
    let mut x = if settings.denoise {
        progress(0.0);
        denoise(&models.join(DENOISER_FILE), samples, progress).unwrap_or_else(|e| {
            warnings.push(format!("{}: {e:#}", tr("Шумоподавление пропущено", "Noise reduction was skipped")));
            samples.to_vec()
        })
    } else {
        samples.to_vec()
    };
    if settings.level_volume {
        level(&mut x);
    }
    Some(x)
}

// ---------- шумоподавление ----------

/// Шумоподавление нейросетью. Куски по 30 с обрабатываются параллельно; каждый начинается
/// на секунду раньше: модели нужно «разогреться», и в эту секунду он плавно сменяет предыдущий.
fn denoise(model: &Path, samples: &[f32], progress: &dyn Fn(f32)) -> Result<Vec<f32>> {
    const CHUNK: usize = 30 * SR;
    const WARMUP: usize = SR;
    if !model.exists() {
        bail!(tr("модель не скачана — откройте «Настройки → Звук»", "the model is not downloaded — open Settings → Audio"));
    }
    let starts: Vec<usize> = (0..samples.len()).step_by(CHUNK).collect();
    let mut out = vec![0.0f32; samples.len()];
    // «Разогревы» кусков: сводятся с концом предыдущего куска, когда готовы все.
    let mut warmups = vec![vec![]; starts.len()];
    let next = AtomicUsize::new(0);
    let (tx, rx) = mpsc::channel();
    std::thread::scope(|s| -> Result<()> {
        for _ in 0..threads().min(starts.len()) {
            let (tx, next, starts) = (tx.clone(), &next, &starts);
            s.spawn(move || {
                // Один поток — одна модель: так sherpa-onnx безопасен.
                let d = match create(model) {
                    Ok(d) => d,
                    Err(e) => {
                        let _ = tx.send(Err(e));
                        return;
                    }
                };
                while let Some(&start) = starts.get(next.fetch_add(1, Ordering::Relaxed)) {
                    let a = start.saturating_sub(WARMUP);
                    let b = (start + CHUNK).min(samples.len());
                    let mut y = d.run(&samples[a..b], SAMPLE_RATE);
                    // Модель обрабатывает звук целыми кадрами и отбрасывает хвост короче кадра —
                    // эти доли секунды берём из исходной записи.
                    let r = if y.sample_rate != SAMPLE_RATE || y.samples.len().abs_diff(b - a) > SR / 10 {
                        Err(anyhow::anyhow!("модель вернула {} отсчётов ({} Гц) вместо {}", y.samples.len(), y.sample_rate, b - a))
                    } else {
                        let got = y.samples.len().min(b - a);
                        y.samples.truncate(got);
                        y.samples.extend_from_slice(&samples[a + got..b]);
                        Ok((start, y.samples))
                    };
                    if tx.send(r).is_err() {
                        break;
                    }
                }
            });
        }
        drop(tx);
        for (done, r) in rx.into_iter().enumerate() {
            let (start, y) = r?;
            let warm = start - start.saturating_sub(WARMUP);
            out[start..start + y.len() - warm].copy_from_slice(&y[warm..]);
            warmups[start / CHUNK] = y[..warm].to_vec();
            progress((done + 1) as f32 / starts.len() as f32);
        }
        Ok(())
    })?;
    for (start, warm) in starts.iter().zip(&warmups) {
        let a = start - warm.len();
        for (i, v) in warm.iter().enumerate() {
            let t = i as f32 / warm.len() as f32;
            out[a + i] = out[a + i] * (1.0 - t) + v * t;
        }
    }
    Ok(out)
}

fn create(model: &Path) -> Result<OfflineSpeechDenoiser> {
    let mut config = OfflineSpeechDenoiserModelConfig::default(); // один поток ONNX на модель
    config.dpdfnet.model = Some(model.to_string_lossy().into_owned());
    OfflineSpeechDenoiser::create(&OfflineSpeechDenoiserConfig { model: config })
        .context(tr("не удалось загрузить модель шумоподавления", "could not load the noise reduction model"))
}

fn threads() -> usize {
    std::thread::available_parallelism().map(|n| n.get().saturating_sub(2).clamp(2, 8)).unwrap_or(4)
}

// ---------- выравнивание громкости ----------

/// Уровень, к которому приводится речь (RMS, дБ от полной шкалы).
const TARGET_DB: f32 = -23.0;
/// Тихие и далёкие голоса поднимаются не больше чем на 30 дБ, громкие опускаются до 10 дБ.
const MAX_BOOST_DB: f32 = 30.0;
const MAX_CUT_DB: f32 = -10.0;
/// Кадр анализа — 10 мс.
const HOP: usize = SR / 100;

/// Приводит речь к одному уровню: громкость фразы оценивается по речи вокруг неё
/// (±1,5 с), так что внутри фразы естественная динамика сохраняется, а тихий и громкий
/// спикеры звучат одинаково. В паузах держится усиление соседней речи — шум в них
/// не «вытягивается» сильнее, чем под речью. Пики ограничиваются без перегрузки.
fn level(x: &mut [f32]) {
    if x.is_empty() {
        return;
    }
    let energy: Vec<f64> =
        x.chunks(HOP).map(|c| c.iter().map(|v| (*v as f64).powi(2)).sum::<f64>() / c.len() as f64).collect();
    let speech = speech_frames(&energy);
    // Средняя энергия речи в окне ±1,5 с (гаусс, σ = 0,75 с) — только по речевым кадрам.
    let weighted: Vec<f64> = energy.iter().zip(&speech).map(|(e, s)| e * s).collect();
    let num = gauss(&weighted, 75.0);
    let den = gauss(&speech, 75.0);
    let gain_db: Vec<Option<f64>> = num
        .iter()
        .zip(&den)
        // Одиночный щелчок речью не считается: речи в окне должно быть хотя бы ~40 мс.
        .map(|(n, d)| (*d > 0.02).then(|| (TARGET_DB as f64 - db(n / d)).clamp(MAX_CUT_DB as f64, MAX_BOOST_DB as f64)))
        .collect();
    let gain_db = gauss(&fill_gaps(&gain_db), 25.0);
    let gain: Vec<f32> = gain_db.iter().map(|g| 10f32.powf(*g as f32 / 20.0)).collect();
    // Усиление — линейно между центрами кадров, без ступенек.
    let last = gain.len() - 1;
    for (n, v) in x.iter_mut().enumerate() {
        let pos = (n as f32 / HOP as f32 - 0.5).max(0.0);
        let (k, t) = (pos as usize, pos.fract());
        let (a, b) = (gain[k.min(last)], gain[(k + 1).min(last)]);
        *v *= a + (b - a) * t;
    }
    limit(x, 0.89); // −1 дБ
}

fn db(e: f64) -> f64 {
    10.0 * (e + 1e-12).log10()
}

/// Речевые кадры (1.0) — на 12 дБ громче местного шума и не тише −65 дБ.
/// Шум — самая тихая секунда в окне ±10 с (секунда — по 10-му перцентилю её кадров):
/// даже в сплошной речи за 20 с найдутся паузы между словами.
fn speech_frames(energy: &[f64]) -> Vec<f64> {
    let levels: Vec<f64> = energy.iter().map(|e| db(*e)).collect();
    let per_second: Vec<f64> = levels.chunks(100).map(|c| percentile(c, 0.1)).collect();
    let floor: Vec<f64> = (0..per_second.len())
        .map(|i| per_second[i.saturating_sub(10)..(i + 11).min(per_second.len())].iter().copied().fold(f64::MAX, f64::min))
        .collect();
    levels
        .iter()
        .enumerate()
        .map(|(k, l)| (*l > (floor[k / 100].max(-80.0) + 12.0).max(-65.0)) as u8 as f64)
        .collect()
}

fn percentile(v: &[f64], p: f64) -> f64 {
    let mut s = v.to_vec();
    s.sort_by(|a, b| a.total_cmp(b));
    s[((s.len() - 1) as f64 * p) as usize]
}

/// Пропуски — линейно между соседними известными значениями, по краям — ближайшим.
fn fill_gaps(v: &[Option<f64>]) -> Vec<f64> {
    let known: Vec<(usize, f64)> = v.iter().enumerate().filter_map(|(i, g)| g.map(|g| (i, g))).collect();
    let Some(&(first, g0)) = known.first() else { return vec![0.0; v.len()] };
    let mut out = vec![g0; first];
    for w in known.windows(2) {
        let ((a, ga), (b, gb)) = (w[0], w[1]);
        out.extend((a..b).map(|i| ga + (gb - ga) * (i - a) as f64 / (b - a) as f64));
    }
    let &(last, gl) = known.last().unwrap();
    out.extend(std::iter::repeat_n(gl, v.len() - last));
    out
}

/// Гауссово сглаживание (σ в кадрах): три прохода скользящего среднего.
fn gauss(v: &[f64], sigma: f64) -> Vec<f64> {
    let r = ((12.0 * sigma * sigma / 3.0 + 1.0).sqrt() / 2.0) as usize;
    (0..3).fold(v.to_vec(), |acc, _| box_mean(&acc, r))
}

/// Среднее в окне ±r; у краёв — по тем значениям, что есть.
fn box_mean(v: &[f64], r: usize) -> Vec<f64> {
    let mut sum = vec![0.0; v.len() + 1];
    for (i, x) in v.iter().enumerate() {
        sum[i + 1] = sum[i] + x;
    }
    (0..v.len())
        .map(|i| {
            let (a, b) = (i.saturating_sub(r), (i + r + 1).min(v.len()));
            (sum[b] - sum[a]) / (b - a) as f64
        })
        .collect()
}

/// Ограничитель пиков: по блокам 5 мс считается нужное ослабление, берётся минимум
/// с соседями и плавно ведётся между центрами блоков. Любой отсчёт блока покрыт
/// минимумом своего блока, так что перегрузки нет, а ослабление нарастает за 5 мс — без щелчков.
fn limit(x: &mut [f32], ceiling: f32) {
    const BLOCK: usize = SR / 200;
    let need: Vec<f32> = x
        .chunks(BLOCK)
        .map(|c| ceiling / c.iter().fold(ceiling, |m, v| m.max(v.abs())))
        .collect();
    if need.iter().all(|g| *g >= 1.0) {
        return;
    }
    let last = need.len() - 1;
    let min3: Vec<f32> = (0..need.len())
        .map(|i| need[i.saturating_sub(1)..=(i + 1).min(last)].iter().copied().fold(1.0, f32::min))
        .collect();
    for (n, v) in x.iter_mut().enumerate() {
        let pos = (n as f32 / BLOCK as f32 - 0.5).max(0.0);
        let (k, t) = (pos as usize, pos.fract());
        let (a, b) = (min3[k.min(last)], min3[(k + 1).min(last)]);
        *v = (*v * (a + (b - a) * t)).clamp(-1.0, 1.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(secs: f32, amp: f32) -> Vec<f32> {
        (0..(secs * SR as f32) as usize)
            .map(|n| amp * (n as f32 * 2.0 * std::f32::consts::PI * 220.0 / SR as f32).sin())
            .collect()
    }

    fn rms_db(x: &[f32]) -> f32 {
        10.0 * (x.iter().map(|v| v * v).sum::<f32>() / x.len() as f32).log10()
    }

    #[test]
    fn quiet_and_loud_voices_meet_at_target() {
        // Громкая «фраза», пауза с тихим шумом, тихая «фраза» (на 30 дБ тише).
        let mut x = tone(4.0, 0.2);
        x.extend((0..2 * SR).map(|n| 1e-4 * ((n * 7919 % 1000) as f32 / 500.0 - 1.0)));
        x.extend(tone(4.0, 0.0063));
        level(&mut x);
        let loud = rms_db(&x[SR..3 * SR]);
        let quiet = rms_db(&x[7 * SR..9 * SR]);
        assert!((loud - TARGET_DB).abs() < 1.5, "громкая: {loud}");
        assert!((quiet - TARGET_DB).abs() < 1.5, "тихая: {quiet}");
        assert!(x.iter().all(|v| v.abs() <= 0.89 + 1e-6));
    }

    #[test]
    fn silence_and_tiny_inputs_are_safe() {
        let mut z = vec![0.0f32; 3 * SR];
        level(&mut z);
        assert!(z.iter().all(|v| *v == 0.0));
        let mut one = vec![0.3f32];
        level(&mut one);
        assert!(one[0].is_finite());
        level(&mut []);
    }

    #[test]
    fn limiter_never_overshoots() {
        let mut x: Vec<f32> = (0..SR).map(|n| if n % 997 == 0 { 3.0 } else { 0.5 * (n as f32 * 0.01).sin() }).collect();
        limit(&mut x, 0.89);
        assert!(x.iter().all(|v| v.abs() <= 0.89 + 1e-6));
    }
}
