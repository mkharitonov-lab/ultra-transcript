//! Локальное хранилище: библиотека, словарь, «кто есть кто», голосовые отпечатки
//! (зашифрованы), предложения, правила папок, настройки.

use crate::speech::AsrModel;
use crate::transcript::Transcript;
use aes_gcm::aead::{Aead, AeadCore, KeyInit, OsRng};
use aes_gcm::{Aes256Gcm, Key, Nonce};
use anyhow::{anyhow, Context, Result};
use base64::Engine as _;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

pub const APP_ID: &str = "app.ultratranscript";

pub fn data_dir() -> PathBuf {
    dirs::data_dir().expect("нет каталога данных").join(APP_ID)
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub asr_model: AsrModel,
    pub llm_enabled: bool,
    pub llm_base_url: String,
    pub llm_model: String,
    /// Не сериализуется в БД — хранится в системной связке ключей.
    pub llm_api_key: String,
    pub cluster_threshold: f32,
    pub voice_threshold: f32,
    pub archive_kbps: u32,
    pub auto_accept_suggestions: bool,
    pub transcript_template: String,
    pub protocol_template: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            asr_model: AsrModel::default(),
            llm_enabled: false,
            llm_base_url: "http://localhost:11434/v1".into(),
            llm_model: "qwen3:8b".into(),
            llm_api_key: String::new(),
            cluster_threshold: 0.4,
            voice_threshold: 0.55,
            archive_kbps: 24,
            auto_accept_suggestions: false,
            transcript_template: String::new(),
            protocol_template: String::new(),
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Term {
    pub id: Option<i64>,
    pub term: String,
    /// Как слышится / частые ошибки распознавания, через запятую.
    pub aliases: String,
    pub definition: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Person {
    pub id: Option<i64>,
    pub name: String,
    pub aliases: String,
    pub role: String,
    pub org: String,
    #[serde(default)]
    pub voiceprints: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Suggestion {
    pub id: i64,
    pub kind: String,
    pub value: String,
    pub detail: String,
    pub recording_id: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Rule {
    pub id: Option<i64>,
    pub input_dir: String,
    pub audio_dir: String,
    pub transcript_dir: String,
    pub protocol_dir: String,
    pub protocol: bool,
    pub enabled: bool,
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

impl Store {
    pub fn open() -> Result<Self> {
        let dir = data_dir();
        std::fs::create_dir_all(dir.join("recordings"))?;
        let db = Connection::open(dir.join("library.db"))?;
        db.execute_batch(SCHEMA)?;
        let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&vault_key(&dir)?));
        Ok(Self { dir, db: Mutex::new(db), cipher })
    }

    fn db(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.db.lock().unwrap()
    }

    pub fn models_dir(&self) -> PathBuf {
        self.dir.join("models")
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
            "SELECT id,title,source,created_at,duration,status,error,rule_id FROM recordings
             ORDER BY created_at DESC",
        )?;
        let rows = st.query_map([], |r| {
            Ok(Recording {
                id: r.get(0)?,
                title: r.get(1)?,
                source: r.get(2)?,
                created_at: r.get(3)?,
                duration: r.get(4)?,
                status: r.get(5)?,
                error: r.get(6)?,
                rule_id: r.get(7)?,
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    pub fn recording(&self, id: &str) -> Result<Recording> {
        self.recordings()?
            .into_iter()
            .find(|r| r.id == id)
            .ok_or_else(|| anyhow!("запись не найдена"))
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
            .context("расшифровка ещё не готова")?;
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

    pub fn terms(&self) -> Result<Vec<Term>> {
        let db = self.db();
        let mut st = db.prepare("SELECT id,term,aliases,definition FROM terms ORDER BY term COLLATE NOCASE")?;
        let rows = st.query_map([], |r| {
            Ok(Term { id: r.get(0)?, term: r.get(1)?, aliases: r.get(2)?, definition: r.get(3)? })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    pub fn save_term(&self, t: &Term) -> Result<i64> {
        let db = self.db();
        match t.id {
            Some(id) => {
                db.execute(
                    "UPDATE terms SET term=?2, aliases=?3, definition=?4 WHERE id=?1",
                    params![id, t.term, t.aliases, t.definition],
                )?;
                Ok(id)
            }
            None => {
                db.execute(
                    "INSERT INTO terms(term,aliases,definition) VALUES(?1,?2,?3)",
                    params![t.term, t.aliases, t.definition],
                )?;
                Ok(db.last_insert_rowid())
            }
        }
    }

    pub fn delete_term(&self, id: i64) -> Result<()> {
        self.db().execute("DELETE FROM terms WHERE id=?1", [id])?;
        Ok(())
    }

    // ---------- кто есть кто ----------

    pub fn people(&self) -> Result<Vec<Person>> {
        let db = self.db();
        let mut st = db.prepare(
            "SELECT p.id,p.name,p.aliases,p.role,p.org,
               (SELECT COUNT(*) FROM voiceprints v WHERE v.person_id=p.id)
             FROM people p ORDER BY p.name COLLATE NOCASE",
        )?;
        let rows = st.query_map([], |r| {
            Ok(Person {
                id: r.get(0)?,
                name: r.get(1)?,
                aliases: r.get(2)?,
                role: r.get(3)?,
                org: r.get(4)?,
                voiceprints: r.get(5)?,
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    pub fn save_person(&self, p: &Person) -> Result<i64> {
        let db = self.db();
        match p.id {
            Some(id) => {
                db.execute(
                    "UPDATE people SET name=?2, aliases=?3, role=?4, org=?5 WHERE id=?1",
                    params![id, p.name, p.aliases, p.role, p.org],
                )?;
                Ok(id)
            }
            None => {
                db.execute(
                    "INSERT INTO people(name,aliases,role,org) VALUES(?1,?2,?3,?4)",
                    params![p.name, p.aliases, p.role, p.org],
                )?;
                Ok(db.last_insert_rowid())
            }
        }
    }

    pub fn delete_person(&self, id: i64) -> Result<()> {
        self.db().execute("DELETE FROM people WHERE id=?1", [id])?;
        Ok(())
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
            out.push((pid, self.decrypt(&blob)?));
        }
        Ok(out)
    }

    pub fn delete_voices(&self, person_id: i64) -> Result<()> {
        self.db().execute("DELETE FROM voiceprints WHERE person_id=?1", [person_id])?;
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
            .map_err(|_| anyhow!("не удалось расшифровать голосовой отпечаток"))?;
        Ok(plain.chunks_exact(4).map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]])).collect())
    }

    // ---------- предложения для автопополнения ----------

    /// Добавляет предложение, если такого ещё не было (включая отклонённые).
    pub fn suggest(&self, kind: &str, value: &str, detail: &str, recording_id: &str) -> Result<bool> {
        let n = self.db().execute(
            "INSERT OR IGNORE INTO suggestions(kind,value,detail,recording_id) VALUES(?1,?2,?3,?4)",
            params![kind, value, detail, recording_id],
        )?;
        Ok(n > 0)
    }

    pub fn suggestions(&self) -> Result<Vec<Suggestion>> {
        let db = self.db();
        let mut st = db.prepare(
            "SELECT id,kind,value,detail,recording_id FROM suggestions WHERE status='new' ORDER BY id DESC",
        )?;
        let rows = st.query_map([], |r| {
            Ok(Suggestion {
                id: r.get(0)?,
                kind: r.get(1)?,
                value: r.get(2)?,
                detail: r.get(3)?,
                recording_id: r.get(4)?,
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    pub fn resolve_suggestion(&self, id: i64, accept: bool) -> Result<()> {
        let s: Suggestion = self.db().query_row(
            "SELECT id,kind,value,detail,recording_id FROM suggestions WHERE id=?1",
            [id],
            |r| Ok(Suggestion { id: r.get(0)?, kind: r.get(1)?, value: r.get(2)?, detail: r.get(3)?, recording_id: r.get(4)? }),
        )?;
        if accept {
            match s.kind.as_str() {
                "person" => {
                    self.save_person(&Person { name: s.value.clone(), role: s.detail.clone(), ..Default::default() })?;
                }
                _ => {
                    self.save_term(&Term { term: s.value.clone(), definition: s.detail.clone(), ..Default::default() })?;
                }
            }
        }
        self.db().execute(
            "UPDATE suggestions SET status=?2 WHERE id=?1",
            params![id, if accept { "accepted" } else { "rejected" }],
        )?;
        Ok(())
    }

    // ---------- правила папок ----------

    pub fn rules(&self) -> Result<Vec<Rule>> {
        let db = self.db();
        let mut st = db.prepare(
            "SELECT id,input_dir,audio_dir,transcript_dir,protocol_dir,protocol,enabled FROM rules ORDER BY id",
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
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    pub fn save_rule(&self, r: &Rule) -> Result<i64> {
        let db = self.db();
        let args = params![r.input_dir, r.audio_dir, r.transcript_dir, r.protocol_dir, r.protocol, r.enabled];
        match r.id {
            Some(id) => {
                db.execute(
                    &format!("UPDATE rules SET input_dir=?1,audio_dir=?2,transcript_dir=?3,protocol_dir=?4,
                              protocol=?5,enabled=?6 WHERE id={id}"),
                    args,
                )?;
                Ok(id)
            }
            None => {
                db.execute(
                    "INSERT INTO rules(input_dir,audio_dir,transcript_dir,protocol_dir,protocol,enabled)
                     VALUES(?1,?2,?3,?4,?5,?6)",
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
