//! Конвейер: Ingest → Enhance → ASR → Diarize → Identify → Terms → Enrich → Render/Export.
//! Текст расшифровки LLM не переписывает: она только пополняет справочники и составляет протокол.

use crate::diar::{DiarModel, Diarizer};
use crate::docx::{self, Field};
use crate::lang::{tr, Stage};
use crate::speech::{self, Engines, Segment, Turn};
use crate::store::{self, Rule, Settings, Store, Term};
use crate::transcript::{fmt_time, Speaker, Transcript, Utterance};
use crate::{audio, llm, markdown, media};
use anyhow::{Context, Result};
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

/// Что конвейер показывает, пока работает: текст появляется в окне, не дожидаясь конца обработки.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Live {
    /// Распознан фрагмент речи.
    Text { start: f32, text: String },
    /// Черновик расшифровки сохранён — его можно читать.
    Draft,
    /// Очередной кусок протокола — JSON, как его пишет LLM; `reset` — протокол начат заново.
    Protocol { text: String, reset: bool },
}

/// Куда конвейер сообщает о ходе работы.
pub trait Report {
    /// Этап и доля его выполнения (0 — доля неизвестна).
    fn stage(&self, stage: Stage, progress: f32);
    fn live(&self, live: Live);
}

/// Полная обработка файла. Возвращает расшифровку и предупреждения (некритичные сбои LLM).
pub fn transcribe(
    store: &Store,
    engines: &Engines,
    diarizer: &mut Diarizer,
    id: &str,
    input: &Path,
    report: &dyn Report,
) -> Result<(Transcript, Vec<String>)> {
    let settings = store.settings();
    let dir = store.recording_dir(id);
    std::fs::create_dir_all(&dir)?;
    let mut warnings = vec![];

    report.stage(Stage::Prepare, 0.0);
    let samples = media::decode(input)?;
    let duration = samples.len() as f32 / media::SAMPLE_RATE as f32;
    // Запись с микрофона после обработки хранится только в архиве: тогда он и есть источник.
    let archive = dir.join("audio.ogg");
    if !same_file(input, &archive) {
        media::encode_archive(input, &archive, settings.archive_kbps)?;
    }
    let prepared =
        audio::for_recognition(&samples, &settings, &store.models_dir(), &|f| report.stage(Stage::Denoise, f), &mut warnings);
    // Обработанный звук — для плеера; архив остаётся исходным. Без копии плеер играет архив.
    let clean = dir.join(audio::PLAYER_FILE);
    let saved = prepared.as_deref().map(|x| media::encode_samples(x, &clean, settings.archive_kbps));
    if let Some(Err(e)) = &saved {
        warnings.push(format!("{}: {e:#}", tr("Обработанный звук для плеера не сохранён", "The processed audio for the player was not saved")));
    }
    if !matches!(saved, Some(Ok(()))) {
        let _ = std::fs::remove_file(&clean);
    }

    report.stage(Stage::Recognize, 0.0);
    let regions = engines.speech_regions(prepared.as_deref().unwrap_or(&samples))?;
    drop(prepared);
    let segments = engines.recognize(&regions, |f, fresh| {
        for seg in fresh.iter().filter(|s| !s.words.is_empty()) {
            report.live(Live::Text { start: seg.start, text: capitalize(&seg.text()) });
        }
        report.stage(Stage::Recognize, f);
    });

    // Без разделения отрезков нет: все слова достаются одному спикеру, голоса не опознаются.
    let turns = if diarizer.model == DiarModel::Off {
        vec![]
    } else {
        report.stage(Stage::Diarize, 0.0);
        diarizer.diarize(&samples, settings.cluster_threshold, &mut |f| report.stage(Stage::Diarize, f))?
    };

    let (utterances, order) = build_utterances(&segments, &turns);
    // Название и дата — те, что уже видит пользователь: при повторной расшифровке они не меняются.
    let known = store.recording(id).ok();
    let language = match engines.language.as_str() {
        "" => detect_language(&utterances).into(),
        l => l.to_string(),
    };
    let mut t = Transcript {
        id: id.to_string(),
        title: known
            .as_ref()
            .map(|r| r.title.clone())
            .filter(|t| !t.is_empty())
            .unwrap_or_else(|| input.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()),
        source: input.to_string_lossy().into_owned(),
        created_at: known
            .map(|r| r.created_at)
            .filter(|d| !d.is_empty())
            .unwrap_or_else(|| chrono::Local::now().format("%Y-%m-%d %H:%M").to_string()),
        duration,
        asr_model: engines.asr.title().to_string(),
        diar_model: diarizer.model.title().to_string(),
        language,
        speakers: vec![],
        utterances,
        protocol: None,
    };

    report.stage(Stage::Identify, 0.0);
    identify(store, engines, &settings, &mut t, &samples, &turns, &order)?;

    report.stage(Stage::Terms, 0.0);
    let aliases = alias_rules(&store.terms()?);
    for u in &mut t.utterances {
        u.text = fix_terms(&u.text, &aliases);
    }
    // Текст — как распознан: LLM его не переписывает, только пополняет справочники.
    store.save_transcript(&t)?;
    report.live(Live::Draft);
    if settings.llm_enabled {
        report.stage(Stage::Enrich, 0.0);
        match enrich(store, &settings, &t) {
            Ok(title) if settings.auto_title && !title.is_empty() => t.title = title,
            Ok(_) => {}
            Err(e) => warnings.push(format!("{}: {e:#}", tr("Справочники не пополнены", "Glossary and people were not updated"))),
        }
    }
    // Пока шла расшифровка, пользователь мог дать записи своё название — оно главнее.
    if store.is_renamed(id) {
        if let Ok(r) = store.recording(id) {
            t.title = r.title;
        }
    }
    store.save_transcript(&t)?;
    Ok((t, warnings))
}

