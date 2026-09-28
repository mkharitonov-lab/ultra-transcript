//! Конвейер: Ingest → ASR → Diarize → Identify → Correct → Enrich → Render/Export.

use crate::docx::{self, Field};
use crate::speech::{self, Engines, Segment, Turn};
use crate::store::{Rule, Settings, Store, Term};
use crate::transcript::{fmt_time, Speaker, Transcript, Utterance};
use crate::{llm, media};
use anyhow::{Context, Result};
use regex::Regex;
use serde_json::{json, Map, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub type Progress<'a> = &'a dyn Fn(&str, f32);

/// Полная обработка файла. Возвращает расшифровку и предупреждения (некритичные сбои LLM).
pub fn transcribe(
    store: &Store,
    engines: &Engines,
    id: &str,
    input: &Path,
    progress: Progress,
) -> Result<(Transcript, Vec<String>)> {
    let settings = store.settings();
    let dir = store.recording_dir(id);
    std::fs::create_dir_all(&dir)?;
    let mut warnings = vec![];

    progress("Подготовка аудио", 0.0);
    let samples = media::decode(input)?;
    let duration = samples.len() as f32 / media::SAMPLE_RATE as f32;
    media::encode_archive(input, &dir.join("audio.ogg"), settings.archive_kbps)?;

    progress("Распознавание речи", 0.0);
    let regions = engines.speech_regions(&samples)?;
    let segments = engines.recognize(&regions, |f| progress("Распознавание речи", f));

    progress("Разделение по спикерам", 0.0);
    let turns = engines.diarize(&samples)?;

    let (utterances, order) = build_utterances(&segments, &turns);
    let mut t = Transcript {
        id: id.to_string(),
        title: input.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default(),
        source: input.to_string_lossy().into_owned(),
        created_at: chrono::Local::now().format("%Y-%m-%d %H:%M").to_string(),
        duration,
        asr_model: engines.asr.title().to_string(),
        speakers: vec![],
        utterances,
        protocol: None,
    };

    progress("Распознавание голосов", 0.0);
    identify(store, engines, &settings, &mut t, &samples, &turns, &order)?;

    progress("Исправление терминов", 0.0);
    let terms = store.terms()?;
    apply_aliases(&mut t, &terms);
    for u in &mut t.utterances {
        u.clean = clean_fillers(&u.text);
    }
    if settings.llm_enabled {
        if let Err(e) = polish(store, &settings, &mut t, progress) {
            warnings.push(format!("Редактура LLM: {e:#}"));
        }
        progress("Пополнение справочников", 0.0);
        if let Err(e) = enrich(store, &settings, &t) {
            warnings.push(format!("Пополнение справочников: {e:#}"));
        }
    }
    store.save_transcript(&t)?;
    Ok((t, warnings))
}

// ---------- слова → реплики ----------

/// Каждому слову — спикер из диаризации; соседние слова одного спикера — в реплику.
/// Возвращает реплики (спикер = "S<кластер>") и порядок появления кластеров.
fn build_utterances(segments: &[Segment], turns: &[Turn]) -> (Vec<Utterance>, Vec<i32>) {
    let speaker_at = |a: f32, b: f32| -> i32 {
        let b = b.min(a + 1.0).max(a + 0.05);
        turns
            .iter()
            .map(|t| (t, (b.min(t.end) - a.max(t.start)).max(0.0), (t.start - b).max(a - t.end).max(0.0)))
            .max_by(|x, y| x.1.total_cmp(&y.1).then(y.2.total_cmp(&x.2)))
            .map(|(t, _, _)| t.speaker)
            .unwrap_or(0)
    };
    let mut out: Vec<Utterance> = vec![];
    let mut order: Vec<i32> = vec![];
    for seg in segments {
        let mut spk: Vec<i32> = seg.words.iter().map(|w| speaker_at(w.start, w.end)).collect();
        // Сглаживание: 1–2 слова «чужого» спикера посреди реплики — ошибка диаризации.
        let mut i = 0;
        while i < spk.len() {
            let j = (i..spk.len()).find(|&k| spk[k] != spk[i]).unwrap_or(spk.len());
            if j - i <= 2 && i > 0 && j < spk.len() && spk[i - 1] == spk[j] {
                let s = spk[j];
                spk[i..j].iter_mut().for_each(|x| *x = s);
            }
            i = j;
        }
        for (w, s) in seg.words.iter().zip(spk) {
            if !order.contains(&s) {
                order.push(s);
            }
            let label = format!("S{s}");
            match out.last_mut() {
                Some(u) if u.speaker == label && w.start - u.end < 1.0 && u.raw.len() < 800 => {
                    u.raw.push(' ');
                    u.raw.push_str(&w.text);
                    u.end = w.end;
                }
                _ => out.push(Utterance {
                    id: out.len(),
                    speaker: label,
                    start: w.start,
                    end: w.end,
                    raw: w.text.clone(),
                    ..Default::default()
                }),
            }
        }
    }
    // Метки S1, S2… по порядку появления.
    let rename: HashMap<String, String> =
        order.iter().enumerate().map(|(i, s)| (format!("S{s}"), format!("S{}", i + 1))).collect();
    for u in &mut out {
        u.speaker = rename[&u.speaker].clone();
        u.raw = u.raw.split_whitespace().collect::<Vec<_>>().join(" ");
        u.text = capitalize(&u.raw);
    }
    (out, order)
}

// ---------- опознание голосов ----------

fn identify(
    store: &Store,
    engines: &Engines,
    settings: &Settings,
    t: &mut Transcript,
    samples: &[f32],
    turns: &[Turn],
    order: &[i32],
) -> Result<()> {
    let known = store.known_voices()?;
    let people = store.people()?;
    let sr = media::SAMPLE_RATE as f32;

    let mut embeddings = vec![];
    for (i, cluster) in order.iter().enumerate() {
        let mut own: Vec<&Turn> = turns.iter().filter(|x| x.speaker == *cluster).collect();
        own.sort_by(|a, b| (b.end - b.start).total_cmp(&(a.end - a.start)));
        let (mut acc, mut total) = (vec![], 0.0);
        for x in own.iter().take(12) {
            if total > 90.0 || (x.end - x.start < 1.0 && !acc.is_empty()) {
                break;
            }
            let a = (x.start * sr) as usize;
            let b = ((x.end.min(x.start + 10.0)) * sr).min(samples.len() as f32) as usize;
            if let Some(e) = engines.embed(&samples[a.min(b)..b]) {
                let w = x.end - x.start;
                acc.push((e, w));
                total += w;
            }
        }
        let emb = acc.first().map(|(e0, _)| {
            let mut c = vec![0.0; e0.len()];
            for (e, w) in &acc {
                c.iter_mut().zip(e).for_each(|(c, x)| *c += x * w);
            }
            speech::normalize(&mut c);
            c
        });
        embeddings.push((format!("S{}", i + 1), emb));
    }

    // Лучшее совпадение для каждой пары (спикер, человек); назначаем жадно без повторов.
    let mut cands = vec![];
    for (label, emb) in &embeddings {
        let Some(emb) = emb else { continue };
        let mut best: HashMap<i64, f32> = HashMap::new();
        for (pid, v) in &known {
            let s = speech::cosine(emb, v);
            let e = best.entry(*pid).or_insert(s);
            *e = e.max(s);
        }
        for (pid, s) in best {
            if s >= settings.voice_threshold {
                cands.push((label.clone(), pid, s));
            }
        }
    }
    cands.sort_by(|a, b| b.2.total_cmp(&a.2));
    let mut matched: HashMap<String, (i64, f32)> = HashMap::new();
    for (label, pid, s) in cands {
        if !matched.contains_key(&label) && !matched.values().any(|(p, _)| *p == pid) {
            matched.insert(label, (pid, s));
        }
    }

    for (i, (label, emb)) in embeddings.iter().enumerate() {
        let hit = matched.get(label).copied();
        let person = hit.and_then(|(pid, _)| people.iter().find(|p| p.id == Some(pid)));
        if let Some(emb) = emb {
            // Опознанный голос пополняет профиль человека; неопознанный ждёт подтверждения.
            store.save_voice(&t.id, label, person.and_then(|p| p.id), emb)?;
        }
        t.speakers.push(Speaker {
            id: label.clone(),
            name: person.map(|p| p.name.clone()).unwrap_or_else(|| format!("Спикер {}", i + 1)),
            person_id: person.and_then(|p| p.id),
            similarity: hit.map(|(_, s)| s),
        });
    }
    Ok(())
}

// ---------- исправление и очистка ----------

/// Детерминированные замены «как слышится» → каноническое написание.
fn apply_aliases(t: &mut Transcript, terms: &[Term]) {
    for term in terms {
        let variants: Vec<String> = std::iter::once(term.term.as_str())
            .chain(term.aliases.split([',', ';', '\n']))
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(regex::escape)
            .collect();
        let Ok(re) = Regex::new(&format!(r"(?i)\b(?:{})\b", variants.join("|"))) else { continue };
        for u in &mut t.utterances {
            u.text = re.replace_all(&u.text, term.term.as_str()).into_owned();
        }
    }
}

/// Базовая очистка без LLM: междометия, частые паразиты, повторы слов.
pub fn clean_fillers(text: &str) -> String {
    use std::sync::LazyLock;
    static HESITATION: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(?i)\b(?:э-э+|а-а+|ну-у+|э+м+|э+|мм+|хм+)\b[,.…]?\s*").unwrap());
    static PARASITE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?i),?\s*\b(?:короче говоря|короче|так сказать|это самое|скажем так|как бы|в общем-то)\b\s*([,.!?…]|$)").unwrap()
    });
    static LEADING: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?i)(^|[.!?…]\s+)(?:ну|вот|так вот|значит|в общем|короче),?\s+").unwrap()
    });
    let s = HESITATION.replace_all(text, "");
    // «…, короче, …» → «… …»; «…, вот.» → «….»
    let s = PARASITE.replace_all(&s, |c: &regex::Captures| match &c[1] {
        "," if c[0].starts_with(',') => " ".to_string(),
        "," => String::new(),
        p => p.to_string(),
    });
    let s = LEADING.replace_all(&s, "$1");
    // Повторы: «я я думаю» → «я думаю».
    let mut words: Vec<&str> = vec![];
    for w in s.split_whitespace() {
        let norm = |x: &str| x.trim_matches(|c: char| !c.is_alphanumeric()).to_lowercase();
        if words.last().is_some_and(|p| norm(p) == norm(w) && !norm(w).is_empty() && !p.ends_with(['.', ',', '!', '?'])) {
            continue;
        }
        words.push(w);
    }
    let s = words.join(" ").replace(" ,", ",").replace(",,", ",").replace(",.", ".");
    static SENTENCE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"([.!?…]\s+)(\p{Ll})").unwrap());
    let s = SENTENCE.replace_all(s.trim_start_matches([',', ' ']), |c: &regex::Captures| {
        format!("{}{}", &c[1], c[2].to_uppercase())
    });
    capitalize(&s)
}

fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().chain(c).collect(),
        None => String::new(),
    }
}

fn knowledge_context(store: &Store) -> Result<String> {
    let mut ctx = String::new();
    let terms = store.terms()?;
    if !terms.is_empty() {
        ctx.push_str("Словарь (писать строго так):\n");
        for t in terms.iter().take(400) {
            ctx.push_str(&format!("- {}", t.term));
            if !t.aliases.is_empty() {
                ctx.push_str(&format!(" (может быть распознано как: {})", t.aliases));
            }
            if !t.definition.is_empty() {
                ctx.push_str(&format!(" — {}", t.definition));
            }
            ctx.push('\n');
        }
    }
    let people = store.people()?;
    if !people.is_empty() {
        ctx.push_str("\nЛюди (ФИО писать строго так, склоняя по правилам):\n");
        for p in people.iter().take(400) {
            ctx.push_str(&format!("- {}", p.name));
            if !p.aliases.is_empty() {
                ctx.push_str(&format!(" (обращения: {})", p.aliases));
            }
            let role = [p.role.as_str(), p.org.as_str()].iter().filter(|s| !s.is_empty()).copied().collect::<Vec<_>>().join(", ");
            if !role.is_empty() {
                ctx.push_str(&format!(" — {role}"));
            }
            ctx.push('\n');
        }
    }
    Ok(ctx)
}

