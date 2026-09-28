//! Локальное хранилище: библиотека, словарь, «кто есть кто» (с находками LLM на проверке),
//! голосовые отпечатки (зашифрованы), правила папок, настройки.

use crate::diar::DiarModel;
use crate::lang::{tr, Language};
use crate::speech::AsrModel;
use crate::transcript::Transcript;
use aes_gcm::aead::{Aead, AeadCore, KeyInit, OsRng};
use aes_gcm::{Aes256Gcm, Key, Nonce};
use anyhow::{anyhow, Context, Result};
use base64::Engine as _;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

pub const APP_ID: &str = "app.ultratranscript";

/// Библиотека, справочники и настройки. `UT_DATA_DIR` — отдельная библиотека
/// (CLI и тесты не должны трогать данные приложения).
pub fn data_dir() -> PathBuf {
    std::env::var_os("UT_DATA_DIR").map(PathBuf::from).unwrap_or_else(app_dir)
}

fn app_dir() -> PathBuf {
    dirs::data_dir().expect("нет каталога данных").join(APP_ID)
}

/// Модели общие для всех библиотек — большие файлы не дублируются.
pub fn models_dir() -> PathBuf {
    app_dir().join("models")
}

/// Тема оформления.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    /// Как в системе.
    #[default]
    System,
    Light,
    Dark,
}