fn same_file(a: &Path, b: &Path) -> bool {
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

/// Язык записи, когда модель определяла его сама: по буквам текста. Нужен, чтобы выбрать язык
/// общения с LLM и шаблоны документов, поэтому различаем только «русский» и «другой».
fn detect_language(utterances: &[Utterance]) -> &'static str {
    let (mut cyrillic, mut other) = (0usize, 0usize);
    for c in utterances.iter().flat_map(|u| u.raw.chars()).filter(|c| c.is_alphabetic()) {
        if ('\u{0400}'..='\u{04FF}').contains(&c) {
            cyrillic += 1;
        } else {
            other += 1;
        }
    }
    if cyrillic >= other {
        "ru"
    } else {
        "other"
    }
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
        embeddings.push((format!("S{}", i + 1), speech::centroid(&acc)));
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
            name: person.map(|p| p.name.clone()).unwrap_or_else(|| speaker_label(i + 1, t.is_russian())),
            person_id: person.and_then(|p| p.id),
            similarity: hit.map(|(_, s)| s),
        });
    }
    Ok(())
}

/// Имя спикера, пока неизвестно, кто это: «Спикер 2» — на языке записи.
pub fn speaker_label(n: usize, russian: bool) -> String {
    format!("{} {n}", if russian { "Спикер" } else { "Speaker" })
}

// ---------- исправление и очистка ----------

/// Детерминированные замены «как слышится» → каноническое написание.
/// Непроверенные находки LLM текст не меняют.
fn alias_rules(terms: &[Term]) -> Vec<(Regex, String)> {
    terms
        .iter()
        .filter(|t| !t.pending)
        .filter_map(|term| {
            let variants: Vec<String> = std::iter::once(term.term.as_str())
                .chain(term.aliases.split([',', ';', '\n']))
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(regex::escape)
                .collect();
            Some((Regex::new(&format!(r"(?i)\b(?:{})\b", variants.join("|"))).ok()?, term.term.clone()))
        })
        .collect()
}

fn fix_terms(text: &str, rules: &[(Regex, String)]) -> String {
    rules.iter().fold(text.to_string(), |s, (re, term)| re.replace_all(&s, term.as_str()).into_owned())
}

pub fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().chain(c).collect(),
        None => String::new(),
    }
}

