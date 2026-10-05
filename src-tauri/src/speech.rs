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
    /// GLM-ASR-Nano (Zhipu AI): распознавание языковой моделью в llama.cpp, 17 языков.
    GlmAsr,
}

impl AsrModel {
    pub fn dir(self) -> &'static str {
        match self {
            Self::Gigaam => "sherpa-onnx-nemo-transducer-punct-giga-am-v3-russian-2025-12-16",
            Self::WhisperTurbo => "sherpa-onnx-whisper-turbo",
            Self::Parakeet => "sherpa-onnx-nemo-parakeet-tdt-0.6b-v3-int8",
            Self::GlmAsr => "glm-asr-nano-2512",
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Self::Gigaam => "GigaAM v3",
            Self::WhisperTurbo => "Whisper large-v3-turbo",
            Self::Parakeet => "Parakeet TDT 0.6B v3",
            Self::GlmAsr => "GLM-ASR-Nano",
        }
    }

    /// Язык, на котором модель распознаёт: GigaAM — только русский, Whisper и Parakeet — заданный
    /// (`language`) или, если задано "auto", определяют сами — тогда здесь пусто.
    /// Parakeet язык напрямую не задаётся: фрагменты не на том языке она распознаёт повторно
    /// (см. `Engines::fix_language`). GLM-ASR язык подсказывается инструкцией (`glm_asr::task`).
    pub fn language(self, language: &str) -> String {
        let language = language.trim().to_lowercase();
        match self {
            Self::Gigaam => "ru".into(),
            _ if language == "auto" => String::new(),
            Self::Parakeet if !PARAKEET_LANGUAGES.contains(&language.as_str()) => String::new(),
            Self::GlmAsr if !crate::glm_asr::LANGUAGES.iter().any(|(c, _)| *c == language) => String::new(),
            _ => language,
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
            Self::GlmAsr => unreachable!("GLM-ASR работает в llama.cpp, не в sherpa-onnx"),
        }
        rc.model_config.num_threads = threads();
        rc.decoding_method = Some("greedy_search".into());
        rc
    }
}

/// Языки Parakeet TDT 0.6B v3.
pub const PARAKEET_LANGUAGES: [&str; 25] = [
    "bg", "cs", "da", "de", "el", "en", "es", "et", "fi", "fr", "hr", "hu", "it", "lt", "lv", "mt", "nl", "pl", "pt",
    "ro", "ru", "sk", "sl", "sv", "uk",
];

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
    recognizer: Recognizer,
    voice: VoicePrinter,
}

