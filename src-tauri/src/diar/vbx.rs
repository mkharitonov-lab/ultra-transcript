//! Кластеризация VBx в варианте pyannote.audio 4 (VBxClustering): начальное разбиение —
//! иерархическая кластеризация по центроидам, затем байесовская смесь в пространстве PLDA
//! сама решает, сколько спикеров оставить (Landini и др., «Bayesian HMM clustering of
//! x-vector sequences (VBx) in speaker diarization», 2022).

use anyhow::{anyhow, bail, Context, Result};
use std::io::Read;
use std::path::Path;

/// Гиперпараметры pyannote community-1 (config.yaml).
const AHC_THRESHOLD: f64 = 0.6;
const FA: f64 = 0.07;
const FB: f64 = 0.8;
const MAX_ITERS: usize = 20;
const EPSILON: f64 = 1e-4;
const INIT_SMOOTHING: f64 = 7.0;

/// Отпечаток спикера в окне: номер окна, номер спикера в окне (0..3), вектор WeSpeaker.
pub struct Voice<'a> {
    pub chunk: usize,
    pub speaker: usize,
    pub embedding: &'a [f32],
    /// Хватает ли чистой речи (без наложений), чтобы участвовать в построении кластеров.
    pub train: bool,
}

/// Преобразование отпечатков в пространство PLDA (BUT Speech@FIT): центрирование, LDA до 128
/// измерений, затем базис, в котором внутриклассовый разброс единичный, а межклассовый — `psi`.
pub struct Plda {
    mean1: Vec<f64>,
    mean2: Vec<f64>,
    /// 256 × 128 по строкам.
    lda: Vec<f64>,
    mu: Vec<f64>,
    /// 128 × 128 по строкам.
    tr: Vec<f64>,
    psi: Vec<f64>,
}

impl Plda {
    /// `xvec_transform.npz` и `plda.npz` из pyannote community-1. pyannote дополнительно решает
    /// обобщённую задачу на собственные значения, но её ответ — те же строки `tr`, упорядоченные
    /// по убыванию `psi` (с точностью до знака); VBx не зависит ни от порядка измерений, ни от знаков.
    pub fn load(dir: &Path) -> Result<Self> {
        let mut x = Npz::open(&dir.join("xvec_transform.npz"))?;
        let mut p = Npz::open(&dir.join("plda.npz"))?;
        let plda = Self {
            mean1: x.array("mean1")?,
            mean2: x.array("mean2")?,
            lda: x.array("lda")?,
            mu: p.array("mu")?,
            tr: p.array("tr")?,
            psi: p.array("psi")?,
        };
        let (d0, d1) = (plda.mean1.len(), plda.mean2.len());
        if plda.lda.len() != d0 * d1 || plda.tr.len() != d1 * d1 || plda.mu.len() != d1 || plda.psi.len() != d1 {
            bail!("PLDA: неожиданные размеры матриц");
        }
        Ok(plda)
    }

    /// `xvec_tf`, затем `plda_tf` из pyannote.audio.utils.vbx.
    fn project(&self, e: &[f32]) -> Vec<f64> {
        let (d0, d1) = (self.mean1.len(), self.mean2.len());
        let mut x: Vec<f64> = e.iter().zip(&self.mean1).map(|(&v, m)| v as f64 - m).collect();
        scale_to(&mut x, (d0 as f64).sqrt());
        let mut y: Vec<f64> = (0..d1)
            .map(|j| (0..d0).map(|i| self.lda[i * d1 + j] * x[i]).sum::<f64>() - self.mean2[j])
            .collect();
        scale_to(&mut y, (d1 as f64).sqrt());
        (0..d1)
            .map(|i| (0..d1).map(|j| (y[j] - self.mu[j]) * self.tr[i * d1 + j]).sum())
            .collect()
    }
}

