//! Образцы голоса людей: откуда взят голос, фрагмент для прослушивания, загрузка из файла.

use crate::lang::tr;
use crate::media;
use crate::speech::{self, VoicePrinter};
use crate::store::Store;
use crate::transcript::{Transcript, Utterance};
use anyhow::{anyhow, ensure, Result};
use serde::Serialize;
use std::path::Path;

/// Сколько секунд загруженного файла идёт в отпечаток голоса и в копию для прослушивания.
const FILE_SECONDS: u32 = 120;
/// Фрагмент записи для прослушивания — не длиннее, секунд.
const LISTEN_SECONDS: f32 = 20.0;
/// Запас перед началом фрагмента: таймкод первого слова бывает чуть поздним.
const LEAD_IN: f32 = 0.25;

#[derive(Debug, Serialize)]
pub struct VoiceSample {
    pub id: i64,
    /// Название записи или имя загруженного файла; пусто — запись удалена.
    pub title: String,
    /// Загружен из файла, а не взят из расшифровки.
    pub uploaded: bool,
    pub date: String,
    /// Аудио для прослушивания; пусто — слушать нечего (запись удалена вместе с аудио).
    pub audio: String,
    /// Фрагмент в `audio`, секунды.
    pub start: f32,
    pub end: f32,
    /// Что человек говорит во фрагменте.
    pub text: String,
}

/// Образцы голоса человека с фрагментами для прослушивания, новые сверху.
pub fn samples(store: &Store, person_id: i64) -> Result<Vec<VoiceSample>> {
    let recordings = store.recordings()?;
    let mut out = vec![];
    for v in store.voices(person_id)? {
        let mut s = VoiceSample {
            id: v.id,
            title: v.label,
            uploaded: v.recording_id.is_empty(),
            date: v.created_at.chars().take(10).collect(),
            audio: String::new(),
            start: 0.0,
            end: v.seconds,
            text: String::new(),
        };
        if s.uploaded {
            let clip = store.voice_clip(&v.speaker);
            if clip.exists() {
                s.audio = clip.to_string_lossy().into_owned();
            }
        } else if let Some(r) = recordings.iter().find(|r| r.id == v.recording_id) {
            s.title = r.title.clone();
            s.date = r.created_at.chars().take(10).collect();
            let audio = store.recording_dir(&r.id).join("audio.ogg");
            let fragment = store.load_transcript(&r.id).ok().and_then(|t| fragment(&t, &v.speaker));
            if let (true, Some((start, end, text))) = (audio.exists(), fragment) {
                s.audio = audio.to_string_lossy().into_owned();
                (s.start, s.end, s.text) = (start, end, text);
            }
        }
        out.push(s);
    }
    Ok(out)
}

/// Где голос спикера слышен лучше всего: самый длинный отрезок, когда он говорит без перерыва
/// на других, — его начало. Возвращает (начало, конец, текст).
fn fragment(t: &Transcript, speaker: &str) -> Option<(f32, f32, String)> {
    let span = |run: &[Utterance]| run[run.len() - 1].end - run[0].start;
    let run = t
        .utterances
        .chunk_by(|a, b| a.speaker == b.speaker)
        .filter(|run| run[0].speaker == speaker)
        .max_by(|a, b| span(a).total_cmp(&span(b)))?;
    let start = (run[0].start - LEAD_IN).max(0.0);
    let end = run[run.len() - 1].end.min(start + LISTEN_SECONDS);
    let text = run.iter().filter(|u| u.start < end).map(|u| u.text.as_str()).collect::<Vec<_>>().join(" ");
    Some((start, end, text))
}

/// Образец голоса из файла, где говорит только этот человек: отпечаток по речи из первых
/// `FILE_SECONDS` секунд и сжатая копия этого отрезка, чтобы образец можно было послушать.
pub fn add_from_file(store: &Store, person_id: i64, input: &Path) -> Result<()> {
    ensure!(
        media::is_media(input),
        "{}: {}",
        tr("этот формат не поддерживается", "this format is not supported"),
        input.display()
    );
    let models = store.models_dir();
    ensure!(
        models.join(speech::VAD_FILE).exists() && models.join(speech::EMBEDDING_FILE).exists(),
        tr("сначала скачайте модели — кнопка на главном экране", "download the models first — the button is on the Home screen")
    );
    let samples = media::decode_head(input, FILE_SECONDS)?;
    let emb = VoicePrinter::load(&models)?.voiceprint(&samples)?.ok_or_else(|| {
        anyhow!(
            "{} {} {}",
            tr("в файле слишком мало речи — нужно хотя бы", "there is too little speech in the file — at least"),
            speech::MIN_VOICE_SECONDS,
            tr("секунды", "seconds are needed")
        )
    })?;
    let name = uuid::Uuid::new_v4().simple().to_string();
    let clip = store.voice_clip(&name);
    std::fs::create_dir_all(store.voices_dir())?;
    media::encode_clip(input, &clip, store.settings().archive_kbps, FILE_SECONDS)?;
    let label = input.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let seconds = samples.len() as f32 / media::SAMPLE_RATE as f32;
    store.add_voice_file(person_id, &name, &label, seconds, &emb).inspect_err(|_| {
        let _ = std::fs::remove_file(&clip);
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn u(speaker: &str, start: f32, end: f32, text: &str) -> Utterance {
        Utterance { speaker: speaker.into(), start, end, text: text.into(), ..Default::default() }
    }

    #[test]
    fn fragment_is_longest_uninterrupted_run() {
        let t = Transcript {
            utterances: vec![
                u("S1", 0.0, 3.0, "Добрый день."),
                u("S2", 3.5, 5.0, "Здравствуйте."),
                u("S1", 5.5, 9.0, "Начнём с бюджета."),
                u("S1", 9.25, 14.0, "Смета готова."),
                u("S2", 14.5, 40.0, "Длинный ответ."),
            ],
            ..Default::default()
        };
        assert_eq!(fragment(&t, "S1"), Some((5.25, 14.0, "Начнём с бюджета. Смета готова.".into())));
        // Длинная реплика — только начало.
        assert_eq!(fragment(&t, "S2"), Some((14.25, 34.25, "Длинный ответ.".into())));
        assert_eq!(fragment(&t, "S3"), None);
    }
}
