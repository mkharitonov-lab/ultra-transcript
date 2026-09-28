//! pyannote community-1 (pyannote.audio 4.0): сегментация pyannote 3.0, голосовые отпечатки
//! WeSpeaker ResNet34 по маске спикера внутри 10-секундного окна и кластеризация VBx.
//! Повторяет конвейер SpeakerDiarization + VBxClustering на ONNX Runtime, вшитом в sherpa-onnx.
//! Отличия от pyannote: шаг окна 2 с вместо 1 с — вдвое быстрее, разметка почти та же (при 2,5 с
//! на коротких записях уже теряются спикеры) — и сразу «исключительная» разметка: не больше
//! одного спикера в каждый момент, так слова однозначно раскладываются по спикерам.

use super::vbx::{self, Plda, Voice};
use crate::speech::{threads, Turn, SEGMENTATION_DIR};
use anyhow::{anyhow, ensure, Result};
use ort::session::Session;
use ort::value::{Tensor, TensorRef};
use std::path::Path;
use std::sync::OnceLock;

/// Папка модели в каталоге моделей: отпечатки по маске (ONNX) и PLDA. Сегментация — общая с pyannote 3.0.
pub const DIR: &str = "pyannote-community-1";
const FBANK: &str = "wespeaker-fbank-b32.onnx";
const RESNET: &str = "wespeaker-resnet-frames-b32.onnx";
const POOL: &str = "wespeaker-pool-classify-b3.onnx";

const SR: usize = 16_000;
/// Окно сегментации — 10 с, шаг — 2 с.
const WINDOW: usize = 10 * SR;
const STEP: usize = 2 * SR;
/// Кадров сегментации в окне и их шаг/длительность (рецептивное поле SincNet), с.
const FRAMES: usize = 589;
const FRAME_STEP: f64 = 270.0 / 16000.0;
const FRAME_DUR: f64 = 991.0 / 16000.0;
/// Сколько спикеров сегментация различает внутри окна.
const LOCAL: usize = 3;
/// fbank и ResNet экспортированы под пачку ровно из 32 окон.
const BATCH: usize = 32;
/// Классы powerset: никто, по одному и пары спикеров.
const CLASSES: usize = 7;
const POWERSET: [[u8; LOCAL]; CLASSES] = [[0, 0, 0], [1, 0, 0], [0, 1, 0], [0, 0, 1], [1, 1, 0], [1, 0, 1], [0, 1, 1]];
/// Меньше стольких кадров чистой речи WeSpeaker не справится — берётся маска с наложениями.
const MIN_CLEAN_FRAMES: usize = 2;

/// Кто из трёх спикеров окна говорит в каждом его кадре.
type Window = [[u8; LOCAL]; FRAMES];

/// Отпечаток спикера окна.
struct Print {
    chunk: usize,
    speaker: usize,
    /// Годится ли для построения кластеров.
    train: bool,
    embedding: Vec<f32>,
}

fn ort_err<R>(e: ort::Error<R>) -> anyhow::Error {
    anyhow!("ONNX Runtime: {e}")
}

/// `ort` собран без своей копии ONNX Runtime и берёт ту, что статически вшита в sherpa-onnx.
fn init_ort() -> Result<()> {
    static OK: OnceLock<bool> = OnceLock::new();
    let ok = *OK.get_or_init(|| {
        // SAFETY: OrtGetApiBase — точка входа слинкованного ONNX Runtime; таблица API живёт до конца процесса.
        let api = unsafe { ((*ort::sys::OrtGetApiBase()).GetApi)(ort::sys::ORT_API_VERSION) };
        !api.is_null() && ort::set_api(unsafe { api.read() })
    });
    ensure!(ok, "ONNX Runtime из sherpa-onnx не поддерживает API {}", ort::sys::ORT_API_VERSION);
    Ok(())
}

fn session(path: &Path) -> Result<Session> {
    ensure!(path.exists(), "нет файла модели {}", path.display());
    Session::builder()
        .map_err(ort_err)?
        .with_intra_threads(threads() as usize)
        .map_err(ort_err)?
        .commit_from_file(path)
        .map_err(ort_err)
}

pub struct Community1 {
    segmentation: Session,
    fbank: Session,
    resnet: Session,
    pool: Session,
    plda: Plda,
}

/// Начала окон, как в pyannote: целые окна с шагом и последнее неполное, дополненное нулями.
fn window_starts(n: usize) -> Vec<usize> {
    let complete = if n >= WINDOW { (n - WINDOW) / STEP + 1 } else { 0 };
    let mut starts: Vec<usize> = (0..complete).map(|c| c * STEP).collect();
    if n < WINDOW || !(n - WINDOW).is_multiple_of(STEP) {
        starts.push(complete * STEP);
    }
    starts
}

