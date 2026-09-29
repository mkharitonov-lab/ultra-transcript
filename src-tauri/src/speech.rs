//! Локальные речевые модели (sherpa-onnx): VAD, распознавание, голосовые эмбеддинги.
//! Диаризация — в модуле `diar`.

use crate::lang::tr;
use crate::media::SAMPLE_RATE;
use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use sherpa_onnx::*;
use std::path::Path;

/// Модель распознавания речи.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AsrModel {
    /// GigaAM v3 (Сбер) с пунктуацией, только русский.
    #[default]
    Gigaam,
    /// Whisper large-v3-turbo (OpenAI): многоязычная.
    WhisperTurbo,
    /// Parakeet TDT 0.6B v3 (NVIDIA): 25 европейских языков, язык определяет сама.
    Parakeet,
}

impl AsrModel {
    pub fn dir(self) -> &'static str {
        match self {
            Self::Gigaam => "sherpa-onnx-nemo-transducer-punct-giga-am-v3-russian-2025-12-16",
            Self::WhisperTurbo => "sherpa-onnx-whisper-turbo",
            Self::Parakeet => "sherpa-onnx-nemo-parakeet-tdt-0.6b-v3-int8",
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Self::Gigaam => "GigaAM v3",
            Self::WhisperTurbo => "Whisper large-v3-turbo",
            Self::Parakeet => "Parakeet TDT 0.6B v3",
        }
    }

    /// Язык, на котором модель распознаёт: GigaAM — только русский, Whisper — заданный
    /// (`language`) или, если задано "auto", определяет сам — тогда здесь пусто.
    /// Parakeet язык не задаётся — всегда определяет сама.
    pub fn language(self, language: &str) -> String {
        match self {
            Self::Gigaam => "ru".into(),
            Self::Parakeet => String::new(),
            Self::WhisperTurbo if language == "auto" => String::new(),
            Self::WhisperTurbo => language.trim().to_lowercase(),
        }
    }

    /// `language` — см. `AsrModel::language`.
    fn config(self, models: &Path, language: &str) -> OfflineRecognizerConfig {
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
            Self::Parakeet => {
                rc.model_config.transducer = OfflineTransducerModelConfig {
                    encoder: p(dir.join("encoder.int8.onnx")),
                    decoder: p(dir.join("decoder.int8.onnx")),
                    joiner: p(dir.join("joiner.int8.onnx")),
                };
                rc.model_config.tokens = p(dir.join("tokens.txt"));
                rc.model_config.model_type = Some("nemo_transducer".into());
            }
            Self::WhisperTurbo => {
                rc.model_config.whisper = OfflineWhisperModelConfig {
                    encoder: p(dir.join("turbo-encoder.int8.onnx")),
                    decoder: p(dir.join("turbo-decoder.int8.onnx")),
                    // Без языка Whisper определяет его сам для каждого фрагмента.
                    language: Some(language.to_string()).filter(|l| !l.is_empty()),
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

impl Segment {
    pub fn text(&self) -> String {
        self.words.iter().flat_map(|w| w.text.split_whitespace()).collect::<Vec<_>>().join(" ")
    }
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
    /// Язык распознавания; пусто — модель определяет его сама.
    pub language: String,
    recognizer: OfflineRecognizer,
    voice: VoicePrinter,
}

/// Поиск речи и голосовые эмбеддинги — без распознавания и диаризации: этого хватает,
/// чтобы снять отпечаток голоса с загруженного файла.
pub struct VoicePrinter {
    vad: VadModelConfig,
    embedder: SpeakerEmbeddingExtractor,
}

pub const VAD_FILE: &str = "silero_vad.onnx";
pub const SEGMENTATION_DIR: &str = "sherpa-onnx-pyannote-segmentation-3-0";
pub const EMBEDDING_FILE: &str = "wespeaker_en_voxceleb_resnet34_LM.onnx";

fn p(path: impl AsRef<Path>) -> Option<String> {
    Some(path.as_ref().to_string_lossy().into_owned())
}

pub(crate) fn threads() -> i32 {
    std::thread::available_parallelism()
        .map(|n| (n.get() as i32 - 2).clamp(2, 8))
        .unwrap_or(4)
}

impl Engines {
    /// `language` — язык записей из настроек (код или "auto"); нужен только Whisper.
    pub fn load(models: &Path, asr: AsrModel, language: &str) -> Result<Self> {
        let language = asr.language(language);
        let recognizer = OfflineRecognizer::create(&asr.config(models, &language)).ok_or_else(|| {
            anyhow!("{} {}", tr("не удалось загрузить модель распознавания", "could not load the speech model"), asr.title())
        })?;
        let voice = VoicePrinter::load(models)?;
        Ok(Self { asr, language, recognizer, voice })
    }

    /// Режет запись на фрагменты речи по паузам.
    pub fn speech_regions(&self, samples: &[f32]) -> Result<Vec<(usize, Vec<f32>)>> {
        self.voice.speech_regions(samples)
    }

    /// Распознаёт фрагменты пачками; после каждой пачки `progress` получает долю выполненного
    /// и только что распознанные фрагменты — их можно показывать, не дожидаясь конца.
    pub fn recognize(
        &self,
        regions: &[(usize, Vec<f32>)],
        mut progress: impl FnMut(f32, &[Segment]),
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
                        AsrModel::Gigaam | AsrModel::Parakeet => tokens_to_words(&r.tokens, r.timestamps.as_deref(), offset, end),
                        AsrModel::WhisperTurbo => text_to_words(&r.text, offset, end),
                    })
                    .unwrap_or_default();
                segments.push(Segment { start: offset, end, words });
            }
            let done = ((i + 1) * 8).min(regions.len());
            progress(done as f32 / regions.len().max(1) as f32, &segments[segments.len() - batch.len()..]);
        }
        segments
    }

    /// L2-нормированный голосовой эмбеддинг фрагмента.
    pub fn embed(&self, samples: &[f32]) -> Option<Vec<f32>> {
        self.voice.embed(samples)
    }
}