/// Справочники для LLM — только проверенные записи. `ru` — на каком языке идёт разговор с LLM.
fn knowledge_context(store: &Store, ru: bool) -> Result<String> {
    let l = |a: &'static str, b: &'static str| if ru { a } else { b };
    let mut ctx = String::new();
    let terms: Vec<Term> = store.terms()?.into_iter().filter(|t| !t.pending).collect();
    if !terms.is_empty() {
        ctx.push_str(l("Словарь (писать строго так):\n", "Glossary (spell exactly like this):\n"));
        for t in terms.iter().take(400) {
            ctx.push_str(&format!("- {}", t.term));
            if !t.aliases.is_empty() {
                ctx.push_str(&format!(" ({}: {})", l("может быть распознано как", "may be recognized as"), t.aliases));
            }
            if !t.definition.is_empty() {
                ctx.push_str(&format!(" — {}", t.definition));
            }
            ctx.push('\n');
        }
    }
    let people: Vec<_> = store.people()?.into_iter().filter(|p| !p.pending).collect();
    if !people.is_empty() {
        ctx.push_str(l(
            "\nЛюди (ФИО писать строго так, склоняя по правилам):\n",
            "\nPeople (spell the names exactly like this):\n",
        ));
        for p in people.iter().take(400) {
            ctx.push_str(&format!("- {}", p.name));
            if !p.aliases.is_empty() {
                ctx.push_str(&format!(" ({}: {})", l("обращения", "also called"), p.aliases));
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

// Запросы к LLM — на языке записи: русские для русских записей, английские для остальных
// (они просят отвечать на языке расшифровки).

const ENRICH_SCHEMA: &str = r#"{"type":"object","properties":{
"terms":{"type":"array","items":{"type":"object","properties":{"term":{"type":"string"},"definition":{"type":"string"}},"required":["term","definition"]}},
"people":{"type":"array","items":{"type":"object","properties":{"name":{"type":"string"},"role":{"type":"string"}},"required":["name","role"]}},
"title":{"type":"string"}},
"required":["terms","people","title"]}"#;

/// JSON Schema протокола из меток шаблона — в порядке их следования в документе.
fn protocol_schema(fields: &[Field]) -> String {
    let str_t = r#"{"type":"string"}"#;
    let props: Vec<String> = fields
        .iter()
        .map(|f| {
            let t = match f {
                Field::Text(_) => str_t.to_string(),
                Field::List(_) => format!(r#"{{"type":"array","items":{str_t}}}"#),
                Field::Table(_, cols) => {
                    let c: Vec<String> = cols.iter().map(|c| format!("{}:{str_t}", json!(c))).collect();
                    let req: Vec<String> = cols.iter().map(|c| json!(c).to_string()).collect();
                    format!(
                        r#"{{"type":"array","items":{{"type":"object","properties":{{{}}},"required":[{}]}}}}"#,
                        c.join(","),
                        req.join(",")
                    )
                }
            };
            format!("{}:{t}", json!(docx::name(f)))
        })
        .collect();
    let req: Vec<String> = fields.iter().map(|f| json!(docx::name(f)).to_string()).collect();
    format!(r#"{{"type":"object","properties":{{{}}},"required":[{}]}}"#, props.join(","), req.join(","))
}

const ENRICH_PROMPT_EN: &str = "You keep the glossary and the list of people for meeting transcripts. Find NEW entities in the text that are not among the already known ones:
- terms: special terms, abbreviations, names of organizations, projects, products and documents; definition is a short explanation from the context or an empty string. Do NOT suggest common words (request, budget, report, meeting, project, deadline) — only what a speech recognition model may misspell;
- people: people mentioned by their last name or full name; role is their position or role from the context;
- title: a short title of the recording by its content, 3 to 7 words, in the language of the text, like a document name (\"Q3 budget review\", \"Interview with a backend developer\"): no date, no quotes, no period at the end.
Use the base form of words. Do not make anything up. Answer strictly in JSON: {\"terms\": [{\"term\": \"\", \"definition\": \"\"}], \"people\": [{\"name\": \"\", \"role\": \"\"}], \"title\": \"\"}";

const ENRICH_PROMPT: &str = "Ты ведёшь справочники для расшифровки совещаний. Найди в тексте НОВЫЕ сущности, которых нет в уже известных:
- terms: специальные термины, аббревиатуры, названия организаций, проектов, продуктов, документов; definition — краткое пояснение из контекста или пустая строка. НЕ предлагай общеупотребительные слова (заявка, смета, отчёт, совещание, проект, срок, бюджет) — только то, что модель распознавания может написать неправильно;
- people: упомянутые люди с фамилией или полным именем; role — должность или роль из контекста;
- title: короткое название записи по её содержанию, 3–7 слов, как заголовок документа («Согласование сметы на ремонт», «Собеседование с бэкенд-разработчиком»): без даты, без кавычек и точки в конце.
Пиши в начальной форме (именительный падеж). Не выдумывай. Ответ строго в JSON: {\"terms\": [{\"term\": \"\", \"definition\": \"\"}], \"people\": [{\"name\": \"\", \"role\": \"\"}], \"title\": \"\"}";

/// Новые термины и люди сразу попадают в справочники — на проверку пользователю
/// (или без неё, если так настроено). Возвращает название записи по содержанию (может быть пустым).
fn enrich(store: &Store, s: &Settings, t: &Transcript) -> Result<String> {
    let known_terms: Vec<String> = store.terms()?.into_iter().map(|x| x.term.to_lowercase()).collect();
    let known_people: Vec<String> = store.people()?.into_iter().map(|x| x.name.to_lowercase()).collect();
    let text: String = t.as_dialogue(false).chars().take(llm::prompt_budget(s).min(30_000)).collect();
    let (system, terms, people, body) = if t.is_russian() {
        (ENRICH_PROMPT, "Уже известные термины", "Уже известные люди", "Текст")
    } else {
        (ENRICH_PROMPT_EN, "Already known terms", "Already known people", "Text")
    };
    let prompt = format!("{terms}: {}\n{people}: {}\n\n{body}:\n{text}", known_terms.join(", "), known_people.join(", "));
    let resp = llm::chat_json(s, system, &prompt, ENRICH_SCHEMA)?;
    let pick = |v: &Value, k: &str| v[k].as_str().unwrap_or("").trim().to_string();
    let pending = !s.auto_accept_suggestions;
    // Уже известное — это и справочник, и всё, что LLM находила раньше: удалённое пользователем не возвращается.
    let seen = |known: &[String], kind: &str| -> Result<HashSet<String>> {
        Ok(known.iter().map(|k| store::match_key(k)).chain(store.proposed(kind)?.iter().map(|v| store::match_key(v))).collect())
    };
    let mut seen_terms = seen(&known_terms, "term")?;
    for x in resp["terms"].as_array().into_iter().flatten() {
        let term = pick(x, "term");
        if !term.is_empty() && seen_terms.insert(store::match_key(&term)) {
            store.propose("term", &term, &pick(x, "definition"), &t.id, pending)?;
        }
    }
    let mut seen_people = seen(&known_people, "person")?;
    for x in resp["people"].as_array().into_iter().flatten() {
        let name = pick(x, "name");
        if !name.is_empty() && seen_people.insert(store::match_key(&name)) {
            store.propose("person", &name, &pick(x, "role"), &t.id, pending)?;
        }
    }
    Ok(clean_title(&pick(&resp, "title")))
}

/// Название от LLM — одной строкой, без кавычек, точки в конце и лишней длины.
fn clean_title(raw: &str) -> String {
    let line = raw.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("");
    let quotes: &[char] = &['"', '\'', '«', '»', '“', '”', '„', '`'];
    let title = line.trim_end_matches(['.', ';', ':']).trim_matches(quotes).trim_end_matches(['.', ';', ':']).trim();
    let title: String = title.chars().take(100).collect();
    capitalize(title.trim())
}

// ---------- протокол ----------

/// Составляет протокол по шаблону; пока LLM пишет, текст уходит в `report` по кускам.
pub fn make_protocol(store: &Store, t: &mut Transcript, report: &dyn Report) -> Result<()> {
    let s = store.settings();
    anyhow::ensure!(
        s.llm_enabled,
        tr(
            "протокол составляет ИИ-помощник — включите его в настройках",
            "minutes are written by the AI assistant — turn it on in Settings"
        )
    );
    let ru = t.is_russian();
    let l = |a: &'static str, b: &'static str| if ru { a } else { b };
    let fields = docx::fields(&protocol_template(store, &s, ru))?;
    let string = l("строка", "string");
    let schema: Vec<String> = fields
        .iter()
        .map(|f| match f {
            Field::Text(n) => format!("\"{n}\": {string}"),
            Field::List(n) => format!("\"{n}\": {}", l("массив строк", "array of strings")),
            Field::Table(n, cols) => format!(
                "\"{n}\": {} {{{}}}",
                l("массив объектов", "array of objects"),
                cols.iter().map(|c| format!("\"{c}\": {string}")).collect::<Vec<_>>().join(", ")
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
        "{}\n{}",
        l(
            "Ты секретарь. Составь сжатый протокол договорённостей по расшифровке совещания: деловой стиль, \
             только факты из текста, без выдумок. Решения и поручения формулируй конкретно. \
             Если данных для поля нет — пустая строка или пустой массив.\n\
             Верни строго JSON с ключами:",
            "You are a secretary. Write concise minutes of the meeting from its transcript: business style, \
             only facts from the text, nothing invented. State decisions and action items specifically. \
             If there is no data for a field, use an empty string or an empty array. \
             Write in the language of the transcript.\n\
             Return strictly JSON with the keys:"
        ),
        schema.join("\n")
    );
    let user = format!(
        "{}: {}\n{}: {}\n\n{}\n\n{}:\n{}",
        l("Дата записи", "Recording date"),
        t.created_at,
        l("Участники", "Participants"),
        participants.join(", "),
        knowledge_context(store, ru)?,
        l("Расшифровка", "Transcript"),
        fit_to_budget(&s, t.as_dialogue(true), ru, report)?
    );
    report.stage(Stage::Protocol, 0.0);
    report.live(Live::Protocol { text: String::new(), reset: true });
    let mut v = llm::chat_json_live(&s, &system, &user, &protocol_schema(&fields), &|piece| {
        report.live(Live::Protocol { text: piece.into(), reset: false })
    })?;
    if let Some(o) = v.as_object_mut() {
        // Дату и участников знаем точно — в этих полях шаблона догадки LLM не нужны.
        for (keys, val) in [(["дата", "date"], protocol_date(&t.created_at, ru)), (["участники", "participants"], participants.join(", "))] {
            for k in keys {
                if matches!(fields.iter().find(|f| docx::name(f) == k), Some(Field::Text(_))) {
                    o.insert(k.into(), json!(val));
                }
            }
        }
    }
    t.protocol = Some(v);
    store.save_transcript(t)?;
    Ok(())
}

/// Дата записи для протокола: «28.09.2026» в русском документе, «2026-09-28» в остальных.
fn protocol_date(created_at: &str, ru: bool) -> String {
    let date = created_at.split_whitespace().next().unwrap_or(created_at);
    match (ru, date.split('-').collect::<Vec<_>>().as_slice()) {
        (true, [y, m, d]) => format!("{d}.{m}.{y}"),
        _ => date.to_string(),
    }
}

const CONDENSE_PROMPT: &str = "Сожми фрагмент расшифровки совещания в подробный конспект: кто что предлагал, \
все решения, поручения (кто, что, срок), цифры и даты, открытые вопросы. Сохраняй имена говорящих. \
Ничего не выдумывай. Ответ строго в JSON: {\"notes\": \"...\"}";

const CONDENSE_PROMPT_EN: &str = "Condense this fragment of a meeting transcript into detailed notes: who proposed what, \
all decisions, action items (who, what, by when), figures and dates, open questions. Keep the speakers' names. \
Do not make anything up. Write in the language of the transcript. Answer strictly in JSON: {\"notes\": \"...\"}";

/// Длинное совещание не помещается в контекст небольшой модели: сначала
/// конспектируем его по частям, протокол составляем по конспекту.
fn fit_to_budget(s: &Settings, text: String, ru: bool, report: &dyn Report) -> Result<String> {
    let budget = llm::prompt_budget(s);
    if text.chars().count() <= budget {
        return Ok(text);
    }
    let mut chunks = vec![String::new()];
    for line in text.lines() {
        if chunks.last().unwrap().chars().count() + line.chars().count() > budget / 2 {
            chunks.push(String::new());
        }
        let c = chunks.last_mut().unwrap();
        c.push_str(line);
        c.push('\n');
    }
    let (prompt, part) = if ru { (CONDENSE_PROMPT, "Часть") } else { (CONDENSE_PROMPT_EN, "Part") };
    let mut notes = vec![];
    for (i, c) in chunks.iter().enumerate() {
        report.stage(Stage::Condense, i as f32 / chunks.len() as f32);
        let v = llm::chat_json(s, prompt, c, r#"{"type":"object","properties":{"notes":{"type":"string"}},"required":["notes"]}"#)?;
        notes.push(format!("{part} {}:\n{}", i + 1, v["notes"].as_str().unwrap_or_default()));
    }
    Ok(notes.join("\n\n"))
}

// ---------- шаблоны и выгрузка ----------

pub fn templates_dir(store: &Store) -> PathBuf {
    store.dir.join("templates")
}

/// Шаблоны по умолчанию — русские и английские: какой взять, решает язык записи.
pub fn ensure_templates(store: &Store) -> Result<()> {
    let dir = templates_dir(store);
    for (name, ru) in [("протокол.docx", true), ("расшифровка.docx", true), ("minutes.docx", false), ("transcript.docx", false)] {
        if !dir.join(name).exists() {
            let protocol = name.starts_with("протокол") || name.starts_with("minutes");
            let body = if protocol { docx::default_protocol(ru) } else { docx::default_transcript(ru) };
            docx::write_docx(&dir.join(name), &body, ru)?;
        }
    }
    Ok(())
}

fn protocol_template(store: &Store, s: &Settings, ru: bool) -> PathBuf {
    pick_template(&s.protocol_template, templates_dir(store).join(if ru { "протокол.docx" } else { "minutes.docx" }))
}

fn transcript_template(store: &Store, s: &Settings, ru: bool) -> PathBuf {
    pick_template(&s.transcript_template, templates_dir(store).join(if ru { "расшифровка.docx" } else { "transcript.docx" }))
}

fn pick_template(custom: &str, default: PathBuf) -> PathBuf {
    if !custom.is_empty() && Path::new(custom).exists() {
        PathBuf::from(custom)
    } else {
        default
    }
}

/// Данные для шаблона расшифровки; метки — и русские, и английские. `verbatim` — в поле
/// «текст» идёт дословная расшифровка вместо очищенной.
fn transcript_data(t: &Transcript, verbatim: bool) -> Map<String, Value> {
    // Подряд идущие реплики одного спикера — один абзац.
    let mut replicas: Vec<Value> = vec![];
    let mut prev = "";
    for u in &t.utterances {
        let text = if verbatim || u.clean.is_empty() { &u.text } else { &u.clean };
        if u.speaker == prev {
            let r = replicas.last_mut().unwrap();
            for (keys, v) in [(["текст", "text"], text), (["дословно", "verbatim"], &u.text)] {
                for k in keys {
                    r[k] = json!(format!("{} {}", r[k].as_str().unwrap_or(""), v));
                }
            }
            continue;
        }
        prev = &u.speaker;
        let (speaker, time) = (t.speaker_name(&u.speaker), fmt_time(u.start));
        replicas.push(json!({
            "спикер": speaker, "speaker": speaker,
            "время": time, "time": time,
            "текст": text, "text": text,
            "дословно": u.text, "verbatim": u.text,
        }));
    }
    let participants = t.speakers.iter().map(|s| s.name.clone()).collect::<Vec<_>>().join(", ");
    let duration = fmt_time(t.duration);
    let v = json!({
        "название": t.title, "title": t.title,
        "дата": t.created_at, "date": t.created_at,
        "длительность": duration, "duration": duration,
        "участники": participants, "participants": participants,
        "реплики": replicas, "utterances": replicas,
    });
    v.as_object().unwrap().clone()
}

/// Какой документ записи.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Doc {
    Transcript,
    Protocol,
}

/// Формат выгрузки: Markdown — основной, Word собирается из тех же данных по шаблону.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Format {
    Md,
    Docx,
}

impl Doc {
    fn stem(self) -> &'static str {
        match self {
            Self::Transcript => "transcript",
            Self::Protocol => "protocol",
        }
    }

    /// Для имени файла: «… — расшифровка.md».
    fn label(self, ru: bool) -> &'static str {
        match (self, ru) {
            (Self::Transcript, true) => "расшифровка",
            (Self::Transcript, false) => "transcript",
            (Self::Protocol, true) => "протокол",
            (Self::Protocol, false) => "minutes",
        }
    }
}

/// Текст документа в Markdown.
pub fn markdown(store: &Store, t: &Transcript, doc: Doc, verbatim: bool) -> Result<String> {
    match doc {
        Doc::Transcript => Ok(markdown::transcript(t, verbatim)),
        Doc::Protocol => {
            // Поля — в порядке меток шаблона.
            let order: Vec<String> = docx::fields(&protocol_template(store, &store.settings(), t.is_russian()))
                .map(|fields| fields.iter().map(|f| docx::name(f).to_string()).collect())
                .unwrap_or_default();
            markdown::protocol(t, &order)
                .with_context(|| tr("протокол ещё не составлен", "the minutes have not been written yet"))
        }
    }
}

/// Записывает документ в файл: туда, куда указал пользователь, или (`to` = None) в папку записи.
pub fn write_doc(store: &Store, t: &Transcript, doc: Doc, format: Format, verbatim: bool, to: Option<&Path>) -> Result<PathBuf> {
    let ext = if format == Format::Md { "md" } else { "docx" };
    let out = to.map(Path::to_path_buf).unwrap_or_else(|| {
        let mark = if verbatim && doc == Doc::Transcript { "-verbatim" } else { "" };
        store.recording_dir(&t.id).join(format!("{}{mark}.{ext}", doc.stem()))
    });
    if let Some(dir) = out.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let saved = || format!("{} {}", tr("не удалось записать", "could not write"), out.display());
    match (format, doc) {
        (Format::Md, _) => std::fs::write(&out, markdown(store, t, doc, verbatim)?).with_context(saved)?,
        (Format::Docx, Doc::Transcript) => {
            let template = transcript_template(store, &store.settings(), t.is_russian());
            docx::render(&template, &transcript_data(t, verbatim), &out).with_context(saved)?
        }
        (Format::Docx, Doc::Protocol) => {
            let Some(Value::Object(fields)) = &t.protocol else {
                anyhow::bail!(tr("протокол ещё не составлен", "the minutes have not been written yet"));
            };
            let template = protocol_template(store, &store.settings(), t.is_russian());
            docx::render(&template, fields, &out).with_context(saved)?
        }
    }
    Ok(out)
}

pub struct Exports {
    pub audio: PathBuf,
    pub transcript: PathBuf,
    pub protocol: Option<PathBuf>,
}

/// Записывает расшифровку и протокол в Markdown в папке записи и, если задано правило,
/// раскладывает файлы по папкам (документы Word — если это включено в правиле).
pub fn export(store: &Store, t: &Transcript, rule: Option<&Rule>) -> Result<Exports> {
    let dir = store.recording_dir(&t.id);
    let transcript = write_doc(store, t, Doc::Transcript, Format::Md, false, None)?;
    let protocol = match &t.protocol {
        Some(Value::Object(_)) => Some(write_doc(store, t, Doc::Protocol, Format::Md, false, None)?),
        _ => {
            let _ = std::fs::remove_file(dir.join("protocol.md"));
            None
        }
    };
    // Документы, собранные по запросу до правки, устарели — при следующем запросе соберутся заново.
    for stale in ["transcript.docx", "protocol.docx", "transcript-verbatim.md", "transcript-verbatim.docx"] {
        let _ = std::fs::remove_file(dir.join(stale));
    }
    let mut ex = Exports { audio: dir.join("audio.ogg"), transcript, protocol };
    let Some(r) = rule else { return Ok(ex) };

    let base = sanitize(&format!("{} {}", t.created_at.replace(':', "-"), t.title));
    let place = |src: &Path, to: &str, name: String| -> Result<PathBuf> {
        let dst = Path::new(to).join(name);
        std::fs::create_dir_all(to)?;
        std::fs::copy(src, &dst)
            .with_context(|| format!("{} {}", tr("не удалось записать", "could not write"), dst.display()))?;
        // Свои же файлы не должны снова попасть в обработку.
        store.mark_processed(&dst, std::fs::metadata(&dst)?.len(), &t.id)?;
        Ok(dst)
    };
    if !r.audio_dir.is_empty() {
        ex.audio = place(&ex.audio, &r.audio_dir, format!("{base}.ogg"))?;
    }
    let docs = [(Doc::Transcript, &r.transcript_dir, Some(ex.transcript.clone())), (Doc::Protocol, &r.protocol_dir, ex.protocol.clone())];
    for (doc, to, src) in docs {
        let (Some(src), false) = (src, to.is_empty()) else { continue };
        let name = format!("{base} — {}", doc.label(t.is_russian()));
        let placed = place(&src, to, format!("{name}.md"))?;
        if r.docx {
            let word = write_doc(store, t, doc, Format::Docx, false, None)?;
            place(&word, to, format!("{name}.docx"))?;
            let _ = std::fs::remove_file(word);
        }
        match doc {
            Doc::Transcript => ex.transcript = placed,
            Doc::Protocol => ex.protocol = Some(placed),
        }
    }
    Ok(ex)
}

fn sanitize(s: &str) -> String {
    s.chars().map(|c| if r#"/\:*?"<>|"#.contains(c) { '_' } else { c }).collect()
}

/// Ищет текст в расшифровках. Возвращает записи с фрагментом вокруг первого совпадения.
pub fn search(store: &Store, query: &str) -> Result<Vec<Hit>> {
    let needle = store::match_key(query);
    let mut out = vec![];
    if needle.chars().count() < 2 {
        return Ok(out);
    }
    for r in store.recordings()? {
        let Ok(t) = store.load_transcript(&r.id) else { continue };
        let found = t.utterances.iter().find_map(|u| {
            [&u.clean, &u.text].into_iter().find_map(|text| snippet(text, &needle))
        });
        if let Some(snippet) = found {
            out.push(Hit { id: r.id, snippet });
        }
    }
    Ok(out)
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Hit {
    pub id: String,
    pub snippet: String,
}

/// Несколько слов вокруг совпадения; регистр и «е/ё» не важны.
fn snippet(text: &str, needle: &str) -> Option<String> {
    let chars: Vec<char> = text.chars().collect();
    // Сравниваем посимвольно, чтобы позиция в приведённой строке совпадала с позицией в исходной.
    let fold = |c: char| c.to_lowercase().next().map_or(c, |l| if l == 'ё' { 'е' } else { l });
    let hay: Vec<char> = chars.iter().copied().map(fold).collect();
    let pat: Vec<char> = needle.chars().map(fold).collect();
    let at = hay.windows(pat.len()).position(|w| w == pat.as_slice())?;
    let mut a = at.saturating_sub(24);
    let mut b = (at + pat.len() + 48).min(chars.len());
    // Не рвём слова по краям фрагмента.
    while a > 0 && a < at && !chars[a - 1].is_whitespace() {
        a += 1;
    }
    while b < chars.len() && b > at + pat.len() && !chars[b].is_whitespace() {
        b -= 1;
    }
    Some(chars[a..b].iter().collect::<String>().trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn title_from_llm_is_one_clean_line() {
        assert_eq!(clean_title("  «согласование сметы на ремонт».\nещё строка"), "Согласование сметы на ремонт");
        assert_eq!(clean_title("\"Q3 budget review\""), "Q3 budget review");
        assert_eq!(clean_title(""), "");
    }

    #[test]
    fn protocol_date_follows_the_language() {
        assert_eq!(protocol_date("2026-09-28 21:05", true), "28.09.2026");
        assert_eq!(protocol_date("2026-09-28 21:05", false), "2026-09-28");
        assert_eq!(protocol_date("вчера", true), "вчера");
    }

    #[test]
    fn detects_russian_by_letters() {
        let u = |raw: &str| Utterance { raw: raw.into(), ..Default::default() };
        assert_eq!(detect_language(&[u("добрый день"), u("ok")]), "ru");
        assert_eq!(detect_language(&[u("good morning everyone"), u("да")]), "other");
        assert_eq!(detect_language(&[]), "ru");
    }

    #[test]
    fn snippet_shows_words_around_the_match() {
        let text = "Коллеги, по заявке в Минпромторг: её нужно подать до десятого октября, иначе не успеем.";
        assert_eq!(snippet(text, "минпромторг").unwrap(), "Коллеги, по заявке в Минпромторг: её нужно подать до десятого октября, иначе не");
        assert_eq!(snippet(text, "не успеем").unwrap(), "десятого октября, иначе не успеем.");
        assert_eq!(snippet("Всё готово", "все").unwrap(), "Всё готово");
        assert_eq!(snippet(text, "смета"), None);
    }

    #[test]
    fn template_data_has_labels_in_both_languages() {
        let t = Transcript {
            title: "Планёрка".into(),
            speakers: vec![Speaker { id: "S1".into(), name: "Петров".into(), ..Default::default() }],
            utterances: vec![
                Utterance { speaker: "S1".into(), text: "Ну, начнём.".into(), clean: "Начнём.".into(), ..Default::default() },
                Utterance { speaker: "S1".into(), start: 4.0, text: "Смета готова.".into(), clean: "Смета готова.".into(), ..Default::default() },
            ],
            ..Default::default()
        };
        let d = transcript_data(&t, false);
        assert_eq!(d["название"], d["title"]);
        assert_eq!(d["реплики"][0]["текст"], "Начнём. Смета готова.");
        assert_eq!(d["utterances"][0]["verbatim"], "Ну, начнём. Смета готова.");
        assert_eq!(transcript_data(&t, true)["реплики"][0]["текст"], "Ну, начнём. Смета готова.");
    }
}
