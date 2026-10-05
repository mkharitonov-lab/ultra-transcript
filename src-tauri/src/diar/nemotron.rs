//! NVIDIA Nemotron 3 Diarization через NeMo-Speech.cpp (C API, модель GGUF; на Mac — Metal, на Windows — процессор).
//! Одна нейросеть сразу выдаёт, кто когда говорит (до 8 спикеров), — без отпечатков и кластеризации.
//! Библиотека не линкуется, а подгружается при выборе движка: без неё приложение работает,
//! а движок недоступен. Собрать её — `src-tauri/scripts/build-nemo-speech.sh`.

use crate::media::SAMPLE_RATE;
use crate::speech::Turn;
use anyhow::{anyhow, bail, Result};
use std::ffi::{c_char, c_void, CStr, CString};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

pub const MODEL_FILE: &str = "Nemotron-3-Diarization.q8_0.gguf";

#[cfg(target_os = "macos")]
const LIBRARY: &str = "libnemo_speech_asr_c.1.dylib";
#[cfg(target_os = "windows")]
const LIBRARY: &str = "nemo_speech_asr_c.dll";
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
const LIBRARY: &str = "libnemo_speech_asr_c.so.1";

/// Где искать библиотеку: `UT_NEMO_SPEECH_DIR`, Frameworks внутри .app, папка `nemo` рядом
/// с программой (установщик Windows), рядом с программой, результат `scripts/build-nemo-speech.sh`
/// при запуске из исходников.
pub fn library_path() -> Option<PathBuf> {
    let mut dirs: Vec<PathBuf> = std::env::var_os("UT_NEMO_SPEECH_DIR").map(PathBuf::from).into_iter().collect();
    if let Some(exe_dir) = std::env::current_exe().ok().and_then(|e| e.parent().map(Path::to_path_buf)) {
        dirs.push(exe_dir.join("../Frameworks"));
        dirs.push(exe_dir.join("nemo"));
        dirs.push(exe_dir);
    }
    dirs.push(Path::new(env!("CARGO_MANIFEST_DIR")).join("target/nemo-speech/lib"));
    dirs.push(Path::new(env!("CARGO_MANIFEST_DIR")).join("target/nemo-speech/bin"));
    dirs.into_iter().map(|d| d.join(LIBRARY)).find(|p| p.exists())
}

const OK: i32 = 0;

#[repr(C)]
struct ModelConfig {
    size: usize,
    model_path: *const c_char,
    /// Номер видеокарты; -1 — процессор.
    gpu: i32,
    preset: *const c_char,
    chunk_frames: i32,
    right_context_frames: i32,
    left_context_frames: i32,
    fifo_frames: i32,
    spkcache_frames: i32,
    update_period_frames: i32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Segment {
    start_time: f64,
    end_time: f64,
    /// С единицы.
    speaker: i32,
}

/// Функции C API из include/nemo_speech/diar.h.
struct Api {
    create: unsafe extern "C" fn(*const ModelConfig, *mut *mut c_void) -> i32,
    destroy: unsafe extern "C" fn(*mut c_void),
    stream_open: unsafe extern "C" fn(*mut c_void, *mut *mut c_void) -> i32,
    push: unsafe extern "C" fn(*mut c_void, *const f32, usize, i32) -> i32,
    finish: unsafe extern "C" fn(*mut c_void) -> i32,
    close: unsafe extern "C" fn(*mut c_void),
    segments: unsafe extern "C" fn(*const c_void, *const c_void, *mut Segment, usize, *mut usize) -> i32,
    last_error: unsafe extern "C" fn() -> *const c_char,
    _library: libloading::Library,
}

/// На Windows зависимости (ggml*.dll) ищутся рядом с самой библиотекой, а не только рядом с программой.
unsafe fn load(path: &Path) -> std::result::Result<libloading::Library, libloading::Error> {
    #[cfg(windows)]
    {
        use libloading::os::windows::{Library, LOAD_WITH_ALTERED_SEARCH_PATH};
        unsafe { Library::load_with_flags(path, LOAD_WITH_ALTERED_SEARCH_PATH) }.map(Into::into)
    }
    #[cfg(not(windows))]
    unsafe {
        libloading::Library::new(path)
    }
}

fn api() -> Result<&'static Api> {
    static API: OnceLock<std::result::Result<Api, String>> = OnceLock::new();
    API.get_or_init(|| {
        let path = library_path().ok_or("библиотека NeMo-Speech.cpp не найдена — соберите её: src-tauri/scripts/build-nemo-speech.sh")?;
        // SAFETY: загружается библиотека NeMo-Speech.cpp; типы функций — из её заголовка diar.h.
        unsafe {
            let lib = load(&path).map_err(|e| format!("{}: {e}", path.display()))?;
            fn sym<T: Copy>(lib: &libloading::Library, name: &[u8]) -> std::result::Result<T, String> {
                // SAFETY: тип T задан полем Api и совпадает с объявлением в diar.h.
                unsafe { lib.get::<T>(name) }.map(|s| *s).map_err(|e| e.to_string())
            }
            Ok(Api {
                create: sym(&lib, b"nemo_speech_diar_create")?,
                destroy: sym(&lib, b"nemo_speech_diar_destroy")?,
                stream_open: sym(&lib, b"nemo_speech_diar_stream_open")?,
                push: sym(&lib, b"nemo_speech_diar_stream_push_f32")?,
                finish: sym(&lib, b"nemo_speech_diar_stream_finish")?,
                close: sym(&lib, b"nemo_speech_diar_stream_close")?,
                segments: sym(&lib, b"nemo_speech_diar_segments")?,
                last_error: sym(&lib, b"nemo_speech_asr_last_error")?,
                _library: lib,
            })
        }
    })
    .as_ref()
    .map_err(|e| anyhow!("Nemotron 3: {e}"))
}