/// Приводит вектор к длине `len`.
fn scale_to(v: &mut [f64], len: f64) {
    let n = v.iter().map(|x| x * x).sum::<f64>().sqrt();
    if n > 0.0 {
        v.iter_mut().for_each(|x| *x *= len / n);
    }
}

/// Кластеры для отпечатков спикеров окон (тот же порядок, что у `voices`). Спикеры одного окна
/// всегда попадают в разные кластеры; `None` — лишний спикер окна, когда кластеров меньше, чем их.
pub fn cluster(plda: &Plda, voices: &[Voice]) -> Vec<Option<usize>> {
    let train: Vec<&Voice> = voices.iter().filter(|v| v.train).collect();
    if train.len() < 2 {
        return vec![Some(0); voices.len()];
    }
    let normed: Vec<Vec<f64>> = train
        .iter()
        .map(|v| {
            let mut x: Vec<f64> = v.embedding.iter().map(|&x| x as f64).collect();
            scale_to(&mut x, 1.0);
            x
        })
        .collect();
    let init = ahc(&normed, AHC_THRESHOLD);
    let k = init.iter().max().map_or(1, |m| m + 1);
    // Сглаженное начальное приближение: softmax(one-hot × 7).
    let (hit, miss) = (INIT_SMOOTHING.exp(), 1.0);
    let norm = hit + miss * (k - 1) as f64;
    let gamma0 = init
        .iter()
        .map(|&c| (0..k).map(|j| (if j == c { hit } else { miss }) / norm).collect())
        .collect();
    let fea: Vec<Vec<f64>> = train.iter().map(|v| plda.project(v.embedding)).collect();
    let (gamma, pi) = vbx(&fea, &plda.psi, gamma0);

    // Центроиды оставшихся спикеров — средние исходных отпечатков с весами-ответственностями.
    let dim = train[0].embedding.len();
    let centroids: Vec<Vec<f64>> = (0..k)
        .filter(|&j| pi[j] > 1e-7)
        .map(|j| {
            let mut c = vec![0.0; dim];
            let mut w = 0.0;
            for (g, v) in gamma.iter().zip(&train) {
                c.iter_mut().zip(v.embedding).for_each(|(c, &x)| *c += g[j] * x as f64);
                w += g[j];
            }
            c.iter_mut().for_each(|c| *c /= w);
            c
        })
        .collect();

    // Каждое окно отдельно: его спикеры — в разные кластеры с наибольшим суммарным сходством.
    let mut out = vec![None; voices.len()];
    let mut i = 0;
    while i < voices.len() {
        let j = (i..voices.len()).find(|&j| voices[j].chunk != voices[i].chunk).unwrap_or(voices.len());
        let scores: Vec<Vec<f64>> = voices[i..j]
            .iter()
            .map(|v| centroids.iter().map(|c| 1.0 + cosine(v.embedding, c)).collect())
            .collect();
        out[i..j].copy_from_slice(&assign(&scores));
        i = j;
    }
    out
}

fn cosine(a: &[f32], b: &[f64]) -> f64 {
    let (mut ab, mut aa, mut bb) = (0.0, 0.0, 0.0);
    for (&x, &y) in a.iter().zip(b) {
        let x = x as f64;
        ab += x * y;
        aa += x * x;
        bb += y * y;
    }
    ab / (aa.sqrt() * bb.sqrt()).max(f64::MIN_POSITIVE)
}