/// Где работает LLM: внутри приложения или на внешнем сервере (Ollama, LM Studio, API).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LlmProvider {
    #[default]
    Builtin,
    Api,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub asr_model: AsrModel,
    /// Подавлять шум перед распознаванием.
    pub denoise: bool,
    /// Выравнивать громкость голосов перед распознаванием.
    pub level_volume: bool,
    pub llm_enabled: bool,
    pub llm_provider: LlmProvider,
    /// Встроенная модель — имя из каталога моделей.
    pub llm_local_model: String,
    pub llm_base_url: String,
    pub llm_model: String,
    /// Не сериализуется в БД — хранится в системной связке ключей.
    pub llm_api_key: String,
    /// Движок диаризации.
    pub diar_model: DiarModel,
    /// Порог кластеризации pyannote 3.0.
    pub cluster_threshold: f32,
    pub voice_threshold: f32,
    pub archive_kbps: u32,
    pub auto_accept_suggestions: bool,
    pub transcript_template: String,
    pub protocol_template: String,
    pub theme: Theme,
    /// Язык интерфейса.
    pub language: Language,
    /// Язык записей для Whisper: код языка ("ru", "en"…) или "auto" — определять самому.
    pub speech_language: String,
    /// Уведомлять о начале и конце расшифровки, когда окно не на экране.
    pub notifications: bool,
    /// Показывать раздел настроек с параметрами для отладки.
    pub developer_mode: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            asr_model: AsrModel::default(),
            denoise: true,
            level_volume: true,
            llm_enabled: false,
            llm_provider: LlmProvider::Builtin,
            llm_local_model: "gigachat-lightning".into(),
            llm_base_url: "http://localhost:11434/v1".into(),
            llm_model: "qwen3:8b".into(),
            llm_api_key: String::new(),
            diar_model: DiarModel::default(),
            cluster_threshold: 0.4,
            voice_threshold: 0.55,
            archive_kbps: 24,
            auto_accept_suggestions: false,
            transcript_template: String::new(),
            protocol_template: String::new(),
            theme: Theme::default(),
            language: Language::default(),
            speech_language: "auto".into(),
            notifications: true,
            developer_mode: false,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Term {
    pub id: Option<i64>,
    pub term: String,
    /// Как слышится / частые ошибки распознавания, через запятую.
    pub aliases: String,
    pub definition: String,
    /// Находка LLM, которую пользователь ещё не подтвердил: в расшифровке не применяется.
    #[serde(default)]
    pub pending: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Person {
    pub id: Option<i64>,
    pub name: String,
    pub aliases: String,
    pub role: String,
    pub org: String,
    #[serde(default)]
    pub voiceprints: i64,
    /// Находка LLM, которую пользователь ещё не подтвердил.
    #[serde(default)]
    pub pending: bool,
}

/// Сколько находок LLM ждут проверки в каждом справочнике.
#[derive(Clone, Copy, Debug, Default, Serialize)]
pub struct Pending {
    pub terms: i64,
    pub people: i64,
}

/// Голосовой образец человека: спикер из записи или загруженный файл.
#[derive(Clone, Debug)]
pub struct Voice {
    pub id: i64,
    /// Пусто — образец загружен из файла.
    pub recording_id: String,
    /// Спикер записи ("S2") или, для файла, имя копии в `voices/`.
    pub speaker: String,
    /// Имя загруженного файла.
    pub label: String,
    /// Длительность копии загруженного файла.
    pub seconds: f32,
    pub created_at: String,
}

/// Ключ для сравнения имён и терминов: без регистра, пробелов по краям и различия «е/ё».
pub fn match_key(s: &str) -> String {
    s.trim().to_lowercase().replace('ё', "е")
}

/// Значение из файла заменяет текущее, только если оно не пустое.
fn fill(cur: &str, new: &str) -> String {
    let new = new.trim();
    if new.is_empty() { cur } else { new }.to_string()
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Rule {
    pub id: Option<i64>,
    pub input_dir: String,
    pub audio_dir: String,
    pub transcript_dir: String,
    pub protocol_dir: String,
    pub protocol: bool,
    /// Рядом с Markdown сохранять документы Word.
    #[serde(default)]
    pub docx: bool,
    pub enabled: bool,
}

/// Папка библиотеки: записи раскладывает по ним пользователь.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Folder {
    pub id: i64,
    pub name: String,
}

/// Сводка для главного экрана.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize)]
pub struct Stats {
    pub recordings: i64,
    /// Общая длительность записей, секунды.
    pub seconds: f64,
    pub terms: i64,
    pub people: i64,
    /// Люди с голосовым профилем.
    pub voices: i64,
    /// Отслеживаемые папки.
    pub rules: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Recording {
    pub id: String,
    pub title: String,
    pub source: String,
    pub created_at: String,
    pub duration: f32,
    pub status: String,
    pub error: String,
    pub rule_id: Option<i64>,
    /// Папка библиотеки.
    #[serde(default)]
    pub folder_id: Option<i64>,
    #[serde(default)]
    pub archived: bool,
    /// Есть ли протокол (по файлам записи; в базе не хранится).
    #[serde(default)]
    pub has_protocol: bool,
}

pub struct Store {
    pub dir: PathBuf,
    db: Mutex<Connection>,
    cipher: Aes256Gcm,
}

const SCHEMA: &str = "
PRAGMA foreign_keys = ON;
CREATE TABLE IF NOT EXISTS recordings(id TEXT PRIMARY KEY, title TEXT, source TEXT, created_at TEXT,
  duration REAL DEFAULT 0, status TEXT, error TEXT DEFAULT '', rule_id INTEGER);
CREATE TABLE IF NOT EXISTS terms(id INTEGER PRIMARY KEY, term TEXT NOT NULL, aliases TEXT DEFAULT '',
  definition TEXT DEFAULT '');
CREATE TABLE IF NOT EXISTS people(id INTEGER PRIMARY KEY, name TEXT NOT NULL, aliases TEXT DEFAULT '',
  role TEXT DEFAULT '', org TEXT DEFAULT '');
CREATE TABLE IF NOT EXISTS voiceprints(id INTEGER PRIMARY KEY,
  person_id INTEGER REFERENCES people(id) ON DELETE CASCADE,
  recording_id TEXT NOT NULL, speaker TEXT NOT NULL, embedding BLOB NOT NULL, created_at TEXT,
  UNIQUE(recording_id, speaker));
CREATE TABLE IF NOT EXISTS suggestions(id INTEGER PRIMARY KEY, kind TEXT, value TEXT, detail TEXT,
  recording_id TEXT, status TEXT DEFAULT 'new', UNIQUE(kind, value));
CREATE TABLE IF NOT EXISTS rules(id INTEGER PRIMARY KEY, input_dir TEXT, audio_dir TEXT,
  transcript_dir TEXT, protocol_dir TEXT, protocol INTEGER, enabled INTEGER);
CREATE TABLE IF NOT EXISTS processed(path TEXT PRIMARY KEY, size INTEGER, recording_id TEXT);
CREATE TABLE IF NOT EXISTS settings(key TEXT PRIMARY KEY, value TEXT);
";

/// Изменения схемы поверх SCHEMA; номер применённого — в `PRAGMA user_version`.
const MIGRATIONS: &[&str] = &[
    // 1. Находки LLM сразу попадают в справочники с пометкой «на проверке», а `suggestions`
    //    остаётся журналом находок (status больше не нужен): удалённое повторно не предлагается.
    //    Образцы голоса из файлов: имя файла и длительность копии.
    "ALTER TABLE terms ADD COLUMN pending INTEGER NOT NULL DEFAULT 0;
     ALTER TABLE people ADD COLUMN pending INTEGER NOT NULL DEFAULT 0;
     ALTER TABLE voiceprints ADD COLUMN label TEXT NOT NULL DEFAULT '';
     ALTER TABLE voiceprints ADD COLUMN seconds REAL NOT NULL DEFAULT 0;
     INSERT INTO terms(term, definition, pending)
       SELECT value, detail, 1 FROM suggestions s WHERE kind = 'term' AND status = 'new'
         AND NOT EXISTS (SELECT 1 FROM terms t WHERE t.term = s.value COLLATE NOCASE);
     INSERT INTO people(name, role, pending)
       SELECT value, detail, 1 FROM suggestions s WHERE kind = 'person' AND status = 'new'
         AND NOT EXISTS (SELECT 1 FROM people p WHERE p.name = s.value COLLATE NOCASE);",
    // 2. Папки библиотеки и архив. Документы Word по правилу папки: прежние правила их сохраняли.
    "CREATE TABLE folders(id INTEGER PRIMARY KEY, name TEXT NOT NULL);
     ALTER TABLE recordings ADD COLUMN folder_id INTEGER;
     ALTER TABLE recordings ADD COLUMN archived INTEGER NOT NULL DEFAULT 0;
     ALTER TABLE rules ADD COLUMN docx INTEGER NOT NULL DEFAULT 0;
     UPDATE rules SET docx = 1;",
];

fn migrate(db: &mut Connection) -> Result<()> {
    let done: i64 = db.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    for (i, sql) in MIGRATIONS.iter().enumerate().skip(done as usize) {
        let tx = db.transaction()?;
        tx.execute_batch(sql)?;
        tx.pragma_update(None, "user_version", i as i64 + 1)?;
        tx.commit()?;
    }
    Ok(())
}

impl Store {
    pub fn open() -> Result<Self> {
        let dir = data_dir();
        std::fs::create_dir_all(dir.join("recordings"))?;
        let mut db = Connection::open(dir.join("library.db"))?;
        db.execute_batch(SCHEMA)?;
        migrate(&mut db)?;
        let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&vault_key(&dir)?));
        Ok(Self { dir, db: Mutex::new(db), cipher })
    }

    fn db(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.db.lock().unwrap()
    }

    pub fn models_dir(&self) -> PathBuf {
        models_dir()
    }

    pub fn recording_dir(&self, id: &str) -> PathBuf {
        self.dir.join("recordings").join(id)
    }

    // ---------- настройки ----------

    pub fn settings(&self) -> Settings {
        let raw: Option<String> = self
            .db()
            .query_row("SELECT value FROM settings WHERE key='settings'", [], |r| r.get(0))
            .optional()
            .ok()
            .flatten();
        let mut s: Settings = raw.and_then(|r| serde_json::from_str(&r).ok()).unwrap_or_default();
        s.llm_api_key = secret_get("llm_api_key").unwrap_or_default();
        s
    }

    pub fn save_settings(&self, s: &Settings) -> Result<()> {
        secret_set("llm_api_key", &s.llm_api_key)?;
        let mut stored = s.clone();
        stored.llm_api_key.clear();
        self.db().execute(
            "INSERT OR REPLACE INTO settings(key, value) VALUES('settings', ?1)",
            [serde_json::to_string(&stored)?],
        )?;
        Ok(())
    }

    // ---------- библиотека ----------

    pub fn upsert_recording(&self, r: &Recording) -> Result<()> {
        self.db().execute(
            "INSERT INTO recordings(id,title,source,created_at,duration,status,error,rule_id)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8)
             ON CONFLICT(id) DO UPDATE SET title=?2, duration=?5, status=?6, error=?7",
            params![r.id, r.title, r.source, r.created_at, r.duration, r.status, r.error, r.rule_id],
        )?;
        Ok(())
    }

    pub fn set_status(&self, id: &str, status: &str, error: &str) -> Result<()> {
        self.db().execute(
            "UPDATE recordings SET status=?2, error=?3 WHERE id=?1",
            params![id, status, error],
        )?;
        Ok(())
    }

    pub fn recordings(&self) -> Result<Vec<Recording>> {
        let db = self.db();
        let mut st = db.prepare(
            "SELECT id,title,source,created_at,duration,status,error,rule_id,folder_id,archived FROM recordings
             ORDER BY created_at DESC, id DESC",
        )?;
        let rows = st.query_map([], |r| {
            let id: String = r.get(0)?;
            Ok(Recording {
                has_protocol: self.has_protocol(&id),
                id,
                title: r.get(1)?,
                source: r.get(2)?,
                created_at: r.get(3)?,
                duration: r.get(4)?,
                status: r.get(5)?,
                error: r.get(6)?,
                rule_id: r.get(7)?,
                folder_id: r.get(8)?,
                archived: r.get(9)?,
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    pub fn recording(&self, id: &str) -> Result<Recording> {
        self.recordings()?
            .into_iter()
            .find(|r| r.id == id)
            .ok_or_else(|| anyhow!(tr("запись не найдена", "recording not found")))
    }

    /// Протокол записан в файл при выгрузке; у записей прежних версий это документ Word.
    fn has_protocol(&self, id: &str) -> bool {
        let dir = self.recording_dir(id);
        dir.join("protocol.md").exists() || dir.join("protocol.docx").exists()
    }

    /// Новое название — и в библиотеке, и в расшифровке.
    pub fn rename_recording(&self, id: &str, title: &str) -> Result<()> {
        let title = title.trim();
        anyhow::ensure!(!title.is_empty(), tr("название не может быть пустым", "the title cannot be empty"));
        match self.load_transcript(id) {
            Ok(mut t) => {
                t.title = title.into();
                self.save_transcript(&t)?;
            }
            // Расшифровки ещё нет — название подхватится, когда она появится.
            Err(_) => {
                self.db().execute("UPDATE recordings SET title=?2 WHERE id=?1", params![id, title])?;
            }
        }
        Ok(())
    }

    pub fn set_archived(&self, ids: &[String], archived: bool) -> Result<()> {
        let mut db = self.db();
        let tx = db.transaction()?;
        for id in ids {
            tx.execute("UPDATE recordings SET archived=?2 WHERE id=?1", params![id, archived])?;
        }
        Ok(tx.commit()?)
    }

    /// Кладёт записи в папку; `None` — убирает из папки.
    pub fn move_recordings(&self, ids: &[String], folder: Option<i64>) -> Result<()> {
        let mut db = self.db();
        let tx = db.transaction()?;
        for id in ids {
            tx.execute("UPDATE recordings SET folder_id=?2 WHERE id=?1", params![id, folder])?;
        }
        Ok(tx.commit()?)
    }

    // ---------- папки библиотеки ----------

    pub fn folders(&self) -> Result<Vec<Folder>> {
        let db = self.db();
        let mut st = db.prepare("SELECT id,name FROM folders ORDER BY name COLLATE NOCASE, id")?;
        let rows = st.query_map([], |r| Ok(Folder { id: r.get(0)?, name: r.get(1)? }))?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// Создаёт папку (`id` = None) или переименовывает.
    pub fn save_folder(&self, id: Option<i64>, name: &str) -> Result<i64> {
        let name = name.trim();
        anyhow::ensure!(!name.is_empty(), tr("название не может быть пустым", "the name cannot be empty"));
        let db = self.db();
        match id {
            Some(id) => {
                db.execute("UPDATE folders SET name=?2 WHERE id=?1", params![id, name])?;
                Ok(id)
            }
            None => {
                db.execute("INSERT INTO folders(name) VALUES(?1)", [name])?;
                Ok(db.last_insert_rowid())
            }
        }
    }

    /// Удаляет папку; её записи остаются в библиотеке.
    pub fn delete_folder(&self, id: i64) -> Result<()> {
        let mut db = self.db();
        let tx = db.transaction()?;
        tx.execute("UPDATE recordings SET folder_id=NULL WHERE folder_id=?1", [id])?;
        tx.execute("DELETE FROM folders WHERE id=?1", [id])?;
        Ok(tx.commit()?)
    }

    pub fn stats(&self) -> Result<Stats> {
        let db = self.db();
        let count = |sql: &str| -> Result<i64> { Ok(db.query_row(sql, [], |r| r.get(0))?) };
        Ok(Stats {
            recordings: count("SELECT COUNT(*) FROM recordings")?,
            seconds: db.query_row("SELECT COALESCE(SUM(duration), 0) FROM recordings", [], |r| r.get(0))?,
            terms: count("SELECT COUNT(*) FROM terms WHERE pending=0")?,
            people: count("SELECT COUNT(*) FROM people WHERE pending=0")?,
            voices: count("SELECT COUNT(DISTINCT person_id) FROM voiceprints WHERE person_id IS NOT NULL")?,
            rules: count("SELECT COUNT(*) FROM rules WHERE enabled=1")?,
        })
    }

    pub fn delete_recording(&self, id: &str) -> Result<()> {
        let db = self.db();
        db.execute("DELETE FROM recordings WHERE id=?1", [id])?;
        db.execute("DELETE FROM voiceprints WHERE recording_id=?1 AND person_id IS NULL", [id])?;
        db.execute("DELETE FROM processed WHERE recording_id=?1", [id])?;
        let _ = std::fs::remove_dir_all(self.recording_dir(id));
        Ok(())
    }

    pub fn load_transcript(&self, id: &str) -> Result<Transcript> {
        let raw = std::fs::read_to_string(self.recording_dir(id).join("transcript.json"))
            .context(tr("расшифровка ещё не готова", "the transcript is not ready yet"))?;
        Ok(serde_json::from_str(&raw)?)
    }

    pub fn save_transcript(&self, t: &Transcript) -> Result<()> {
        let dir = self.recording_dir(&t.id);
        std::fs::create_dir_all(&dir)?;
        let tmp = dir.join("transcript.json.tmp");
        std::fs::write(&tmp, serde_json::to_vec_pretty(t)?)?;
        std::fs::rename(tmp, dir.join("transcript.json"))?;
        self.db().execute(
            "UPDATE recordings SET title=?2, duration=?3 WHERE id=?1",
            params![t.id, t.title, t.duration],
        )?;
        Ok(())
    }

    // ---------- обработанные файлы из папок ----------

    pub fn is_processed(&self, path: &Path, size: u64) -> bool {
        self.db()
            .query_row(
                "SELECT 1 FROM processed WHERE path=?1 AND size=?2",
                params![path.to_string_lossy(), size as i64],
                |_| Ok(()),
            )
            .is_ok()
    }

    pub fn mark_processed(&self, path: &Path, size: u64, recording_id: &str) -> Result<()> {
        self.db().execute(
            "INSERT OR REPLACE INTO processed(path,size,recording_id) VALUES(?1,?2,?3)",
            params![path.to_string_lossy(), size as i64, recording_id],
        )?;
        Ok(())
    }

    // ---------- словарь ----------

    /// Сначала находки на проверке, затем по алфавиту.
    pub fn terms(&self) -> Result<Vec<Term>> {
        let db = self.db();
        let mut st = db.prepare(
            "SELECT id,term,aliases,definition,pending FROM terms ORDER BY pending DESC, term COLLATE NOCASE",
        )?;
        let rows = st.query_map([], |r| {
            Ok(Term { id: r.get(0)?, term: r.get(1)?, aliases: r.get(2)?, definition: r.get(3)?, pending: r.get(4)? })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    pub fn save_term(&self, t: &Term) -> Result<i64> {
        let db = self.db();
        match t.id {
            Some(id) => {
                db.execute(
                    "UPDATE terms SET term=?2, aliases=?3, definition=?4, pending=?5 WHERE id=?1",
                    params![id, t.term, t.aliases, t.definition, t.pending],
                )?;
                Ok(id)
            }
            None => {
                db.execute(
                    "INSERT INTO terms(term,aliases,definition,pending) VALUES(?1,?2,?3,?4)",
                    params![t.term, t.aliases, t.definition, t.pending],
                )?;
                Ok(db.last_insert_rowid())
            }
        }
    }

    pub fn delete_term(&self, id: i64) -> Result<()> {
        self.db().execute("DELETE FROM terms WHERE id=?1", [id])?;
        Ok(())
    }

    /// Термины из файла. Уже известный термин (без учёта регистра и «ё») дополняется непустыми
    /// полями и считается проверенным, остальные добавляются. Возвращает (добавлено, обновлено).
    pub fn merge_terms(&self, items: &[Term]) -> Result<(usize, usize)> {
        let mut known: HashMap<String, Term> = self.terms()?.into_iter().map(|t| (match_key(&t.term), t)).collect();
        let mut db = self.db();
        let tx = db.transaction()?;
        let (mut added, mut updated) = (0, 0);
        for item in items {
            match known.get_mut(&match_key(&item.term)) {
                Some(cur) => {
                    let next = Term {
                        aliases: fill(&cur.aliases, &item.aliases),
                        definition: fill(&cur.definition, &item.definition),
                        pending: false,
                        ..cur.clone()
                    };
                    if next != *cur {
                        tx.execute(
                            "UPDATE terms SET aliases=?2, definition=?3, pending=0 WHERE id=?1",
                            params![next.id, next.aliases, next.definition],
                        )?;
                        *cur = next;
                        updated += 1;
                    }
                }
                None => {
                    let t = Term {
                        term: item.term.trim().into(),
                        aliases: item.aliases.trim().into(),
                        definition: item.definition.trim().into(),
                        ..Default::default()
                    };
                    tx.execute(
                        "INSERT INTO terms(term,aliases,definition) VALUES(?1,?2,?3)",
                        params![t.term, t.aliases, t.definition],
                    )?;
                    known.insert(match_key(&t.term), Term { id: Some(tx.last_insert_rowid()), ..t });
                    added += 1;
                }
            }
        }
        tx.commit()?;
        Ok((added, updated))
    }

    // ---------- кто есть кто ----------

    /// Сначала находки на проверке, затем по алфавиту.
    pub fn people(&self) -> Result<Vec<Person>> {
        let db = self.db();
        let mut st = db.prepare(
            "SELECT p.id,p.name,p.aliases,p.role,p.org,
               (SELECT COUNT(*) FROM voiceprints v WHERE v.person_id=p.id), p.pending
             FROM people p ORDER BY p.pending DESC, p.name COLLATE NOCASE",
        )?;
        let rows = st.query_map([], |r| {
            Ok(Person {
                id: r.get(0)?,
                name: r.get(1)?,
                aliases: r.get(2)?,
                role: r.get(3)?,
                org: r.get(4)?,
                voiceprints: r.get(5)?,
                pending: r.get(6)?,
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    pub fn save_person(&self, p: &Person) -> Result<i64> {
        let db = self.db();
        match p.id {
            Some(id) => {
                db.execute(
                    "UPDATE people SET name=?2, aliases=?3, role=?4, org=?5, pending=?6 WHERE id=?1",
                    params![id, p.name, p.aliases, p.role, p.org, p.pending],
                )?;
                Ok(id)
            }
            None => {
                db.execute(
                    "INSERT INTO people(name,aliases,role,org,pending) VALUES(?1,?2,?3,?4,?5)",
                    params![p.name, p.aliases, p.role, p.org, p.pending],
                )?;
                Ok(db.last_insert_rowid())
            }
        }
    }

    /// Человек из находок LLM подтверждён: его указали спикером или загрузили его голос.
    pub fn confirm_person(&self, id: i64) -> Result<()> {
        self.db().execute("UPDATE people SET pending=0 WHERE id=?1", [id])?;
        Ok(())
    }

    /// Удаляет человека вместе с голосовым профилем и копиями загруженных образцов.
    pub fn delete_person(&self, id: i64) -> Result<()> {
        let clips: Vec<String> = {
            let db = self.db();
            let mut st = db.prepare("SELECT speaker FROM voiceprints WHERE person_id=?1 AND recording_id=''")?;
            let rows = st.query_map([id], |r| r.get(0))?;
            rows.collect::<Result<_, _>>()?
        };
        self.db().execute("DELETE FROM people WHERE id=?1", [id])?;
        for c in clips {
            let _ = std::fs::remove_file(self.voice_clip(&c));
        }
        Ok(())
    }

    /// Люди из файла — как `merge_terms`, совпадение по ФИО.
    pub fn merge_people(&self, items: &[Person]) -> Result<(usize, usize)> {
        let mut known: HashMap<String, Person> = self.people()?.into_iter().map(|p| (match_key(&p.name), p)).collect();
        let mut db = self.db();
        let tx = db.transaction()?;
        let (mut added, mut updated) = (0, 0);
        for item in items {
            match known.get_mut(&match_key(&item.name)) {
                Some(cur) => {
                    let next = Person {
                        aliases: fill(&cur.aliases, &item.aliases),
                        role: fill(&cur.role, &item.role),
                        org: fill(&cur.org, &item.org),
                        pending: false,
                        ..cur.clone()
                    };
                    if next != *cur {
                        tx.execute(
                            "UPDATE people SET aliases=?2, role=?3, org=?4, pending=0 WHERE id=?1",
                            params![next.id, next.aliases, next.role, next.org],
                        )?;
                        *cur = next;
                        updated += 1;
                    }
                }
                None => {
                    let p = Person {
                        name: item.name.trim().into(),
                        aliases: item.aliases.trim().into(),
                        role: item.role.trim().into(),
                        org: item.org.trim().into(),
                        ..Default::default()
                    };
                    tx.execute(
                        "INSERT INTO people(name,aliases,role,org) VALUES(?1,?2,?3,?4)",
                        params![p.name, p.aliases, p.role, p.org],
                    )?;
                    known.insert(match_key(&p.name), Person { id: Some(tx.last_insert_rowid()), ..p });
                    added += 1;
                }
            }
        }
        tx.commit()?;
        Ok((added, updated))
    }

    // ---------- находки LLM ----------

    /// Находка LLM сразу попадает в справочник: на проверку (`pending`) или, если так
    /// настроено, как обычная запись. Каждая находка остаётся в журнале `suggestions`,
    /// поэтому удалённое пользователем LLM больше не предложит.
    pub fn propose(&self, kind: &str, value: &str, detail: &str, recording_id: &str, pending: bool) -> Result<bool> {
        let mut db = self.db();
        let tx = db.transaction()?;
        let new = tx.execute(
            "INSERT OR IGNORE INTO suggestions(kind,value,detail,recording_id) VALUES(?1,?2,?3,?4)",
            params![kind, value, detail, recording_id],
        )? > 0;
        if new {
            let sql = match kind {
                "person" => "INSERT INTO people(name,role,pending) VALUES(?1,?2,?3)",
                _ => "INSERT INTO terms(term,definition,pending) VALUES(?1,?2,?3)",
            };
            tx.execute(sql, params![value, detail, pending])?;
        }
        tx.commit()?;
        Ok(new)
    }

    /// Всё, что LLM когда-либо находила (`kind`: "term" | "person").
    pub fn proposed(&self, kind: &str) -> Result<Vec<String>> {
        let db = self.db();
        let mut st = db.prepare("SELECT value FROM suggestions WHERE kind=?1")?;
        let rows = st.query_map([kind], |r| r.get(0))?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    pub fn pending(&self) -> Result<Pending> {
        let db = self.db();
        let count = |table: &str| -> Result<i64> {
            Ok(db.query_row(&format!("SELECT COUNT(*) FROM {table} WHERE pending=1"), [], |r| r.get(0))?)
        };
        Ok(Pending { terms: count("terms")?, people: count("people")? })
    }

    // ---------- голосовые отпечатки (AES-256-GCM, ключ в связке ключей) ----------

    /// Сохраняет голос спикера записи; `person_id` = None — пока не опознан.
    pub fn save_voice(&self, recording_id: &str, speaker: &str, person_id: Option<i64>, emb: &[f32]) -> Result<()> {
        let blob = self.encrypt(emb)?;
        self.db().execute(
            "INSERT INTO voiceprints(person_id,recording_id,speaker,embedding,created_at)
             VALUES(?1,?2,?3,?4,datetime('now'))
             ON CONFLICT(recording_id,speaker) DO UPDATE SET person_id=?1, embedding=?4",
            params![person_id, recording_id, speaker, blob],
        )?;
        Ok(())
    }

    /// Привязывает голос спикера записи к человеку (обучение по подтверждению).
    pub fn bind_voice(&self, recording_id: &str, speaker: &str, person_id: Option<i64>) -> Result<()> {
        self.db().execute(
            "UPDATE voiceprints SET person_id=?3 WHERE recording_id=?1 AND speaker=?2",
            params![recording_id, speaker, person_id],
        )?;
        Ok(())
    }

    /// Все отпечатки известных людей: (person_id, эмбеддинг).
    pub fn known_voices(&self) -> Result<Vec<(i64, Vec<f32>)>> {
        let db = self.db();
        let mut st = db.prepare(
            "SELECT person_id, embedding FROM voiceprints WHERE person_id IS NOT NULL
             ORDER BY created_at DESC",
        )?;
        let rows = st.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, Vec<u8>>(1)?)))?;
        let mut out = vec![];
        for row in rows {
            let (pid, blob) = row?;
            // Отпечаток, который не читается (сменился ключ шифрования), не должен мешать расшифровке.
            if let Ok(voice) = self.decrypt(&blob) {
                out.push((pid, voice));
            }
        }
        Ok(out)
    }

    /// Голосовые образцы человека, новые сверху.
    pub fn voices(&self, person_id: i64) -> Result<Vec<Voice>> {
        let db = self.db();
        let mut st = db.prepare(
            "SELECT id,recording_id,speaker,label,seconds,created_at FROM voiceprints
             WHERE person_id=?1 ORDER BY id DESC",
        )?;
        let rows = st.query_map([person_id], |r| {
            Ok(Voice {
                id: r.get(0)?,
                recording_id: r.get(1)?,
                speaker: r.get(2)?,
                label: r.get(3)?,
                seconds: r.get(4)?,
                created_at: r.get::<_, Option<String>>(5)?.unwrap_or_default(),
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// Копии загруженных образцов голоса (Opus) — чтобы их можно было послушать.
    pub fn voices_dir(&self) -> PathBuf {
        self.dir.join("voices")
    }

    pub fn voice_clip(&self, name: &str) -> PathBuf {
        self.voices_dir().join(format!("{name}.ogg"))
    }

    /// Образец голоса из файла: `clip` — имя копии (см. `voice_clip`), `label` — имя файла.
    /// Загруженный голос подтверждает человека, если тот был находкой LLM.
    pub fn add_voice_file(&self, person_id: i64, clip: &str, label: &str, seconds: f32, emb: &[f32]) -> Result<()> {
        let blob = self.encrypt(emb)?;
        let now = chrono::Local::now().format("%Y-%m-%d %H:%M").to_string();
        let db = self.db();
        db.execute(
            "INSERT INTO voiceprints(person_id,recording_id,speaker,embedding,created_at,label,seconds)
             VALUES(?1,'',?2,?3,?4,?5,?6)",
            params![person_id, clip, blob, now, label, seconds],
        )?;
        db.execute("UPDATE people SET pending=0 WHERE id=?1", [person_id])?;
        Ok(())
    }

    /// Убирает образец из профиля человека. Голос спикера из существующей записи только
    /// отвязывается — его можно снова назначить в расшифровке; остальное удаляется.
    pub fn delete_voice(&self, id: i64) -> Result<()> {
        let (recording_id, speaker): (String, String) =
            self.db().query_row("SELECT recording_id, speaker FROM voiceprints WHERE id=?1", [id], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })?;
        if recording_id.is_empty() {
            self.db().execute("DELETE FROM voiceprints WHERE id=?1", [id])?;
            let _ = std::fs::remove_file(self.voice_clip(&speaker));
        } else if self.recording(&recording_id).is_ok() {
            self.db().execute("UPDATE voiceprints SET person_id=NULL WHERE id=?1", [id])?;
        } else {
            self.db().execute("DELETE FROM voiceprints WHERE id=?1", [id])?;
        }
        Ok(())
    }

    fn encrypt(&self, v: &[f32]) -> Result<Vec<u8>> {
        let plain: Vec<u8> = v.iter().flat_map(|x| x.to_le_bytes()).collect();
        let nonce = Aes256Gcm::generate_nonce(&mut OsRng);
        let mut out = nonce.to_vec();
        out.extend(self.cipher.encrypt(&nonce, plain.as_ref()).map_err(|_| anyhow!("шифрование"))?);
        Ok(out)
    }

    fn decrypt(&self, blob: &[u8]) -> Result<Vec<f32>> {
        let (nonce, data) = blob.split_at(12);
        let plain = self
            .cipher
            .decrypt(Nonce::from_slice(nonce), data)
            .map_err(|_| anyhow!(tr("не удалось прочитать голосовой профиль", "could not read the voice profile")))?;
        Ok(plain.chunks_exact(4).map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]])).collect())
    }

    // ---------- правила папок ----------

    pub fn rules(&self) -> Result<Vec<Rule>> {
        let db = self.db();
        let mut st = db.prepare(
            "SELECT id,input_dir,audio_dir,transcript_dir,protocol_dir,protocol,enabled,docx FROM rules ORDER BY id",
        )?;
        let rows = st.query_map([], |r| {
            Ok(Rule {
                id: r.get(0)?,
                input_dir: r.get(1)?,
                audio_dir: r.get(2)?,
                transcript_dir: r.get(3)?,
                protocol_dir: r.get(4)?,
                protocol: r.get(5)?,
                enabled: r.get(6)?,
                docx: r.get(7)?,
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    pub fn save_rule(&self, r: &Rule) -> Result<i64> {
        let db = self.db();
        let args = params![r.input_dir, r.audio_dir, r.transcript_dir, r.protocol_dir, r.protocol, r.enabled, r.docx];
        match r.id {
            Some(id) => {
                db.execute(
                    &format!("UPDATE rules SET input_dir=?1,audio_dir=?2,transcript_dir=?3,protocol_dir=?4,
                              protocol=?5,enabled=?6,docx=?7 WHERE id={id}"),
                    args,
                )?;
                Ok(id)
            }
            None => {
                db.execute(
                    "INSERT INTO rules(input_dir,audio_dir,transcript_dir,protocol_dir,protocol,enabled,docx)
                     VALUES(?1,?2,?3,?4,?5,?6,?7)",
                    args,
                )?;
                Ok(db.last_insert_rowid())
            }
        }
    }

    pub fn delete_rule(&self, id: i64) -> Result<()> {
        self.db().execute("DELETE FROM rules WHERE id=?1", [id])?;
        Ok(())
    }
}

// ---------- секреты ----------
// В релизе — системная связка ключей (Keychain / Windows Credential Manager).
// В отладочной сборке — файл в каталоге данных: пересобранный бинарник
// иначе каждый раз запрашивает доступ к Keychain.

#[cfg(not(debug_assertions))]
fn secret_get(name: &str) -> Option<String> {
    keyring::Entry::new(APP_ID, name).ok()?.get_password().ok()
}

#[cfg(not(debug_assertions))]
fn secret_set(name: &str, value: &str) -> Result<()> {
    let e = keyring::Entry::new(APP_ID, name)?;
    if value.is_empty() {
        let _ = e.delete_credential();
    } else {
        e.set_password(value)?;
    }
    Ok(())
}

#[cfg(debug_assertions)]
fn secret_get(name: &str) -> Option<String> {
    std::fs::read_to_string(data_dir().join(format!(".dev-secret-{name}"))).ok()
}

#[cfg(debug_assertions)]
fn secret_set(name: &str, value: &str) -> Result<()> {
    Ok(std::fs::write(data_dir().join(format!(".dev-secret-{name}")), value)?)
}

fn vault_key(_dir: &Path) -> Result<Vec<u8>> {
    let b64 = base64::engine::general_purpose::STANDARD;
    if let Some(k) = secret_get("vault_key").and_then(|k| b64.decode(k.trim()).ok()) {
        if k.len() == 32 {
            return Ok(k);
        }
    }
    let key = Aes256Gcm::generate_key(&mut OsRng).to_vec();
    secret_set("vault_key", &b64.encode(&key))?;
    Ok(key)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Хранилище в памяти поверх готовой базы; файлы — во временном каталоге.
    fn store_with(db: Connection) -> Store {
        let dir = std::env::temp_dir().join(format!("ut-store-{}", uuid::Uuid::new_v4().simple()));
        Store { dir, db: Mutex::new(db), cipher: Aes256Gcm::new(&Aes256Gcm::generate_key(&mut OsRng)) }
    }

    fn memory() -> Store {
        let mut db = Connection::open_in_memory().unwrap();
        db.execute_batch(SCHEMA).unwrap();
        migrate(&mut db).unwrap();
        store_with(db)
    }

    #[test]
    fn migration_moves_new_suggestions_into_directories() {
        let mut db = Connection::open_in_memory().unwrap();
        db.execute_batch(SCHEMA).unwrap();
        db.execute_batch(
            "INSERT INTO terms(term) VALUES('Минпромторг');
             INSERT INTO suggestions(kind,value,detail,status) VALUES
               ('term','ГИСП','информационная система','new'), ('term','Минпромторг','','new'),
               ('term','Отклонённое','','rejected'), ('person','Петров Сергей','директор','new');",
        )
        .unwrap();
        migrate(&mut db).unwrap();
        migrate(&mut db).unwrap(); // повторно ничего не меняет
        let s = store_with(db);
        let terms: Vec<_> = s.terms().unwrap().into_iter().map(|t| (t.term, t.definition, t.pending)).collect();
        assert_eq!(
            terms,
            [("ГИСП".into(), "информационная система".into(), true), ("Минпромторг".into(), String::new(), false)]
        );
        let people = s.people().unwrap();
        assert_eq!((people[0].name.as_str(), people[0].role.as_str(), people[0].pending), ("Петров Сергей", "директор", true));
        assert_eq!((s.pending().unwrap().terms, s.pending().unwrap().people), (1, 1));
    }

    #[test]
    fn import_merges_known_entries_and_confirms_pending() {
        let s = memory();
        s.save_term(&Term { term: "Минпромторг".into(), definition: "Министерство".into(), ..Default::default() }).unwrap();
        s.propose("term", "ГИСП", "", "r1", true).unwrap();
        let (added, updated) = s
            .merge_terms(&[
                // Известный термин: пустое определение из файла не стирает имеющееся.
                Term { term: " минпромторг ".into(), aliases: "минпром торг".into(), ..Default::default() },
                // Находка LLM из файла — подтверждена.
                Term { term: "ГИСП".into(), ..Default::default() },
                Term { term: "ФРП".into(), definition: "Фонд развития промышленности".into(), ..Default::default() },
                // Повтор в том же файле ничего не меняет.
                Term { term: "фрп".into(), ..Default::default() },
            ])
            .unwrap();
        assert_eq!((added, updated), (1, 2));
        let terms: Vec<_> = s.terms().unwrap().into_iter().map(|t| (t.term, t.aliases, t.definition, t.pending)).collect();
        assert_eq!(
            terms,
            [
                ("ГИСП".into(), String::new(), String::new(), false),
                ("Минпромторг".into(), "минпром торг".into(), "Министерство".into(), false),
                ("ФРП".into(), String::new(), "Фонд развития промышленности".into(), false),
            ]
        );
        let (added, updated) = s
            .merge_people(&[Person { name: "Алёна Петрова".into(), role: "юрист".into(), ..Default::default() }])
            .unwrap();
        assert_eq!((added, updated), (1, 0));
        let (added, updated) =
            s.merge_people(&[Person { name: "алена петрова".into(), org: "Минпромторг".into(), ..Default::default() }]).unwrap();
        assert_eq!((added, updated), (0, 1));
        let p = &s.people().unwrap()[0];
        assert_eq!((p.name.as_str(), p.role.as_str(), p.org.as_str()), ("Алёна Петрова", "юрист", "Минпромторг"));
    }

    #[test]
    fn deleted_proposals_are_not_proposed_again() {
        let s = memory();
        assert!(s.propose("person", "Петров", "директор", "r1", true).unwrap());
        assert!(s.propose("term", "ГИСП", "", "r1", false).unwrap());
        assert!(!s.terms().unwrap()[0].pending, "без проверки — сразу в словарь");
        let id = s.people().unwrap()[0].id.unwrap();
        s.delete_person(id).unwrap();
        assert!(!s.propose("person", "Петров", "", "r2", true).unwrap());
        assert!(s.people().unwrap().is_empty());
        assert_eq!(s.proposed("person").unwrap(), ["Петров"]);
    }

    fn recording(id: &str, title: &str) -> Recording {
        Recording {
            id: id.into(),
            title: title.into(),
            source: String::new(),
            created_at: "2026-09-01 10:00".into(),
            duration: 60.0,
            status: "done".into(),
            error: String::new(),
            rule_id: None,
            folder_id: None,
            archived: false,
            has_protocol: false,
        }
    }

    #[test]
    fn recordings_go_to_folders_and_archive() {
        let s = memory();
        s.upsert_recording(&recording("r1", "Планёрка")).unwrap();
        s.upsert_recording(&recording("r2", "Интервью")).unwrap();
        let folder = s.save_folder(None, "  Проект  ").unwrap();
        assert!(s.save_folder(None, "  ").is_err(), "папка без названия");
        s.move_recordings(&["r1".into(), "r2".into()], Some(folder)).unwrap();
        s.set_archived(&["r2".into()], true).unwrap();
        // Повторная постановка в очередь не выбрасывает запись из папки и архива.
        s.upsert_recording(&Recording { status: "queued".into(), ..recording("r2", "Интервью") }).unwrap();
        let state = |s: &Store| -> Vec<(String, Option<i64>, bool)> {
            let mut all = s.recordings().unwrap();
            all.sort_by(|a, b| a.id.cmp(&b.id));
            all.into_iter().map(|r| (r.id, r.folder_id, r.archived)).collect()
        };
        assert_eq!(state(&s), [("r1".into(), Some(folder), false), ("r2".into(), Some(folder), true)]);

        assert_eq!(s.save_folder(Some(folder), "Порт").unwrap(), folder);
        assert_eq!(s.folders().unwrap(), [Folder { id: folder, name: "Порт".into() }]);
        s.delete_folder(folder).unwrap();
        assert!(s.folders().unwrap().is_empty());
        assert_eq!(state(&s), [("r1".into(), None, false), ("r2".into(), None, true)], "записи остались в библиотеке");
        assert_eq!(s.stats().unwrap().recordings, 2);
    }

    #[test]
    fn rename_changes_library_and_transcript() {
        let s = memory();
        s.upsert_recording(&recording("r1", "запись-001")).unwrap();
        // Расшифровки ещё нет — меняется только библиотека.
        s.rename_recording("r1", " Планёрка ").unwrap();
        assert_eq!(s.recording("r1").unwrap().title, "Планёрка");
        s.save_transcript(&Transcript { id: "r1".into(), title: "Планёрка".into(), duration: 60.0, ..Default::default() }).unwrap();
        s.rename_recording("r1", "Совет директоров").unwrap();
        assert_eq!(s.recording("r1").unwrap().title, "Совет директоров");
        assert_eq!(s.load_transcript("r1").unwrap().title, "Совет директоров");
        assert!(s.rename_recording("r1", "  ").is_err());
        let _ = std::fs::remove_dir_all(&s.dir);
    }

    #[test]
    fn old_settings_get_defaults_for_new_fields() {
        let s = memory();
        s.db().execute("INSERT INTO settings(key, value) VALUES('settings', '{\"llm_enabled\":true,\"theme\":\"dark\"}')", []).unwrap();
        let v = s.settings();
        assert!(v.llm_enabled && v.notifications && !v.developer_mode);
        assert_eq!((v.theme, v.language, v.speech_language.as_str()), (Theme::Dark, Language::System, "auto"));
    }

    #[test]
    fn voice_samples_are_unbound_or_deleted() {
        let s = memory();
        let pid = s.save_person(&Person { name: "Петров".into(), pending: true, ..Default::default() }).unwrap();
        s.upsert_recording(&recording("r1", "Планёрка")).unwrap();
        s.save_voice("r1", "S1", Some(pid), &[1.0, 0.0]).unwrap();
        s.save_voice("gone", "S2", Some(pid), &[0.0, 1.0]).unwrap();
        std::fs::create_dir_all(s.voices_dir()).unwrap();
        std::fs::write(s.voice_clip("clip"), b"ogg").unwrap();
        s.add_voice_file(pid, "clip", "петров.m4a", 12.0, &[0.6, 0.8]).unwrap();
        let p = &s.people().unwrap()[0];
        assert_eq!((p.voiceprints, p.pending), (3, false), "загруженный голос подтверждает человека");

        for v in s.voices(pid).unwrap() {
            s.delete_voice(v.id).unwrap();
        }
        assert_eq!(s.people().unwrap()[0].voiceprints, 0);
        assert!(!s.voice_clip("clip").exists());
        // Голос из существующей записи остался — его можно назначить снова.
        let left: Vec<(String, Option<i64>)> = {
            let db = s.db();
            let mut st = db.prepare("SELECT recording_id, person_id FROM voiceprints").unwrap();
            st.query_map([], |r| Ok((r.get(0)?, r.get(1)?))).unwrap().collect::<Result<_, _>>().unwrap()
        };
        assert_eq!(left, [("r1".to_string(), None)]);
        let _ = std::fs::remove_dir_all(&s.dir);
    }
}
