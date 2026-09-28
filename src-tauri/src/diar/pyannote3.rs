//! pyannote 3.0 через sherpa-onnx: сегментация, голосовые отпечатки WeSpeaker и иерархическая
//! кластеризация с порогом. Работает через C API: только там есть колбэк прогресса.

use crate::speech::{threads, Turn, EMBEDDING_FILE, SEGMENTATION_DIR};
use anyhow::{anyhow, Result};
use sherpa_onnx_sys as sys;
use std::ffi::{c_void, CString};
use std::path::{Path, PathBuf};

extern "C" {
    fn SherpaOnnxOfflineSpeakerDiarizationProcessWithCallback(
        sd: *const sys::OfflineSpeakerDiarization,
        samples: *const f32,
        n: i32,
        callback: extern "C" fn(i32, i32, *mut c_void) -> i32,
        arg: *mut c_void,
    ) -> *const sys::OfflineSpeakerDiarizationResult;
}

pub struct Pyannote3(*const sys::OfflineSpeakerDiarization);

// SAFETY: как и в обёртке sherpa-onnx — объект вызывается из одного потока за раз.
unsafe impl Send for Pyannote3 {}
unsafe impl Sync for Pyannote3 {}

fn clustering(threshold: f32) -> sys::FastClusteringConfig {
    sys::FastClusteringConfig { num_clusters: -1, threshold, compute_confidence: 0 }
}

impl Pyannote3 {
    pub fn load(models: &Path) -> Result<Self> {
        let path = |p: PathBuf| CString::new(p.to_string_lossy().into_owned()).ok();
        let fail = || anyhow!(crate::lang::tr("не удалось загрузить модель разделения по спикерам", "could not load the speaker separation model"));
        let segmentation = path(models.join(SEGMENTATION_DIR).join("model.onnx")).ok_or_else(fail)?;
        let embedding = path(models.join(EMBEDDING_FILE)).ok_or_else(fail)?;
        let config = sys::OfflineSpeakerDiarizationConfig {
            segmentation: sys::OfflineSpeakerSegmentationModelConfig {
                pyannote: sys::OfflineSpeakerSegmentationPyannoteModelConfig {
                    model: segmentation.as_ptr(),
                    // Сдвиг окна 10% (по умолчанию) почти не улучшает разметку, но в 2–3 раза медленнее.
                    window_shift_ratio: 0.25,
                },
                num_threads: threads(),
                debug: 0,
                provider: c"cpu".as_ptr(),
            },
            embedding: sys::SpeakerEmbeddingExtractorConfig {
                model: embedding.as_ptr(),
                num_threads: threads(),
                debug: 0,
                provider: c"cpu".as_ptr(),
            },
            // Порог задаётся перед каждым запуском (см. diarize): его можно менять без перезагрузки.
            clustering: clustering(0.5),
            min_duration_on: 0.3,
            min_duration_off: 0.5,
        };
        // SAFETY: строки конфигурации живы до конца вызова, C API копирует их.
        let sd = unsafe { sys::SherpaOnnxCreateOfflineSpeakerDiarization(&config) };
        if sd.is_null() {
            return Err(fail());
        }
        Ok(Self(sd))
    }

    /// Прогресс считается по голосовым эмбеддингам фрагментов; сегментация идёт до них, без прогресса.
    pub fn diarize(&self, samples: &[f32], threshold: f32, progress: &mut dyn FnMut(f32)) -> Result<Vec<Turn>> {
        // Колбэк приходит на каждый фрагмент — тысячи раз на часовой записи; наружу — целые проценты.
        let mut last = 0;
        let mut on_fraction = |f: f32| {
            let pct = (f * 100.0) as u32;
            if pct > last {
                last = pct;
                progress(f);
            }
        };
        extern "C" fn on_progress(done: i32, total: i32, arg: *mut c_void) -> i32 {
            // SAFETY: arg — `&mut on_fraction` из diarize(); колбэк вызывается синхронно внутри неё.
            let f = unsafe { &mut *arg.cast::<&mut dyn FnMut(f32)>() };
            f(done as f32 / total.max(1) as f32);
            0
        }
        let mut callback: &mut dyn FnMut(f32) = &mut on_fraction;
        // Меняется только кластеризация: C API читает из конфигурации одно это поле.
        // SAFETY: нулевые указатели в остальных полях C API заменяет значениями по умолчанию.
        let config = sys::OfflineSpeakerDiarizationConfig { clustering: clustering(threshold), ..unsafe { std::mem::zeroed() } };
        // SAFETY: self.0 жив, пока жив Pyannote3; результат и сегменты освобождаются здесь же.
        unsafe {
            sys::SherpaOnnxOfflineSpeakerDiarizationSetConfig(self.0, &config);
            let r = SherpaOnnxOfflineSpeakerDiarizationProcessWithCallback(
                self.0,
                samples.as_ptr(),
                samples.len() as i32,
                on_progress,
                (&mut callback as *mut &mut dyn FnMut(f32)).cast(),
            );
            if r.is_null() {
                return Err(anyhow!(crate::lang::tr("не удалось разделить запись по спикерам", "could not separate the speakers")));
            }
            let n = sys::SherpaOnnxOfflineSpeakerDiarizationResultGetNumSegments(r);
            let mut turns = vec![];
            if n > 0 {
                let segs = sys::SherpaOnnxOfflineSpeakerDiarizationResultSortByStartTime(r);
                if !segs.is_null() {
                    turns = std::slice::from_raw_parts(segs, n as usize)
                        .iter()
                        .map(|s| Turn { start: s.start, end: s.end, speaker: s.speaker })
                        .collect();
                    sys::SherpaOnnxOfflineSpeakerDiarizationDestroySegment(segs);
                }
            }
            sys::SherpaOnnxOfflineSpeakerDiarizationDestroyResult(r);
            Ok(turns)
        }
    }
}

impl Drop for Pyannote3 {
    fn drop(&mut self) {
        // SAFETY: указатель получен от SherpaOnnxCreateOfflineSpeakerDiarization и не null.
        unsafe { sys::SherpaOnnxDestroyOfflineSpeakerDiarization(self.0) }
    }
}
