//! Локальные речевые модели (sherpa-onnx): VAD, распознавание, диаризация, голосовые эмбеддинги.

use crate::media::SAMPLE_RATE;
use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use sherpa_onnx::*;
use std::path::Path;

/// Модель распознавания речи.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AsrModel {
    /// GigaAM v3 (Сбер) с пунктуацией: для русского точнее и быстрее.
    #[default]
    Gigaam,
    /// Whisper large-v3-turbo (OpenAI): многоязычная.
    WhisperTurbo,
}

impl AsrModel {
    pub fn dir(self) -> &'static str {
        match self {
            Self::Gigaam => "sherpa-onnx-nemo-transducer-punct-giga-am-v3-russian-2025-12-16",
            Self::WhisperTurbo => "sherpa-onnx-whisper-turbo",
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Self::Gigaam => "GigaAM v3",
            Self::WhisperTurbo => "Whisper large-v3-turbo",
        }
    }

    fn config(self, models: &Path) -> OfflineRecognizerConfig {
        let dir = models.join(self.dir());
        let mut rc = OfflineRecognizerConfig::default();
        match self {
            Self::Gigaam => {
                rc.model_config.transducer = OfflineTransducerModelConfig {
                    encoder: p(dir.join("encoder.int8.onnx")),
                    decoder: p(dir.join("decoder.onnx")),
                    joiner: p(dir.join("joiner.onnx")),
                };
                rc.model_config.tokens = p(dir.join("tokens.txt"));
                rc.model_config.model_type = Some("nemo_transducer".into());
            }
            Self::WhisperTurbo => {
                rc.model_config.whisper = OfflineWhisperModelConfig {
                    encoder: p(dir.join("turbo-encoder.int8.onnx")),
                    decoder: p(dir.join("turbo-decoder.int8.onnx")),
                    language: Some("ru".into()),
                    task: Some("transcribe".into()),
                    tail_paddings: 0,
                    enable_token_timestamps: false,
                    enable_segment_timestamps: false,
                };
                rc.model_config.tokens = p(dir.join("turbo-tokens.txt"));
            }
        }
        rc.model_config.num_threads = threads();
        rc.decoding_method = Some("greedy_search".into());
        rc
    }
}

pub struct Word {
    pub text: String,
    pub start: f32,
    pub end: f32,
}

/// Фрагмент речи между паузами (по VAD) с распознанными словами.
pub struct Segment {
    pub start: f32,
    pub end: f32,
    pub words: Vec<Word>,
}

/// Отрезок диаризации: кто говорил с `start` по `end`.
#[derive(Clone, Copy)]
pub struct Turn {
    pub start: f32,
    pub end: f32,
    pub speaker: i32,
}

pub struct Engines {
    pub asr: AsrModel,
    recognizer: OfflineRecognizer,
    vad: VadModelConfig,
    diarizer: OfflineSpeakerDiarization,
    embedder: SpeakerEmbeddingExtractor,
}

pub const VAD_FILE: &str = "silero_vad.onnx";
pub const SEGMENTATION_DIR: &str = "sherpa-onnx-pyannote-segmentation-3-0";
pub const EMBEDDING_FILE: &str = "wespeaker_en_voxceleb_resnet34_LM.onnx";

fn p(path: impl AsRef<Path>) -> Option<String> {
    Some(path.as_ref().to_string_lossy().into_owned())
}

fn threads() -> i32 {
    std::thread::available_parallelism()
        .map(|n| (n.get() as i32 - 2).clamp(2, 8))
        .unwrap_or(4)
}

impl Engines {
    pub fn load(models: &Path, asr: AsrModel, cluster_threshold: f32) -> Result<Self> {
        let recognizer = OfflineRecognizer::create(&asr.config(models))
            .ok_or_else(|| anyhow!("не удалось загрузить модель распознавания {}", asr.title()))?;

        let vad = VadModelConfig {
            silero_vad: SileroVadModelConfig {
                model: p(models.join(VAD_FILE)),
                threshold: 0.5,
                min_silence_duration: 0.5,
                min_speech_duration: 0.25,
                window_size: 512,
                max_speech_duration: 20.0,
            },
            ten_vad: Default::default(),
            sample_rate: SAMPLE_RATE,
            num_threads: 1,
            provider: None,
            debug: false,
        };

        let embedding = SpeakerEmbeddingExtractorConfig {
            model: p(models.join(EMBEDDING_FILE)),
            num_threads: threads(),
            ..Default::default()
        };
        let diarizer = OfflineSpeakerDiarization::create(&OfflineSpeakerDiarizationConfig {
            segmentation: OfflineSpeakerSegmentationModelConfig {
                pyannote: OfflineSpeakerSegmentationPyannoteModelConfig {
                    model: p(models.join(SEGMENTATION_DIR).join("model.onnx")),
                    // Сдвиг окна 10% (по умолчанию) почти не улучшает разметку, но в 2–3 раза медленнее.
                    window_shift_ratio: 0.25,
                },
                num_threads: threads(),
                ..Default::default()
            },
            embedding: embedding.clone(),
            clustering: FastClusteringConfig {
                num_clusters: -1,
                threshold: cluster_threshold,
                ..Default::default()
            },
            min_duration_on: 0.3,
            min_duration_off: 0.5,
            ..Default::default()
        })
        .ok_or_else(|| anyhow!("не удалось загрузить модель диаризации"))?;
        let embedder = SpeakerEmbeddingExtractor::create(&embedding)
            .ok_or_else(|| anyhow!("не удалось загрузить модель голосовых эмбеддингов"))?;

        Ok(Self { asr, recognizer, vad, diarizer, embedder })
    }