fn push_window(buf: &mut Vec<f32>, samples: &[f32], start: usize) {
    let part = &samples[start.min(samples.len())..(start + WINDOW).min(samples.len())];
    buf.extend_from_slice(part);
    buf.resize(buf.len() + WINDOW - part.len(), 0.0);
}

/// Середина кадра сегментации, с.
fn frame_middle(t: usize) -> f32 {
    (t as f64 * FRAME_STEP + FRAME_DUR / 2.0) as f32
}

impl Community1 {
    pub fn load(models: &Path) -> Result<Self> {
        init_ort()?;
        let dir = models.join(DIR);
        Ok(Self {
            segmentation: session(&models.join(SEGMENTATION_DIR).join("model.onnx"))?,
            fbank: session(&dir.join(FBANK))?,
            resnet: session(&dir.join(RESNET))?,
            pool: session(&dir.join(POOL))?,
            plda: Plda::load(&dir)?,
        })
    }

    pub fn diarize(&mut self, samples: &[f32], progress: &mut dyn FnMut(f32)) -> Result<Vec<Turn>> {
        let starts = window_starts(samples.len());
        let seg = self.segment(samples, &starts, &mut |f| progress(0.1 * f))?;
        let prints = self.embed(samples, &starts, &seg, &mut |f| progress(0.1 + 0.85 * f))?;
        let voices: Vec<Voice> = prints
            .iter()
            .map(|p| Voice { chunk: p.chunk, speaker: p.speaker, embedding: &p.embedding, train: p.train })
            .collect();
        let clusters = vbx::cluster(&self.plda, &voices);
        let mut hard = vec![[None; LOCAL]; starts.len()];
        for (v, k) in voices.iter().zip(clusters) {
            hard[v.chunk][v.speaker] = k;
        }
        let turns = reconstruct(&seg, &hard);
        progress(1.0);
        Ok(turns)
    }

    fn segment(&mut self, samples: &[f32], starts: &[usize], progress: &mut dyn FnMut(f32)) -> Result<Vec<Window>> {
        let mut seg = Vec::with_capacity(starts.len());
        for batch in starts.chunks(BATCH) {
            let mut input = Vec::with_capacity(batch.len() * WINDOW);
            batch.iter().for_each(|&s| push_window(&mut input, samples, s));
            let tensor = Tensor::from_array(([batch.len(), 1, WINDOW], input)).map_err(ort_err)?;
            let out = self.segmentation.run(ort::inputs!["x" => tensor]).map_err(ort_err)?;
            let (shape, y) = out["y"].try_extract_tensor::<f32>().map_err(ort_err)?;
            ensure!(shape[1] as usize == FRAMES && shape[2] as usize == CLASSES, "сегментация: неожиданная форма {shape:?}");
            for window in y.as_chunks::<{ FRAMES * CLASSES }>().0 {
                let mut frames = [[0u8; LOCAL]; FRAMES];
                for (f, scores) in frames.iter_mut().zip(window.as_chunks::<CLASSES>().0) {
                    let best = (0..scores.len()).fold(0, |b, i| if scores[i] > scores[b] { i } else { b });
                    *f = POWERSET[best];
                }
                seg.push(frames);
            }
            progress(seg.len() as f32 / starts.len() as f32);
        }
        Ok(seg)
    }

    /// Отпечатки спикеров, активных в окнах: ResNet считается раз на окно, маски спикеров — при усреднении.
    fn embed(&mut self, samples: &[f32], starts: &[usize], seg: &[Window], progress: &mut dyn FnMut(f32)) -> Result<Vec<Print>> {
        let active: Vec<usize> = (0..seg.len()).filter(|&c| seg[c].iter().any(|f| f.iter().any(|&x| x > 0))).collect();
        let mut prints = vec![];
        for (done, batch) in active.chunks(BATCH).enumerate() {
            let mut input = Vec::with_capacity(BATCH * WINDOW);
            batch.iter().for_each(|&c| push_window(&mut input, samples, starts[c]));
            input.resize(BATCH * WINDOW, 0.0);
            let waveform = Tensor::from_array(([BATCH, 1, WINDOW], input)).map_err(ort_err)?;
            let fbank_out = self.fbank.run(ort::inputs!["waveform" => waveform]).map_err(ort_err)?;
            let (shape, fbank) = fbank_out["fbank"].try_extract_tensor::<f32>().map_err(ort_err)?;
            let fbank = TensorRef::from_array_view((shape.clone(), fbank)).map_err(ort_err)?;
            let out = self.resnet.run(ort::inputs!["fbank" => fbank]).map_err(ort_err)?;
            let (shape, frames) = out["frames"].try_extract_tensor::<f32>().map_err(ort_err)?;
            let (dims, per_window) = (shape.clone(), frames.len() / BATCH);
            for (i, &c) in batch.iter().enumerate() {
                let masks = masks(&seg[c]);
                let own = &frames[i * per_window..(i + 1) * per_window];
                let mut input = Vec::with_capacity(LOCAL * per_window);
                (0..LOCAL).for_each(|_| input.extend_from_slice(own));
                let frames = Tensor::from_array(([LOCAL, dims[1] as usize, dims[2] as usize], input)).map_err(ort_err)?;
                let weights = Tensor::from_array(([LOCAL, FRAMES], masks.iter().flat_map(|m| m.used).collect::<Vec<f32>>()))
                    .map_err(ort_err)?;
                let out = self.pool.run(ort::inputs!["frames" => frames, "weights" => weights]).map_err(ort_err)?;
                let (_, e) = out["output"].try_extract_tensor::<f32>().map_err(ort_err)?;
                let dim = e.len() / LOCAL;
                for (s, m) in masks.iter().enumerate().filter(|(_, m)| m.active) {
                    prints.push(Print { chunk: c, speaker: s, train: m.train, embedding: e[s * dim..(s + 1) * dim].to_vec() });
                }
            }
            progress(((done + 1) * BATCH).min(active.len()) as f32 / active.len() as f32);
        }
        Ok(prints)
    }
}