/// Иерархическая кластеризация по центроидам с отсечкой по высоте — как в scipy
/// `fcluster(linkage(x, "centroid"), t, criterion="distance")`. Метки — 0..k.
fn ahc(x: &[Vec<f64>], threshold: f64) -> Vec<usize> {
    let n = x.len();
    let mut d = Vec::with_capacity(n * (n - 1) / 2);
    for i in 0..n {
        for j in i + 1..n {
            d.push(x[i].iter().zip(&x[j]).map(|(a, b)| (a - b) * (a - b)).sum::<f64>().sqrt());
        }
    }
    let steps = kodama::linkage(&mut d, n, kodama::Method::Centroid).steps().to_vec();
    // У центроидного метода высоты слияний бывают немонотонны: fcluster сравнивает с порогом
    // наибольшую высоту в поддереве.
    let mut top = vec![0.0f64; n + steps.len()];
    for (i, s) in steps.iter().enumerate() {
        top[n + i] = s.dissimilarity.max(top[s.cluster1]).max(top[s.cluster2]);
    }
    let children = |node: usize| (steps[node - n].cluster1, steps[node - n].cluster2);
    let mut labels = vec![0; n];
    let mut next = 0;
    let mut stack = vec![n + steps.len() - 1];
    while let Some(node) = stack.pop() {
        if node >= n && top[node] > threshold {
            let (a, b) = children(node);
            stack.extend([b, a]);
            continue;
        }
        // Всё поддерево — один кластер.
        let mut leaves = vec![node];
        while let Some(m) = leaves.pop() {
            if m < n {
                labels[m] = next;
            } else {
                let (a, b) = children(m);
                leaves.extend([a, b]);
            }
        }
        next += 1;
    }
    labels
}

/// VBx без HMM (pyannote.audio.utils.vbx.VBx). `x` — отпечатки в пространстве PLDA, `phi` —
/// межклассовая дисперсия по измерениям, `gamma` — начальные ответственности (n × k).
/// Возвращает уточнённые ответственности и веса спикеров; у лишних спикеров вес стремится к нулю.
fn vbx(x: &[Vec<f64>], phi: &[f64], mut gamma: Vec<Vec<f64>>) -> (Vec<Vec<f64>>, Vec<f64>) {
    let (d, k) = (phi.len(), gamma[0].len());
    let mut pi = vec![1.0 / k as f64; k];
    let g: Vec<f64> = x
        .iter()
        .map(|r| -0.5 * (r.iter().map(|v| v * v).sum::<f64>() + d as f64 * (2.0 * std::f64::consts::PI).ln()))
        .collect();
    let v: Vec<f64> = phi.iter().map(|p| p.sqrt()).collect();
    let rho: Vec<Vec<f64>> = x.iter().map(|r| r.iter().zip(&v).map(|(a, b)| a * b).collect()).collect();
    let mut elbo_prev = f64::NEG_INFINITY;
    for iter in 0..MAX_ITERS {
        let mut inv_l = vec![vec![0.0; d]; k];
        let mut alpha = vec![vec![0.0; d]; k];
        for j in 0..k {
            let gsum: f64 = gamma.iter().map(|g| g[j]).sum();
            for t in 0..d {
                inv_l[j][t] = 1.0 / (1.0 + FA / FB * gsum * phi[t]);
                let gr: f64 = gamma.iter().zip(&rho).map(|(g, r)| g[j] * r[t]).sum();
                alpha[j][t] = FA / FB * inv_l[j][t] * gr;
            }
        }
        let penalty: Vec<f64> = (0..k)
            .map(|j| (0..d).map(|t| (inv_l[j][t] + alpha[j][t] * alpha[j][t]) * phi[t]).sum::<f64>())
            .collect();
        let lpi: Vec<f64> = pi.iter().map(|p| (p + 1e-8).ln()).collect();
        let mut total = 0.0;
        for (n, r) in rho.iter().enumerate() {
            let lp: Vec<f64> = (0..k)
                .map(|j| FA * (r.iter().zip(&alpha[j]).map(|(a, b)| a * b).sum::<f64>() - 0.5 * penalty[j] + g[n]) + lpi[j])
                .collect();
            let m = lp.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
            let lse = m + lp.iter().map(|l| (l - m).exp()).sum::<f64>().ln();
            total += lse;
            gamma[n].iter_mut().zip(&lp).for_each(|(g, l)| *g = (l - lse).exp());
        }
        let sums: Vec<f64> = (0..k).map(|j| gamma.iter().map(|g| g[j]).sum()).collect();
        let all: f64 = sums.iter().sum();
        pi = sums.iter().map(|s| s / all).collect();
        let reg: f64 = (0..k)
            .flat_map(|j| (0..d).map(move |t| (j, t)))
            .map(|(j, t)| inv_l[j][t].ln() - inv_l[j][t] - alpha[j][t] * alpha[j][t] + 1.0)
            .sum();
        let elbo = total + FB * 0.5 * reg;
        if iter > 0 && elbo - elbo_prev < EPSILON {
            break;
        }
        elbo_prev = elbo;
    }
    (gamma, pi)
}