enum Recognizer {
    Sherpa(OfflineRecognizer),
    Glm(crate::glm_asr::GlmAsr),
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
        let recognizer = match asr {
            AsrModel::GlmAsr => Recognizer::Glm(crate::glm_asr::GlmAsr::load(&models.join(asr.dir()))?),
            _ => Recognizer::Sherpa(OfflineRecognizer::create(&asr.config(models, &language)).ok_or_else(|| {
                anyhow!("{} {}", tr("не удалось загрузить модель распознавания", "could not load the speech model"), asr.title())
            })?),
        };
        let voice = VoicePrinter::load(models)?;
        Ok(Self { asr, language, recognizer, voice })
    }

    /// Режет запись на фрагменты речи по паузам.
    pub fn speech_regions(&self, samples: &[f32]) -> Result<Vec<(usize, Vec<f32>)>> {
        self.voice.speech_regions(samples)
    }

    /// Поиск речи для потока: фразы между паузами достаются по мере поступления звука.
    pub fn vad(&self) -> Result<VoiceActivityDetector> {
        self.voice.vad()
    }

    /// Распознаёт фрагменты пачками; после каждой пачки `progress` получает долю выполненного
    /// и только что распознанные фрагменты — их можно показывать, не дожидаясь конца.
    pub fn recognize(
        &self,
        regions: &[(usize, Vec<f32>)],
        mut progress: impl FnMut(f32, &[Segment]),
    ) -> Vec<Segment> {
        let sr = SAMPLE_RATE as f32;
        let mut segments = Vec::with_capacity(regions.len());
        // Фрагменты не на том языке, для которых ещё не нашлось подсказки.
        let mut pending = vec![];
        for (i, batch) in regions.chunks(8).enumerate() {
            let first = segments.len();
            let clips: Vec<_> = batch.iter().map(|(start, s)| (*start as f32 / sr, s.as_slice())).collect();
            for ((offset, s), words) in clips.iter().zip(self.decode(&clips)) {
                segments.push(Segment { start: *offset, end: offset + s.len() as f32 / sr, words });
            }
            pending.extend(first..segments.len());
            pending = self.fix_language(regions, &mut segments, pending);
            let done = ((i + 1) * 8).min(regions.len());
            progress(done as f32 / regions.len().max(1) as f32, &segments[first..]);
        }
        segments
    }

    /// Распознаёт отрывки разом; у каждого — его начало в записи, с него отсчитываются таймкоды слов.
    fn decode(&self, clips: &[(f32, &[f32])]) -> Vec<Vec<Word>> {
        // sherpa-onnx на пустой пачке разыменовывает нулевой указатель и роняет процесс.
        if clips.is_empty() {
            return vec![];
        }
        let recognizer = match &self.recognizer {
            Recognizer::Sherpa(r) => r,
            Recognizer::Glm(glm) => {
                return clips
                    .iter()
                    .map(|(offset, s)| {
                        let end = offset + s.len() as f32 / SAMPLE_RATE as f32;
                        match glm.transcribe(s, &self.language) {
                            Ok(text) => text_to_words(&text, *offset, end),
                            Err(e) => {
                                eprintln!("GLM-ASR: {e:#}");
                                vec![]
                            }
                        }
                    })
                    .collect();
            }
        };
        let streams: Vec<_> = clips
            .iter()
            .map(|(_, s)| {
                let st = recognizer.create_stream();
                st.accept_waveform(SAMPLE_RATE, s);
                st
            })
            .collect();
        recognizer.decode_multiple_streams(&streams.iter().collect::<Vec<_>>());
        clips
            .iter()
            .zip(&streams)
            .map(|((offset, s), st)| {
                let end = offset + s.len() as f32 / SAMPLE_RATE as f32;
                st.get_result()
                    .map(|r| match self.asr {
                        AsrModel::Gigaam | AsrModel::Parakeet => tokens_to_words(&r.tokens, r.timestamps.as_deref(), *offset, end),
                        AsrModel::WhisperTurbo | AsrModel::GlmAsr => text_to_words(&r.text, *offset, end),
                    })
                    .unwrap_or_default()
            })
            .collect()
    }

    /// Parakeet язык не задаётся, и на коротких фрагментах — «угу», «да», обрывки фраз — она
    /// иногда сбивается на другой: русская речь выходит английскими словами. Такие фрагменты
    /// (`pending` — их номера; слова не той азбуки) распознаём заново, добавив после них
    /// несколько секунд речи из той же записи, уже распознанной на нужном языке: по ней модель
    /// и определяет язык. Подсказка перед фрагментом хуже: короткий фрагмент после неё модель
    /// чаще пропускает целиком. Новый текст берём, если в нём меньше чужих слов.
    /// Возвращает фрагменты, для которых подсказки пока нет.
    fn fix_language(&self, regions: &[(usize, Vec<f32>)], segments: &mut [Segment], pending: Vec<usize>) -> Vec<usize> {
        let Some(native) = alphabet(&self.language).filter(|_| self.asr == AsrModel::Parakeet) else {
            return vec![];
        };
        let pending: Vec<usize> = pending.into_iter().filter(|&k| foreign_words(&segments[k].words, native) > 0).collect();
        // Подсказка — фрагмент целиком на нужном языке, не короче 4 слов.
        let hints: Vec<usize> = (0..segments.len())
            .filter(|&j| segments[j].words.len() >= 4 && foreign_words(&segments[j].words, native) == 0)
            .collect();
        if pending.is_empty() || hints.is_empty() {
            return pending;
        }
        const HINT_SECONDS: f32 = 6.0;
        const GAP_SECONDS: f32 = 0.3;
        let sr = SAMPLE_RATE as f32;
        let clips: Vec<(usize, Vec<f32>)> = pending
            .iter()
            .map(|&k| {
                let j = *hints.iter().min_by_key(|&&j| j.abs_diff(k)).unwrap();
                let hint = &segments[j];
                // Подсказку режем на границе слова.
                let cut = hint.words.iter().map(|w| w.start - hint.start).find(|&t| t > HINT_SECONDS).unwrap_or(f32::MAX);
                let hint_audio = &regions[j].1[..((cut * sr) as usize).min(regions[j].1.len())];
                let gap = vec![0.0; (GAP_SECONDS * sr) as usize];
                (k, [&regions[k].1, &gap, hint_audio].concat())
            })
            .collect();
        let decoded = self.decode(&clips.iter().map(|(k, a)| (segments[*k].start, a.as_slice())).collect::<Vec<_>>());
        for ((k, _), words) in clips.iter().zip(decoded) {
            let seg = &mut segments[*k];
            // Слова подсказки отбрасываем.
            let mut words: Vec<Word> = words.into_iter().filter(|w| w.start < seg.end + GAP_SECONDS / 2.0).collect();
            if foreign_words(&words, native) < foreign_words(&seg.words, native) {
                for w in &mut words {
                    w.end = w.end.min(seg.end);
                    w.start = w.start.min(w.end);
                }
                seg.words = words;
            }
        }
        vec![]
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

    fn vad(&self) -> Result<VoiceActivityDetector> {
        VoiceActivityDetector::create(&self.vad, 60.0)
            .ok_or_else(|| anyhow!(tr("не удалось загрузить модель поиска речи", "could not load the speech detection model")))
    }

    /// Режет запись на фрагменты речи по паузам.
    pub fn speech_regions(&self, samples: &[f32]) -> Result<Vec<(usize, Vec<f32>)>> {
        let vad = self.vad()?;
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

/// Whisper и GLM-ASR не дают надёжных таймкодов слов: распределяем слова по фрагменту
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

/// Азбука языка: кириллица, греческий или латиница. None — язык не задан.
fn alphabet(language: &str) -> Option<fn(char) -> bool> {
    let f: fn(char) -> bool = match language {
        "" => return None,
        "ru" | "uk" | "be" | "bg" | "kk" | "sr" | "mk" => |c| ('\u{400}'..='\u{52F}').contains(&c),
        "el" => |c| ('\u{370}'..='\u{3FF}').contains(&c) || ('\u{1F00}'..='\u{1FFF}').contains(&c),
        _ => |c| c.is_ascii_alphabetic() || ('\u{C0}'..='\u{24F}').contains(&c),
    };
    Some(f)
}

/// Сколько слов написано не азбукой языка (числа и знаки не в счёт).
fn foreign_words(words: &[Word], native: fn(char) -> bool) -> usize {
    words
        .iter()
        .filter(|w| w.text.chars().any(char::is_alphabetic) && !w.text.chars().any(native))
        .count()
}

/// Токены SentencePiece ("▁" = начало слова; у Parakeet — пробел) → слова с таймкодами.
fn tokens_to_words(tokens: &[String], ts: Option<&[f32]>, offset: f32, end: f32) -> Vec<Word> {
    let mut words: Vec<Word> = vec![];
    for (i, tok) in tokens.iter().enumerate() {
        let t = offset + ts.and_then(|t| t.get(i)).copied().unwrap_or(0.0);
        match (tok.strip_prefix(['▁', ' ']), words.last_mut()) {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parakeet_tokens_start_words_with_a_space() {
        let tokens: Vec<String> = [" Я", " мо", "гу", " копи", " паст."].map(String::from).into();
        let words = tokens_to_words(&tokens, Some(&[0.0, 0.2, 0.3, 0.5, 0.8]), 10.0, 11.5);
        let texts: Vec<_> = words.iter().map(|w| w.text.as_str()).collect();
        assert_eq!(texts, ["Я", "могу", "копи", "паст."]);
        assert_eq!((words[1].start, words[1].end, words[3].end), (10.2, 10.5, 11.5));
    }

    #[test]
    fn counts_words_in_a_foreign_alphabet() {
        let words = |s: &str| s.split(' ').map(|w| Word { text: w.into(), start: 0.0, end: 0.0 }).collect::<Vec<_>>();
        let ru = alphabet("ru").unwrap();
        assert_eq!(foreign_words(&words("That needs по стандарту, 7 штук."), ru), 2);
        assert_eq!(foreign_words(&words("Это раз, 2 — три."), ru), 0);
        assert_eq!(foreign_words(&words("Это the end"), alphabet("en").unwrap()), 1);
        assert!(alphabet("").is_none());
    }

    #[test]
    fn parakeet_takes_only_its_languages() {
        assert_eq!(AsrModel::Parakeet.language("RU"), "ru");
        assert_eq!(AsrModel::Parakeet.language("zh"), "");
        assert_eq!(AsrModel::Parakeet.language("auto"), "");
        assert_eq!(AsrModel::WhisperTurbo.language("zh"), "zh");
        assert_eq!(AsrModel::Gigaam.language("en"), "ru");
        assert_eq!(AsrModel::GlmAsr.language("ru"), "ru");
        assert_eq!(AsrModel::GlmAsr.language("pl"), "");
    }
}

