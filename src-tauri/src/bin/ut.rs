//! CLI для проверки конвейера без интерфейса: `ut <файл> [--protocol] [--whisper]`.

use std::sync::{mpsc, Arc};
use ultra_transcript_lib::{models, pipeline, service::{Job, Service}, speech::AsrModel, store::Store};

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let file = args.iter().find(|a| !a.starts_with("--")).expect("использование: ut <файл> [--protocol] [--whisper]");
    let store = Arc::new(Store::open()?);
    pipeline::ensure_templates(&store)?;
    let asr = if args.iter().any(|a| a == "--whisper") { AsrModel::WhisperTurbo } else { AsrModel::Gigaam };
    for m in models::required(&store.models_dir(), asr).into_iter().filter(|m| !m.installed) {
        eprintln!("скачиваю {}", m.title);
        models::install(&store.models_dir(), &m, &|_| {})?;
    }
    let (tx, rx) = mpsc::channel();
    let svc = Service::start(store.clone(), Arc::new(move |e| { let _ = tx.send(e); }), false);
    let t0 = std::time::Instant::now();
    let mut settings = store.settings();
    settings.asr_model = asr;
    store.save_settings(&settings)?;
    let id = svc.import(std::fs::canonicalize(file)?, None)?;
    let mut protocol_requested = !args.iter().any(|a| a == "--protocol");
    let mut last = String::new();
    for e in rx {
        if e.recording_id != id { continue; }
        let line = format!("{} {}", e.status, e.stage);
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
    Ok(())
}