    /// Режет запись на фрагменты речи по паузам.
    pub fn speech_regions(&self, samples: &[f32]) -> Result<Vec<(usize, Vec<f32>)>> {
        let vad = VoiceActivityDetector::create(&self.vad, 60.0)
            .ok_or_else(|| anyhow!("не удалось создать VAD"))?;
        let mut out = vec![];
        let mut drain = |vad: &VoiceActivityDetector| {
            while !vad.is_empty() {
                if let Some(seg) = vad.front() {
                    out.push((seg.start() as usize, seg.samples().to_vec()));
                }
                vad.pop();
            }
        };
        for chunk in samples.chunks(512) {
            vad.accept_waveform(chunk);
            drain(&vad);
        }
        vad.flush();
        drain(&vad);
        Ok(out)
    }

    /// Распознаёт фрагменты пачками; `progress` получает долю выполненного.
    pub fn recognize(
        &self,
        regions: &[(usize, Vec<f32>)],
        mut progress: impl FnMut(f32),
    ) -> Vec<Segment> {
        let mut segments = Vec::with_capacity(regions.len());
        for (i, batch) in regions.chunks(8).enumerate() {
            let streams: Vec<_> = batch
                .iter()
                .map(|(_, s)| {
                    let st = self.recognizer.create_stream();
                    st.accept_waveform(SAMPLE_RATE, s);
                    st
                })
                .collect();
            self.recognizer
                .decode_multiple_streams(&streams.iter().collect::<Vec<_>>());
            for ((start, s), st) in batch.iter().zip(&streams) {
                let offset = *start as f32 / SAMPLE_RATE as f32;
                let end = offset + s.len() as f32 / SAMPLE_RATE as f32;
                let words = st
                    .get_result()
                    .map(|r| match self.asr {
                        AsrModel::Gigaam => tokens_to_words(&r.tokens, r.timestamps.as_deref(), offset, end),
                        AsrModel::WhisperTurbo => text_to_words(&r.text, offset, end),
                    })
                    .unwrap_or_default();
                segments.push(Segment { start: offset, end, words });
            }
            progress(((i + 1) * 8).min(regions.len()) as f32 / regions.len().max(1) as f32);
        }
        segments
    }

    pub fn diarize(&self, samples: &[f32]) -> Result<Vec<Turn>> {
        let r = self
            .diarizer
            .process(samples)
            .ok_or_else(|| anyhow!("диаризация не удалась"))?;
        Ok(r.sort_by_start_time()
            .into_iter()
            .map(|s| Turn { start: s.start, end: s.end, speaker: s.speaker })
            .collect())
    }

    /// L2-нормированный голосовой эмбеддинг фрагмента.
    pub fn embed(&self, samples: &[f32]) -> Option<Vec<f32>> {
        let st = self.embedder.create_stream()?;
        st.accept_waveform(SAMPLE_RATE, samples);
        st.input_finished();
        if !self.embedder.is_ready(&st) {
            return None;
        }
        let mut v = self.embedder.compute(&st)?;
        normalize(&mut v);
        Some(v)
    }
}

pub fn normalize(v: &mut [f32]) {
    let n = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if n > 0.0 {
        v.iter_mut().for_each(|x| *x /= n);
    }
}

pub fn cosine(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

/// Whisper не даёт надёжных таймкодов слов: распределяем слова по фрагменту
/// пропорционально длине. Фрагменты короткие (≤ 20 с), для разметки спикеров этого хватает.
fn text_to_words(text: &str, offset: f32, end: f32) -> Vec<Word> {
    if is_hallucination(text) {
        return vec![];
    }
    let parts: Vec<&str> = text.split_whitespace().collect();
    let total = parts.iter().map(|w| w.chars().count() + 1).sum::<usize>().max(1) as f32;
    let mut pos = 0usize;
    parts
        .iter()
        .map(|w| {
            let len = w.chars().count() + 1;
            let a = offset + (end - offset) * pos as f32 / total;
            pos += len;
            Word { text: w.to_string(), start: a, end: offset + (end - offset) * pos as f32 / total }
        })
        .collect()
}

/// Известные «галлюцинации» Whisper на русском — фразы из субтитров обучающих данных,
/// которые модель выдаёт на тишине и шуме.
fn is_hallucination(text: &str) -> bool {
    const PHRASES: &[&str] = &[
        "субтитры", "продолжение следует", "спасибо за просмотр", "редактор субтитров",
        "корректор", "подписывайтесь на канал", "ставьте лайк",
    ];
    let t = text.trim().to_lowercase();
    t.is_empty() || (t.chars().count() < 80 && PHRASES.iter().any(|p| t.contains(p)))
}

/// Токены SentencePiece ("▁" = начало слова) → слова с таймкодами.
fn tokens_to_words(tokens: &[String], ts: Option<&[f32]>, offset: f32, end: f32) -> Vec<Word> {
    let mut words: Vec<Word> = vec![];
    for (i, tok) in tokens.iter().enumerate() {
        let t = offset + ts.and_then(|t| t.get(i)).copied().unwrap_or(0.0);
        match (tok.strip_prefix('▁'), words.last_mut()) {
            (Some(rest), _) => words.push(Word { text: rest.to_string(), start: t, end: t }),
            (None, Some(last)) => last.text.push_str(tok),
            (None, None) => words.push(Word { text: tok.clone(), start: t, end: t }),
        }
    }
    words.retain(|w| !w.text.trim().is_empty());
    for i in 0..words.len() {
        words[i].end = words.get(i + 1).map(|n| n.start).unwrap_or(end);
    }
    words
}
