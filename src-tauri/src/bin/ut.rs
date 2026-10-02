//! CLI для проверки конвейера без интерфейса:
//! `ut <файл> [--protocol] [--whisper|--parakeet] [--lang <язык>] [--diar off|pyannote3|community1|nemotron3] [--threshold <порог>] [--llm <модель>] [--raw] [--live]`.
//! `--raw` — без предобработки звука (шумоподавления и выравнивания громкости);
//! `--lang` — язык записи для Whisper и Parakeet (ru, en… или auto); `--live` — печатать текст по мере распознавания.

use std::sync::{mpsc, Arc};
use ultra_transcript_lib::pipeline::Live;
use ultra_transcript_lib::service::{Job, Service, Signal};
use ultra_transcript_lib::{diar::DiarModel, models, pipeline, speech::AsrModel, store::Store};

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let value = |flag: &str| args.iter().position(|a| a == flag).and_then(|i| args.get(i + 1));
    let file = args.iter().enumerate()
        .find(|(i, a)| !a.starts_with("--") && (*i == 0 || !["--llm", "--diar", "--threshold", "--lang"].contains(&args[i - 1].as_str())))
        .map(|(_, a)| a)
        .expect("использование: ut <файл> [--protocol] [--whisper|--parakeet] [--diar <движок>] [--threshold <порог>] [--llm <модель>] [--raw] [--live]");
    let raw = args.iter().any(|a| a == "--raw");
    // Своя библиотека во временной папке: CLI не должен подхватывать задачи
    // и менять настройки работающего приложения. Модели — общие.
    if std::env::var_os("UT_DATA_DIR").is_none() {
        std::env::set_var("UT_DATA_DIR", std::env::temp_dir().join("ultra-transcript-cli"));
    }
    let store = Arc::new(Store::open()?);
    pipeline::ensure_templates(&store)?;
    let asr = if args.iter().any(|a| a == "--whisper") {
        AsrModel::WhisperTurbo
    } else if args.iter().any(|a| a == "--parakeet") {
        AsrModel::Parakeet
    } else {
        AsrModel::Gigaam
    };
    let diar: DiarModel = match value("--diar") {
        Some(v) => serde_json::from_value(serde_json::Value::String(v.clone()))
            .map_err(|_| anyhow::anyhow!("--diar: off, pyannote3, community1 или nemotron3"))?,
        None => DiarModel::default(),
    };
    let denoiser = models::find(&store.models_dir(), "denoiser").filter(|_| !raw);
    for m in models::required(&store.models_dir(), asr, diar).into_iter().chain(denoiser).filter(|m| !m.installed) {
        eprintln!("скачиваю {}", m.title);
        models::install(&store.models_dir(), &m, &|_| {})?;
    }
    let (tx, rx) = mpsc::channel();
    let live = args.iter().any(|a| a == "--live");
    let svc = Service::start(store.clone(), Arc::new(move |signal| { let _ = tx.send(signal); }), false);
    let t0 = std::time::Instant::now();
    let mut settings = store.settings();
    settings.asr_model = asr;
    settings.diar_model = diar;
    settings.denoise = !raw;
    settings.level_volume = !raw;
    if let Some(v) = value("--threshold") {
        settings.cluster_threshold = v.parse()?;
    }
    settings.speech_language = value("--lang").cloned().unwrap_or_else(|| "ru".into());
    // --llm <модель из каталога>: редактура, справочники и протокол встроенной LLM.
    if let Some(i) = args.iter().position(|a| a == "--llm") {
        settings.llm_enabled = true;
        settings.llm_provider = ultra_transcript_lib::store::LlmProvider::Builtin;
        settings.llm_local_model = args.get(i + 1).cloned().unwrap_or_default();
    }
    store.save_settings(&settings)?;
    let id = svc.import(std::fs::canonicalize(file)?, None)?;
    let mut protocol_requested = !args.iter().any(|a| a == "--protocol");
    let mut last = String::new();
    for signal in rx {
        let e = match signal {
            Signal::Job(e) => e,
            Signal::Live(l) => {
                match l.live {
                    Live::Text { start, text } if live => eprintln!("    [{}] {text}", ultra_transcript_lib::transcript::fmt_time(start)),
                    Live::Draft if live => eprintln!("    — черновик сохранён"),
                    Live::Protocol { text, .. } if live => eprint!("{text}"),
                    _ => {}
                }
                continue;
            }
        };
        if e.recording_id != id { continue; }
        let line = format!("{} {}", e.status, e.title);
        if line != last || e.progress >= 1.0 {
            eprintln!("[{:6.1}s] {line} {:.0}%", t0.elapsed().as_secs_f32(), e.progress * 100.0);
            last = line;
        }
        match e.status.as_str() {
            "error" => anyhow::bail!(e.message),
            "done" if !protocol_requested => {
                protocol_requested = true;
                svc.enqueue(Job::Protocol { id: id.clone() });
            }
            "done" => {
                if !e.message.is_empty() { eprintln!("предупреждения: {}", e.message); }
                break;
            }
            _ => {}
        }
    }
    let t = store.load_transcript(&id)?;
    for s in &t.speakers {
        println!("# {} = {} {:?}", s.id, s.name, s.similarity);
    }
    for u in &t.utterances {
        println!("[{}] {}: {}", ultra_transcript_lib::transcript::fmt_time(u.start), t.speaker_name(&u.speaker), u.text);
        if u.clean != u.text { println!("        ~ {}", u.clean); }
    }
    println!("\nпапка: {}", store.recording_dir(&id).display());
    ultra_transcript_lib::local_llm::unload(); // иначе llama.cpp падает при выходе, освобождая видеопамять
    Ok(())
}