impl Api {
    fn check(&self, status: i32, what: &str) -> Result<()> {
        if status == OK {
            return Ok(());
        }
        // SAFETY: строка ошибки принадлежит библиотеке и живёт до следующего вызова в этом потоке.
        let msg = unsafe { CStr::from_ptr((self.last_error)()) }.to_string_lossy().into_owned();
        bail!("Nemotron 3: {what}: {msg}")
    }
}

pub struct Nemotron3 {
    model: *mut c_void,
}

// SAFETY: модель можно передавать между потоками; потоки обработки создаются и закрываются внутри diarize.
unsafe impl Send for Nemotron3 {}

impl Nemotron3 {
    pub fn load(models: &Path) -> Result<Self> {
        let api = api()?;
        let path = models.join(MODEL_FILE);
        let model_path = CString::new(path.to_string_lossy().into_owned())?;
        // Потоковый режим с большими окнами (задержка 30 с): держит записи любой длины.
        let preset = c"v3-offline";
        let create = |gpu: i32| {
            let cfg = ModelConfig {
                size: std::mem::size_of::<ModelConfig>(),
                model_path: model_path.as_ptr(),
                gpu,
                preset: preset.as_ptr(),
                chunk_frames: 0,
                right_context_frames: 0,
                left_context_frames: -1,
                fifo_frames: 0,
                spkcache_frames: 0,
                update_period_frames: 0,
            };
            let mut model = std::ptr::null_mut();
            // SAFETY: конфигурация и строки живы до конца вызова; библиотека их копирует.
            let status = unsafe { (api.create)(&cfg, &mut model) };
            api.check(status, "не удалось загрузить модель").map(|_| model)
        };
        // Видеокарта, а если не вышло — процессор.
        let model = create(0).or_else(|_| create(-1))?;
        Ok(Self { model })
    }

    pub fn diarize(&mut self, samples: &[f32], progress: &mut dyn FnMut(f32)) -> Result<Vec<Turn>> {
        let api = api()?;
        let mut stream = std::ptr::null_mut();
        // SAFETY: модель жива, пока жив Nemotron3; поток закрывается до выхода из функции.
        api.check(unsafe { (api.stream_open)(self.model, &mut stream) }, "поток")?;
        let result: Result<Vec<Turn>> = (|| {
            // Модель считает по мере поступления звука — прогресс по отданным кускам.
            let piece = 30 * SAMPLE_RATE as usize;
            for (i, chunk) in samples.chunks(piece).enumerate() {
                let status = unsafe { (api.push)(stream, chunk.as_ptr(), chunk.len(), SAMPLE_RATE) };
                api.check(status, "обработка")?;
                progress(((i + 1) * piece).min(samples.len()) as f32 / samples.len().max(1) as f32);
            }
            api.check(unsafe { (api.finish)(stream) }, "обработка")?;
            let mut count = 0;
            api.check(unsafe { (api.segments)(stream, std::ptr::null(), std::ptr::null_mut(), 0, &mut count) }, "результат")?;
            let mut segments = vec![Segment::default(); count];
            api.check(
                unsafe { (api.segments)(stream, std::ptr::null(), segments.as_mut_ptr(), segments.len(), &mut count) },
                "результат",
            )?;
            segments.truncate(count);
            Ok(segments
                .iter()
                .map(|s| Turn { start: s.start_time as f32, end: s.end_time as f32, speaker: s.speaker - 1 })
                .collect())
        })();
        unsafe { (api.close)(stream) };
        result
    }
}

impl Drop for Nemotron3 {
    fn drop(&mut self) {
        if let Ok(api) = api() {
            // SAFETY: модель получена от nemo_speech_diar_create, потоков на ней уже нет.
            unsafe { (api.destroy)(self.model) }
        }
    }
}