/// Сколько речи нужно для отпечатка голоса из файла, секунд.
pub const MIN_VOICE_SECONDS: f32 = 3.0;

impl VoicePrinter {
    pub fn load(models: &Path) -> Result<Self> {
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
        let embedder = SpeakerEmbeddingExtractor::create(&SpeakerEmbeddingExtractorConfig {
            model: p(models.join(EMBEDDING_FILE)),
            num_threads: threads(),
            ..Default::default()
        })
        .ok_or_else(|| anyhow!(tr("не удалось загрузить модель голосов", "could not load the voice model")))?;
        Ok(Self { vad, embedder })
    }

    /// Режет запись на фрагменты речи по паузам.
    pub fn speech_regions(&self, samples: &[f32]) -> Result<Vec<(usize, Vec<f32>)>> {
        let vad = VoiceActivityDetector::create(&self.vad, 60.0)
            .ok_or_else(|| anyhow!(tr("не удалось загрузить модель поиска речи", "could not load the speech detection model")))?;
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

    /// Отпечаток голоса по речи во всей записи, паузы не в счёт. Как для спикера в расшифровке:
    /// до 90 с речи кусками не длиннее 10 с. None — речи меньше `MIN_VOICE_SECONDS`.
    pub fn voiceprint(&self, samples: &[f32]) -> Result<Option<Vec<f32>>> {
        let sr = SAMPLE_RATE as usize;
        let (mut parts, mut total) = (vec![], 0.0);
        for (_, region) in self.speech_regions(samples)? {
            if total >= 90.0 {
                break;
            }
            let piece = &region[..region.len().min(10 * sr)];
            if let Some(e) = self.embed(piece) {
                let w = piece.len() as f32 / sr as f32;
                parts.push((e, w));
                total += w;
            }
        }
        Ok(if total < MIN_VOICE_SECONDS { None } else { centroid(&parts) })
    }
}

pub fn normalize(v: &mut [f32]) {
    let n = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if n > 0.0 {
        v.iter_mut().for_each(|x| *x /= n);
    }
}

/// Средний голос по кускам речи с весами (обычно — длительностями), L2-нормированный.
pub fn centroid(parts: &[(Vec<f32>, f32)]) -> Option<Vec<f32>> {
    let mut c = vec![0.0; parts.first()?.0.len()];
    for (e, w) in parts {
        c.iter_mut().zip(e).for_each(|(c, x)| *c += x * w);
    }
    normalize(&mut c);
    Some(c)
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

/// Известные «галлюцинации» Whisper — фразы из субтитров обучающих данных,
/// которые модель выдаёт на тишине и шуме.
fn is_hallucination(text: &str) -> bool {
    const PHRASES: &[&str] = &[
        "субтитры", "продолжение следует", "спасибо за просмотр", "редактор субтитров",
        "корректор", "подписывайтесь на канал", "ставьте лайк",
        "thanks for watching", "thank you for watching", "subtitles by", "amara.org", "please subscribe",
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