/// Назначение спикеров окна разным кластерам с наибольшей суммой сходства, как
/// `linear_sum_assignment(maximize=True)`: назначается min(спикеров, кластеров) пар.
/// Спикеров в окне не больше трёх — хватает полного перебора.
fn assign(scores: &[Vec<f64>]) -> Vec<Option<usize>> {
    struct Search<'a> {
        scores: &'a [Vec<f64>],
        need: usize,
        used: Vec<bool>,
        cur: Vec<Option<usize>>,
        best: (f64, Vec<Option<usize>>),
    }
    impl Search<'_> {
        fn go(&mut self, row: usize, assigned: usize, sum: f64) {
            let rows = self.scores.len();
            if row == rows {
                if assigned == self.need && sum > self.best.0 {
                    self.best = (sum, self.cur.clone());
                }
                return;
            }
            if rows - row > self.need - assigned {
                self.cur[row] = None;
                self.go(row + 1, assigned, sum);
            }
            if assigned < self.need {
                for c in 0..self.used.len() {
                    if !self.used[c] {
                        self.used[c] = true;
                        self.cur[row] = Some(c);
                        self.go(row + 1, assigned + 1, sum + self.scores[row][c]);
                        self.used[c] = false;
                    }
                }
            }
            self.cur[row] = None;
        }
    }
    let k = scores.first().map_or(0, |r| r.len());
    let mut s = Search {
        scores,
        need: scores.len().min(k),
        used: vec![false; k],
        cur: vec![None; scores.len()],
        best: (f64::NEG_INFINITY, vec![None; scores.len()]),
    };
    s.go(0, 0, 0.0);
    s.best.1
}

/// Архив NumPy .npz: zip из файлов .npy.
struct Npz(zip::ZipArchive<std::fs::File>);

impl Npz {
    fn open(path: &Path) -> Result<Self> {
        let f = std::fs::File::open(path).with_context(|| format!("нет файла {}", path.display()))?;
        Ok(Self(zip::ZipArchive::new(f)?))
    }

    /// Массив float32/float64 в порядке C, как плоский вектор.
    fn array(&mut self, name: &str) -> Result<Vec<f64>> {
        let mut bytes = vec![];
        self.0.by_name(&format!("{name}.npy"))?.read_to_end(&mut bytes)?;
        let bad = || anyhow!("{name}.npy: неподдерживаемый формат");
        if bytes.get(..6) != Some(b"\x93NUMPY") || bytes.len() < 10 {
            return Err(bad());
        }
        let (len, start) = if bytes[6] == 1 {
            (u16::from_le_bytes([bytes[8], bytes[9]]) as usize, 10)
        } else {
            (u32::from_le_bytes(bytes.get(8..12).ok_or_else(bad)?.try_into()?) as usize, 12)
        };
        let header = std::str::from_utf8(bytes.get(start..start + len).ok_or_else(bad)?)?;
        if header.contains("'fortran_order': True") {
            return Err(bad());
        }
        let data = &bytes[start + len..];
        if header.contains("'descr': '<f8'") {
            Ok(data.as_chunks::<8>().0.iter().map(|b| f64::from_le_bytes(*b)).collect())
        } else if header.contains("'descr': '<f4'") {
            Ok(data.as_chunks::<4>().0.iter().map(|b| f32::from_le_bytes(*b) as f64).collect())
        } else {
            Err(bad())
        }
    }
}