struct Mask {
    active: bool,
    /// Хватает ли речи без наложений (не меньше 20% окна), чтобы строить по ней кластеры.
    train: bool,
    /// Кадры, по которым усредняется отпечаток: без наложений, если их достаточно.
    used: [f32; FRAMES],
}

fn masks(seg: &Window) -> [Mask; LOCAL] {
    std::array::from_fn(|s| {
        let mut mask = [0.0; FRAMES];
        let mut clean = [0.0; FRAMES];
        let (mut n, mut n_clean) = (0, 0);
        for (f, frame) in seg.iter().enumerate() {
            if frame[s] > 0 {
                mask[f] = 1.0;
                n += 1;
                if frame.iter().map(|&x| x as usize).sum::<usize>() < 2 {
                    clean[f] = 1.0;
                    n_clean += 1;
                }
            }
        }
        Mask {
            active: n > 0,
            train: n_clean * 5 >= FRAMES,
            used: if n_clean > MIN_CLEAN_FRAMES { clean } else { mask },
        }
    })
}

/// Разметка по кластерам, как `reconstruct` + `to_diarization` в pyannote с числом спикеров,
/// ограниченным одним: в каждом кадре — кластер с наибольшей суммарной активностью по окнам,
/// если по окнам в среднем там кто-то говорит.
fn reconstruct(seg: &[Window], hard: &[[Option<usize>; LOCAL]]) -> Vec<Turn> {
    let clusters = hard.iter().flatten().flatten().max().map_or(0, |k| k + 1);
    if seg.is_empty() || clusters == 0 {
        return vec![];
    }
    let step = STEP as f64 / SR as f64;
    let first = |c: usize| (c as f64 * step / FRAME_STEP).round_ties_even() as usize;
    let total = ((WINDOW as f64 / SR as f64 + (seg.len() - 1) as f64 * step) / FRAME_STEP).round_ties_even() as usize + 1;
    let mut speaking = vec![0.0f32; total];
    let mut windows = vec![0.0f32; total];
    let mut activity = vec![0.0f32; total * clusters];
    for (c, (frames, hard)) in seg.iter().zip(hard).enumerate() {
        let t0 = first(c);
        for (f, frame) in frames.iter().enumerate().take(total.saturating_sub(t0)) {
            let t = t0 + f;
            speaking[t] += frame.iter().map(|&x| x as f32).sum::<f32>();
            windows[t] += 1.0;
            let mut best = vec![0u8; clusters];
            for (s, k) in hard.iter().enumerate() {
                if let Some(k) = *k {
                    best[k] = best[k].max(frame[s]);
                }
            }
            best.iter().enumerate().for_each(|(k, &x)| activity[t * clusters + k] += x as f32);
        }
    }
    // Кто говорит в кадре (или никто).
    let who: Vec<Option<usize>> = (0..total)
        .map(|t| {
            // np.rint: половина округляется к чётному, 0.5 → 0.
            let count = if windows[t] > 0.0 { (speaking[t] / windows[t]).round_ties_even() } else { 0.0 };
            let row = &activity[t * clusters..(t + 1) * clusters];
            let k = (0..clusters).fold(0, |b, k| if row[k] > row[b] { k } else { b });
            (count >= 1.0 && row[k] > 0.0).then_some(k)
        })
        .collect();
    let mut turns = vec![];
    let mut t = 0;
    while t < total {
        let Some(k) = who[t] else {
            t += 1;
            continue;
        };
        let end = (t..total).find(|&e| who[e] != Some(k)).unwrap_or(total);
        // Как Binarize в pyannote: от середины первого кадра до середины первого кадра после (или последнего).
        turns.push(Turn { start: frame_middle(t), end: frame_middle(end.min(total - 1)), speaker: k as i32 });
        t = end;
    }
    turns
}