const POLISH_PROMPT: &str = "Ты редактор стенограмм совещаний на русском языке. Тебе дают реплики автоматической расшифровки с номерами.
Для КАЖДОЙ реплики верни объект:
- \"id\": номер реплики;
- \"text\": дословный текст, в котором исправлены только ошибки распознавания — термины, аббревиатуры, названия и ФИО пиши строго как в словаре и списке людей (с правильным склонением). Ничего не удаляй и не перефразируй. Если исправлять нечего — не включай поле text;
- \"clean\": тот же смысл литературным языком: без слов-паразитов (ну, вот, как бы, типа, значит, короче, э-э), повторов, оговорок и разговорных оборотов. Не сокращай содержание, не добавляй ничего от себя.
Ответ строго в JSON: {\"items\": [{\"id\": 0, \"text\": \"...\", \"clean\": \"...\"}]}";

fn polish(store: &Store, s: &Settings, t: &mut Transcript, progress: Progress) -> Result<()> {
    let ctx = knowledge_context(store)?;
    let batches: Vec<Vec<usize>> = {
        let (mut out, mut cur, mut len) = (vec![], vec![], 0);
        for (i, u) in t.utterances.iter().enumerate() {
            if len > 3500 && !cur.is_empty() {
                out.push(std::mem::take(&mut cur));
                len = 0;
            }
            len += u.text.len();
            cur.push(i);
        }
        if !cur.is_empty() {
            out.push(cur);
        }
        out
    };
    for (n, batch) in batches.iter().enumerate() {
        progress("Редактура текста", n as f32 / batches.len() as f32);
        let lines: String = batch
            .iter()
            .map(|&i| {
                let u = &t.utterances[i];
                format!("[{}] {}: {}\n", u.id, t.speaker_name(&u.speaker), u.text)
            })
            .collect();
        let resp = llm::chat_json(s, POLISH_PROMPT, &format!("{ctx}\nРеплики:\n{lines}"))?;
        for item in resp["items"].as_array().into_iter().flatten() {
            let Some(id) = item["id"].as_u64().map(|x| x as usize) else { continue };
            let Some(u) = t.utterances.iter_mut().find(|u| u.id == id) else { continue };
            if let Some(x) = item["text"].as_str().filter(|x| !x.trim().is_empty()) {
                u.text = x.trim().to_string();
            }
            if let Some(x) = item["clean"].as_str().filter(|x| !x.trim().is_empty()) {
                u.clean = x.trim().to_string();
            }
        }
    }
    Ok(())
}

const ENRICH_PROMPT: &str = "Ты ведёшь справочники для расшифровки совещаний. Найди в тексте НОВЫЕ сущности, которых нет в уже известных:
- terms: специальные термины, аббревиатуры, названия организаций, проектов, продуктов, документов (не общеупотребительные слова); definition — краткое пояснение из контекста или пустая строка;
- people: упомянутые люди с фамилией или полным именем; role — должность или роль из контекста.
Пиши в начальной форме (именительный падеж). Не выдумывай. Ответ строго в JSON: {\"terms\": [{\"term\": \"\", \"definition\": \"\"}], \"people\": [{\"name\": \"\", \"role\": \"\"}]}";

fn enrich(store: &Store, s: &Settings, t: &Transcript) -> Result<()> {
    let known_terms: Vec<String> = store.terms()?.into_iter().map(|x| x.term.to_lowercase()).collect();
    let known_people: Vec<String> = store.people()?.into_iter().map(|x| x.name.to_lowercase()).collect();
    let text: String = t.as_dialogue(false).chars().take(24_000).collect();
    let prompt = format!(
        "Уже известные термины: {}\nУже известные люди: {}\n\nТекст:\n{text}",
        known_terms.join(", "),
        known_people.join(", ")
    );
    let resp = llm::chat_json(s, ENRICH_PROMPT, &prompt)?;
    let pick = |v: &Value, k: &str| v[k].as_str().unwrap_or("").trim().to_string();
    for x in resp["terms"].as_array().into_iter().flatten() {
        let term = pick(x, "term");
        if !term.is_empty() && !known_terms.contains(&term.to_lowercase()) {
            store.suggest("term", &term, &pick(x, "definition"), &t.id)?;
        }
    }
    for x in resp["people"].as_array().into_iter().flatten() {
        let name = pick(x, "name");
        if !name.is_empty() && !known_people.contains(&name.to_lowercase()) {
            store.suggest("person", &name, &pick(x, "role"), &t.id)?;
        }
    }
    if s.auto_accept_suggestions {
        for sg in store.suggestions()? {
            if sg.recording_id == t.id {
                store.resolve_suggestion(sg.id, true)?;
            }
        }
    }
    Ok(())
}

// ---------- протокол ----------

pub fn make_protocol(store: &Store, t: &mut Transcript) -> Result<()> {
    let s = store.settings();
    anyhow::ensure!(s.llm_enabled, "для протокола нужна LLM — включите её в настройках");
    let template = protocol_template(store, &s);
    let fields = docx::fields(&template)?;
    let schema: Vec<String> = fields
        .iter()
        .map(|f| match f {
            Field::Text(n) => format!("\"{n}\": строка"),
            Field::List(n) => format!("\"{n}\": массив строк"),
            Field::Table(n, cols) => format!(
                "\"{n}\": массив объектов {{{}}}",
                cols.iter().map(|c| format!("\"{c}\": строка")).collect::<Vec<_>>().join(", ")
            ),
        })
        .collect();
    let people = store.people()?;
    let participants: Vec<String> = t
        .speakers
        .iter()
        .map(|sp| {
            let role = sp
                .person_id
                .and_then(|id| people.iter().find(|p| p.id == Some(id)))
                .map(|p| format!(" ({})", p.role))
                .filter(|r| r != " ()")
                .unwrap_or_default();
            format!("{}{role}", sp.name)
        })
        .collect();
    let system = format!(
        "Ты секретарь. Составь сжатый протокол договорённостей по расшифровке совещания: деловой стиль, \
         только факты из текста, без выдумок. Решения и поручения формулируй конкретно. \
         Если данных для поля нет — пустая строка или пустой массив.\n\
         Верни строго JSON с ключами:\n{}",
        schema.join("\n")
    );
    let user = format!(
        "Дата записи: {}\nУчастники: {}\n\n{}\n\nРасшифровка:\n{}",
        t.created_at,
        participants.join(", "),
        knowledge_context(store)?,
        t.as_dialogue(true)
    );
    let mut v = llm::chat_json(&s, &system, &user)?;
    if let Some(o) = v.as_object_mut() {
        for (k, val) in [("дата", t.created_at.clone()), ("участники", participants.join(", "))] {
            if o.get(k).and_then(Value::as_str).is_none_or(str::is_empty) {
                o.insert(k.into(), json!(val));
            }
        }
    }
    t.protocol = Some(v);
    store.save_transcript(t)?;
    Ok(())
}

// ---------- шаблоны и экспорт ----------

pub fn templates_dir(store: &Store) -> PathBuf {
    store.dir.join("templates")
}

pub fn ensure_templates(store: &Store) -> Result<()> {
    let dir = templates_dir(store);
    for (name, body) in [("протокол.docx", docx::default_protocol()), ("расшифровка.docx", docx::default_transcript())] {
        if !dir.join(name).exists() {
            docx::write_docx(&dir.join(name), &body)?;
        }
    }
    Ok(())
}

fn protocol_template(store: &Store, s: &Settings) -> PathBuf {
    pick_template(&s.protocol_template, templates_dir(store).join("протокол.docx"))
}

fn transcript_template(store: &Store, s: &Settings) -> PathBuf {
    pick_template(&s.transcript_template, templates_dir(store).join("расшифровка.docx"))
}

fn pick_template(custom: &str, default: PathBuf) -> PathBuf {
    if !custom.is_empty() && Path::new(custom).exists() {
        PathBuf::from(custom)
    } else {
        default
    }
}

/// Данные для шаблона расшифровки.
fn transcript_data(t: &Transcript) -> Map<String, Value> {
    // Подряд идущие реплики одного спикера — один абзац.
    let mut replicas: Vec<Value> = vec![];
    let mut prev = "";
    for u in &t.utterances {
        if u.speaker == prev {
            let r = replicas.last_mut().unwrap();
            for (k, v) in [("текст", &u.clean), ("дословно", &u.text)] {
                r[k] = json!(format!("{} {}", r[k].as_str().unwrap_or(""), v));
            }
            continue;
        }
        prev = &u.speaker;
        replicas.push(json!({
            "спикер": t.speaker_name(&u.speaker),
            "время": fmt_time(u.start),
            "текст": u.clean,
            "дословно": u.text,
        }));
    }
    let v = json!({
        "название": t.title,
        "дата": t.created_at,
        "длительность": fmt_time(t.duration),
        "участники": t.speakers.iter().map(|s| s.name.clone()).collect::<Vec<_>>().join(", "),
        "реплики": replicas,
    });
    v.as_object().unwrap().clone()
}

pub struct Exports {
    pub audio: PathBuf,
    pub transcript: PathBuf,
    pub protocol: Option<PathBuf>,
}

/// Собирает .docx в папке записи и, если задано правило, раскладывает файлы по папкам.
pub fn export(store: &Store, t: &Transcript, rule: Option<&Rule>) -> Result<Exports> {
    let s = store.settings();
    let dir = store.recording_dir(&t.id);
    let transcript = dir.join("transcript.docx");
    docx::render(&transcript_template(store, &s), &transcript_data(t), &transcript)?;
    let protocol = match &t.protocol {
        Some(Value::Object(p)) => {
            let out = dir.join("protocol.docx");
            docx::render(&protocol_template(store, &s), p, &out)?;
            Some(out)
        }
        _ => None,
    };
    let audio = dir.join("audio.ogg");
    let mut ex = Exports { audio, transcript, protocol };
    if let Some(r) = rule {
        let base = sanitize(&format!("{} {}", t.created_at.replace(':', "-"), t.title));
        let place = |src: &Path, to: &str, name: String| -> Result<PathBuf> {
            let dst = Path::new(to).join(name);
            std::fs::create_dir_all(to)?;
            std::fs::copy(src, &dst).with_context(|| format!("не удалось записать {}", dst.display()))?;
            // Свои же файлы не должны снова попасть в обработку.
            store.mark_processed(&dst, std::fs::metadata(&dst)?.len(), &t.id)?;
            Ok(dst)
        };
        if !r.audio_dir.is_empty() {
            ex.audio = place(&ex.audio, &r.audio_dir, format!("{base}.ogg"))?;
        }
        if !r.transcript_dir.is_empty() {
            ex.transcript = place(&ex.transcript, &r.transcript_dir, format!("{base} — расшифровка.docx"))?;
        }
        if let (Some(p), false) = (&ex.protocol, r.protocol_dir.is_empty()) {
            ex.protocol = Some(place(p, &r.protocol_dir, format!("{base} — протокол.docx"))?);
        }
    }
    Ok(ex)
}

fn sanitize(s: &str) -> String {
    s.chars().map(|c| if r#"/\:*?"<>|"#.contains(c) { '_' } else { c }).collect()
}

#[cfg(test)]
mod tests {
    use super::clean_fillers;

    #[test]
    fn removes_fillers() {
        assert_eq!(clean_fillers("Ну, э-э, я я думаю, что это, как бы, правильно."), "Я думаю, что это правильно.");
        assert_eq!(clean_fillers("Мы, короче, решили."), "Мы решили.");
        assert_eq!(clean_fillers("Готово. Ну, давайте начнём."), "Готово. Давайте начнём.");
    }
}
