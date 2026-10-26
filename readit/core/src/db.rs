//! Local SQLite store. All library data lives on the device.
//!
//! Migrations are applied in order inside a transaction and tracked with
//! `PRAGMA user_version`, so an interrupted upgrade rolls back cleanly and is
//! retried on the next launch.

use crate::error::{ReaditError, Result};
use crate::model::*;
use rusqlite::{params, Connection, OptionalExtension, Row};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

pub fn now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

const MIGRATIONS: &[&str] = &[
    // v1: initial schema
    r#"
    CREATE TABLE documents (
        id TEXT PRIMARY KEY,
        title TEXT NOT NULL,
        file_name TEXT NOT NULL,
        format TEXT NOT NULL,
        source_path TEXT NOT NULL,
        size_bytes INTEGER NOT NULL,
        location_kind TEXT NOT NULL,
        location_count INTEGER NOT NULL,
        section_count INTEGER NOT NULL,
        imported_at INTEGER NOT NULL,
        last_opened_at INTEGER,
        favourite INTEGER NOT NULL DEFAULT 0,
        warnings TEXT NOT NULL DEFAULT '[]'
    );
    CREATE TABLE sections (
        document_id TEXT NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
        ordinal INTEGER NOT NULL,
        kind TEXT NOT NULL,
        level INTEGER NOT NULL,
        text TEXT NOT NULL,
        location INTEGER NOT NULL,
        PRIMARY KEY (document_id, ordinal)
    );
    CREATE VIRTUAL TABLE sections_fts USING fts5(document_id UNINDEXED, ordinal UNINDEXED, text);
    CREATE TABLE reading_positions (
        document_id TEXT PRIMARY KEY REFERENCES documents(id) ON DELETE CASCADE,
        section_ordinal INTEGER NOT NULL,
        segment_index INTEGER NOT NULL,
        updated_at INTEGER NOT NULL
    );
    CREATE TABLE bookmarks (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        document_id TEXT NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
        section_ordinal INTEGER NOT NULL,
        segment_index INTEGER NOT NULL,
        label TEXT NOT NULL,
        note TEXT,
        highlight_text TEXT,
        created_at INTEGER NOT NULL
    );
    CREATE TABLE listening_history (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        document_id TEXT NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
        started_at INTEGER NOT NULL,
        seconds_listened INTEGER NOT NULL DEFAULT 0
    );
    CREATE TABLE queue (
        position INTEGER PRIMARY KEY,
        document_id TEXT NOT NULL REFERENCES documents(id) ON DELETE CASCADE
    );
    CREATE TABLE collections (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        name TEXT NOT NULL UNIQUE
    );
    CREATE TABLE collection_items (
        collection_id INTEGER NOT NULL REFERENCES collections(id) ON DELETE CASCADE,
        document_id TEXT NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
        PRIMARY KEY (collection_id, document_id)
    );
    CREATE TABLE pronunciations (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        term TEXT NOT NULL,
        replacement TEXT NOT NULL,
        case_sensitive INTEGER NOT NULL DEFAULT 0,
        document_id TEXT REFERENCES documents(id) ON DELETE CASCADE
    );
    CREATE TABLE installed_voices (
        id TEXT PRIMARY KEY,
        engine TEXT NOT NULL,
        name TEXT NOT NULL,
        language TEXT NOT NULL,
        accent TEXT NOT NULL,
        gender TEXT NOT NULL,
        quality TEXT NOT NULL,
        licence TEXT NOT NULL,
        size_bytes INTEGER NOT NULL,
        offline INTEGER NOT NULL,
        path TEXT NOT NULL,
        installed_at INTEGER NOT NULL
    );
    CREATE TABLE consent_records (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        subject_name TEXT NOT NULL,
        relation TEXT NOT NULL CHECK (relation IN ('self', 'authorised_other')),
        statement TEXT NOT NULL,
        processing_location TEXT NOT NULL,
        accepted_at INTEGER NOT NULL,
        revoked_at INTEGER
    );
    CREATE TABLE voice_profiles (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        name TEXT NOT NULL,
        description TEXT NOT NULL DEFAULT '',
        language TEXT NOT NULL,
        consent_id INTEGER NOT NULL REFERENCES consent_records(id),
        recording_path TEXT,
        model_path TEXT,
        disabled INTEGER NOT NULL DEFAULT 0,
        created_at INTEGER NOT NULL
    );
    CREATE TABLE reading_profiles (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        name TEXT NOT NULL UNIQUE,
        voice_id TEXT,
        speed REAL NOT NULL DEFAULT 1.0,
        pitch REAL NOT NULL DEFAULT 1.0,
        paragraph_pause_ms INTEGER NOT NULL DEFAULT 350,
        heading_pause_ms INTEGER NOT NULL DEFAULT 700,
        read_speaker_notes INTEGER NOT NULL DEFAULT 0,
        read_slide_content INTEGER NOT NULL DEFAULT 1,
        skip_headers_footers INTEGER NOT NULL DEFAULT 1
    );
    CREATE TABLE audio_cache (
        segment_id TEXT NOT NULL,
        voice_id TEXT NOT NULL,
        speed REAL NOT NULL,
        path TEXT NOT NULL,
        size_bytes INTEGER NOT NULL,
        created_at INTEGER NOT NULL,
        PRIMARY KEY (segment_id, voice_id, speed)
    );
    CREATE TABLE settings (key TEXT PRIMARY KEY, value TEXT NOT NULL);
    INSERT INTO reading_profiles (name, speed, paragraph_pause_ms, heading_pause_ms) VALUES
        ('Study', 0.9, 500, 900),
        ('Work', 1.25, 300, 600),
        ('Relaxed', 1.0, 700, 1100),
        ('Accessibility', 1.0, 450, 800),
        ('My Reader', 1.0, 350, 700);
    "#,
    // v2: starter pronunciations for Ghanaian terms. Ordinary rules: users can edit or delete them.
    r#"
    INSERT INTO pronunciations (term, replacement, case_sensitive) VALUES
        ('cedis', 'see-deez', 0),
        ('cedi', 'see-dee', 0),
        ('pesewas', 'peh-SEH-wahz', 0),
        ('pesewa', 'peh-SEH-wah', 0),
        ('Accra', 'ah-KRAH', 1),
        ('Kumasi', 'koo-MAH-see', 1),
        ('Tamale', 'TAH-mah-leh', 1),
        ('Takoradi', 'tah-koh-RAH-dee', 1);
    "#,
];

pub struct Db {
    conn: Connection,
}

impl Db {
    pub fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open(path)?;
        Self::init(conn)
    }

    pub fn open_in_memory() -> Result<Self> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(mut conn: Connection) -> Result<Self> {
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        let current: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
        for (i, sql) in MIGRATIONS.iter().enumerate().skip(current as usize) {
            let tx = conn.transaction()?;
            tx.execute_batch(sql)?;
            tx.pragma_update(None, "user_version", (i + 1) as i64)?;
            tx.commit()?;
        }
        Ok(Self { conn })
    }

    pub fn schema_version(&self) -> Result<i64> {
        Ok(self.conn.pragma_query_value(None, "user_version", |r| r.get(0))?)
    }

    // ---------- documents ----------

    pub fn document_exists(&self, id: &str) -> Result<bool> {
        Ok(self
            .conn
            .query_row("SELECT 1 FROM documents WHERE id = ?1", [id], |_| Ok(()))
            .optional()?
            .is_some())
    }

    pub fn insert_document(&mut self, doc: &Document, sections: &[RawSection]) -> Result<()> {
        let tx = self.conn.transaction()?;
        tx.execute(
            "INSERT INTO documents (id, title, file_name, format, source_path, size_bytes,
                location_kind, location_count, section_count, imported_at, favourite, warnings)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, 0, ?11)",
            params![
                doc.id,
                doc.title,
                doc.file_name,
                doc.format.as_str(),
                doc.source_path,
                doc.size_bytes as i64,
                doc.location_kind.as_str(),
                doc.location_count,
                sections.len() as u32,
                doc.imported_at,
                serde_json::to_string(&doc.warnings).unwrap_or_else(|_| "[]".into()),
            ],
        )?;
        {
            let mut ins = tx.prepare(
                "INSERT INTO sections (document_id, ordinal, kind, level, text, location)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            )?;
            let mut fts =
                tx.prepare("INSERT INTO sections_fts (document_id, ordinal, text) VALUES (?1, ?2, ?3)")?;
            for (i, s) in sections.iter().enumerate() {
                ins.execute(params![doc.id, i as u32, s.kind.as_str(), s.level, s.text, s.location])?;
                fts.execute(params![doc.id, i as u32, s.text])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    fn row_to_document(r: &Row) -> rusqlite::Result<Document> {
        let warnings: String = r.get("warnings")?;
        Ok(Document {
            id: r.get("id")?,
            title: r.get("title")?,
            file_name: r.get("file_name")?,
            format: DocFormat::parse(&r.get::<_, String>("format")?).unwrap_or(DocFormat::Txt),
            source_path: r.get("source_path")?,
            size_bytes: r.get::<_, i64>("size_bytes")? as u64,
            location_kind: LocationKind::parse(&r.get::<_, String>("location_kind")?),
            location_count: r.get("location_count")?,
            section_count: r.get("section_count")?,
            imported_at: r.get("imported_at")?,
            last_opened_at: r.get("last_opened_at")?,
            favourite: r.get::<_, i64>("favourite")? != 0,
            warnings: serde_json::from_str(&warnings).unwrap_or_default(),
        })
    }

    pub fn get_document(&self, id: &str) -> Result<Document> {
        self.conn
            .query_row("SELECT * FROM documents WHERE id = ?1", [id], Self::row_to_document)
            .optional()?
            .ok_or_else(|| ReaditError::NotFound { detail: format!("document {id}") })
    }

    /// `filter`: "all", "recent", "favourites". `query` matches title or file name.
    pub fn list_documents(&self, filter: &str, query: Option<&str>) -> Result<Vec<Document>> {
        let mut sql = String::from("SELECT * FROM documents WHERE 1=1");
        if filter == "favourites" {
            sql.push_str(" AND favourite = 1");
        }
        if filter == "recent" {
            sql.push_str(" AND last_opened_at IS NOT NULL");
        }
        if query.is_some() {
            sql.push_str(" AND (title LIKE ?1 OR file_name LIKE ?1)");
        }
        sql.push_str(if filter == "recent" {
            " ORDER BY last_opened_at DESC"
        } else {
            " ORDER BY COALESCE(last_opened_at, imported_at) DESC"
        });
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = match query {
            Some(q) => stmt.query_map([format!("%{q}%")], Self::row_to_document)?.collect::<Vec<_>>(),
            None => stmt.query_map([], Self::row_to_document)?.collect::<Vec<_>>(),
        };
        Ok(rows.into_iter().collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn mark_opened(&self, id: &str) -> Result<()> {
        self.conn.execute("UPDATE documents SET last_opened_at = ?2 WHERE id = ?1", params![id, now()])?;
        Ok(())
    }

    pub fn set_favourite(&self, id: &str, fav: bool) -> Result<()> {
        self.conn.execute("UPDATE documents SET favourite = ?2 WHERE id = ?1", params![id, fav as i64])?;
        Ok(())
    }

    pub fn rename_document(&self, id: &str, title: &str) -> Result<()> {
        self.conn.execute("UPDATE documents SET title = ?2 WHERE id = ?1", params![id, title])?;
        Ok(())
    }

    /// Removes the library entry and everything derived from it. The source
    /// file is user-owned and is never touched here.
    pub fn remove_document(&mut self, id: &str) -> Result<()> {
        let tx = self.conn.transaction()?;
        tx.execute("DELETE FROM sections_fts WHERE document_id = ?1", [id])?;
        tx.execute("DELETE FROM audio_cache WHERE segment_id LIKE ?1", [format!("{id}:%")])?;
        tx.execute("DELETE FROM documents WHERE id = ?1", [id])?;
        tx.commit()?;
        Ok(())
    }

    // ---------- sections ----------

    fn row_to_section(r: &Row) -> rusqlite::Result<Section> {
        let doc: String = r.get("document_id")?;
        let ordinal: u32 = r.get("ordinal")?;
        Ok(Section {
            id: format!("{doc}:{ordinal}"),
            document_id: doc,
            ordinal,
            kind: SectionKind::parse(&r.get::<_, String>("kind")?),
            level: r.get("level")?,
            text: r.get("text")?,
            location: r.get("location")?,
        })
    }

    /// Paged access so large books never need to be loaded whole.
    pub fn sections(&self, doc_id: &str, from: u32, limit: u32) -> Result<Vec<Section>> {
        let mut stmt = self.conn.prepare(
            "SELECT * FROM sections WHERE document_id = ?1 AND ordinal >= ?2 ORDER BY ordinal LIMIT ?3",
        )?;
        let v = stmt
            .query_map(params![doc_id, from, limit], Self::row_to_section)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(v)
    }

    pub fn section(&self, doc_id: &str, ordinal: u32) -> Result<Section> {
        self.conn
            .query_row(
                "SELECT * FROM sections WHERE document_id = ?1 AND ordinal = ?2",
                params![doc_id, ordinal],
                Self::row_to_section,
            )
            .optional()?
            .ok_or_else(|| ReaditError::NotFound { detail: format!("section {doc_id}:{ordinal}") })
    }

    /// Headings form the table of contents.
    pub fn outline(&self, doc_id: &str) -> Result<Vec<Section>> {
        let mut stmt = self.conn.prepare(
            "SELECT * FROM sections WHERE document_id = ?1 AND kind = 'heading' ORDER BY ordinal",
        )?;
        let v = stmt.query_map([doc_id], Self::row_to_section)?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(v)
    }

    /// First section on a given page, slide or chapter.
    pub fn first_section_at(&self, doc_id: &str, location: u32) -> Result<Option<u32>> {
        Ok(self
            .conn
            .query_row(
                "SELECT MIN(ordinal) FROM sections WHERE document_id = ?1 AND location >= ?2",
                params![doc_id, location],
                |r| r.get::<_, Option<u32>>(0),
            )
            .optional()?
            .flatten())
    }

    pub fn search(&self, doc_id: &str, query: &str, limit: u32) -> Result<Vec<Section>> {
        // Quote each term so user input cannot inject FTS syntax.
        let fts_query = query
            .split_whitespace()
            .map(|t| format!("\"{}\"", t.replace('"', "")))
            .collect::<Vec<_>>()
            .join(" ");
        if fts_query.is_empty() {
            return Ok(vec![]);
        }
        let mut stmt = self.conn.prepare(
            "SELECT s.* FROM sections_fts f JOIN sections s
               ON s.document_id = f.document_id AND s.ordinal = f.ordinal
             WHERE f.document_id = ?1 AND sections_fts MATCH ?2
             ORDER BY s.ordinal LIMIT ?3",
        )?;
        let v = stmt
            .query_map(params![doc_id, fts_query, limit], Self::row_to_section)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(v)
    }

    // ---------- progress, history, queue ----------

    pub fn save_position(&self, doc_id: &str, section: u32, segment: u32) -> Result<()> {
        self.conn.execute(
            "INSERT INTO reading_positions (document_id, section_ordinal, segment_index, updated_at)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(document_id) DO UPDATE SET section_ordinal = ?2, segment_index = ?3, updated_at = ?4",
            params![doc_id, section, segment, now()],
        )?;
        Ok(())
    }

    pub fn position(&self, doc_id: &str) -> Result<Option<ReadingPosition>> {
        Ok(self
            .conn
            .query_row(
                "SELECT * FROM reading_positions WHERE document_id = ?1",
                [doc_id],
                |r| {
                    Ok(ReadingPosition {
                        document_id: r.get("document_id")?,
                        section_ordinal: r.get("section_ordinal")?,
                        segment_index: r.get("segment_index")?,
                        updated_at: r.get("updated_at")?,
                    })
                },
            )
            .optional()?)
    }

    pub fn log_listening(&self, doc_id: &str, seconds: u32) -> Result<()> {
        self.conn.execute(
            "INSERT INTO listening_history (document_id, started_at, seconds_listened) VALUES (?1, ?2, ?3)",
            params![doc_id, now() - seconds as i64, seconds],
        )?;
        Ok(())
    }

    pub fn queue(&self) -> Result<Vec<String>> {
        let mut stmt = self.conn.prepare("SELECT document_id FROM queue ORDER BY position")?;
        let v = stmt.query_map([], |r| r.get(0))?.collect::<rusqlite::Result<Vec<String>>>()?;
        Ok(v)
    }

    pub fn set_queue(&mut self, ids: &[String]) -> Result<()> {
        let tx = self.conn.transaction()?;
        tx.execute("DELETE FROM queue", [])?;
        for (i, id) in ids.iter().enumerate() {
            tx.execute("INSERT INTO queue (position, document_id) VALUES (?1, ?2)", params![i as i64, id])?;
        }
        tx.commit()?;
        Ok(())
    }

    // ---------- bookmarks and notes ----------

    pub fn add_bookmark(
        &self,
        doc_id: &str,
        section: u32,
        segment: u32,
        label: &str,
        note: Option<&str>,
        highlight: Option<&str>,
    ) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO bookmarks (document_id, section_ordinal, segment_index, label, note, highlight_text, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![doc_id, section, segment, label, note, highlight, now()],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn bookmarks(&self, doc_id: &str) -> Result<Vec<Bookmark>> {
        let mut stmt = self.conn.prepare(
            "SELECT * FROM bookmarks WHERE document_id = ?1 ORDER BY section_ordinal, segment_index",
        )?;
        let v = stmt
            .query_map([doc_id], |r| {
                Ok(Bookmark {
                    id: r.get("id")?,
                    document_id: r.get("document_id")?,
                    section_ordinal: r.get("section_ordinal")?,
                    segment_index: r.get("segment_index")?,
                    label: r.get("label")?,
                    note: r.get("note")?,
                    highlight_text: r.get("highlight_text")?,
                    created_at: r.get("created_at")?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(v)
    }

    pub fn update_bookmark_note(&self, id: i64, note: Option<&str>) -> Result<()> {
        self.conn.execute("UPDATE bookmarks SET note = ?2 WHERE id = ?1", params![id, note])?;
        Ok(())
    }

    pub fn delete_bookmark(&self, id: i64) -> Result<()> {
        self.conn.execute("DELETE FROM bookmarks WHERE id = ?1", [id])?;
        Ok(())
    }

    // ---------- collections ----------

    pub fn create_collection(&self, name: &str) -> Result<i64> {
        self.conn.execute("INSERT INTO collections (name) VALUES (?1)", [name])?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn add_to_collection(&self, collection: i64, doc_id: &str) -> Result<()> {
        self.conn.execute(
            "INSERT OR IGNORE INTO collection_items (collection_id, document_id) VALUES (?1, ?2)",
            params![collection, doc_id],
        )?;
        Ok(())
    }

    pub fn collections(&self) -> Result<Vec<Collection>> {
        let mut stmt = self.conn.prepare(
            "SELECT c.id, c.name, COUNT(i.document_id) FROM collections c
             LEFT JOIN collection_items i ON i.collection_id = c.id GROUP BY c.id ORDER BY c.name",
        )?;
        let v = stmt
            .query_map([], |r| Ok(Collection { id: r.get(0)?, name: r.get(1)?, document_count: r.get(2)? }))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(v)
    }

    // ---------- pronunciation ----------

    pub fn add_pronunciation(
        &self,
        term: &str,
        replacement: &str,
        case_sensitive: bool,
        doc_id: Option<&str>,
    ) -> Result<i64> {
        if term.trim().is_empty() {
            return Err(ReaditError::Invalid { detail: "The word to replace cannot be empty.".into() });
        }
        self.conn.execute(
            "INSERT INTO pronunciations (term, replacement, case_sensitive, document_id) VALUES (?1, ?2, ?3, ?4)",
            params![term, replacement, case_sensitive as i64, doc_id],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    /// Global rules plus rules specific to `doc_id`.
    pub fn pronunciations(&self, doc_id: Option<&str>) -> Result<Vec<PronunciationRule>> {
        let mut stmt = self.conn.prepare(
            "SELECT * FROM pronunciations WHERE document_id IS NULL OR document_id = ?1 ORDER BY length(term) DESC",
        )?;
        let v = stmt
            .query_map([doc_id], |r| {
                Ok(PronunciationRule {
                    id: r.get("id")?,
                    term: r.get("term")?,
                    replacement: r.get("replacement")?,
                    case_sensitive: r.get::<_, i64>("case_sensitive")? != 0,
                    document_id: r.get("document_id")?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(v)
    }

    pub fn delete_pronunciation(&self, id: i64) -> Result<()> {
        self.conn.execute("DELETE FROM pronunciations WHERE id = ?1", [id])?;
        Ok(())
    }

    // ---------- voices ----------

    pub fn register_voice(&self, v: &InstalledVoice) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO installed_voices
             (id, engine, name, language, accent, gender, quality, licence, size_bytes, offline, path, installed_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            params![
                v.id, v.engine, v.name, v.language, v.accent, v.gender, v.quality, v.licence,
                v.size_bytes as i64, v.offline as i64, v.path, v.installed_at
            ],
        )?;
        Ok(())
    }

    pub fn installed_voices(&self) -> Result<Vec<InstalledVoice>> {
        let mut stmt = self.conn.prepare("SELECT * FROM installed_voices ORDER BY name")?;
        let v = stmt
            .query_map([], |r| {
                Ok(InstalledVoice {
                    id: r.get("id")?,
                    engine: r.get("engine")?,
                    name: r.get("name")?,
                    language: r.get("language")?,
                    accent: r.get("accent")?,
                    gender: r.get("gender")?,
                    quality: r.get("quality")?,
                    licence: r.get("licence")?,
                    size_bytes: r.get::<_, i64>("size_bytes")? as u64,
                    offline: r.get::<_, i64>("offline")? != 0,
                    path: r.get("path")?,
                    installed_at: r.get("installed_at")?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(v)
    }

    pub fn unregister_voice(&self, id: &str) -> Result<()> {
        self.conn.execute("DELETE FROM installed_voices WHERE id = ?1", [id])?;
        Ok(())
    }

    // ---------- consent and personal voices ----------

    pub fn record_consent(
        &self,
        subject: &str,
        relation: &str,
        statement: &str,
        processing_location: &str,
    ) -> Result<i64> {
        if relation != "self" && relation != "authorised_other" {
            return Err(ReaditError::Invalid { detail: "Consent relation must be self or authorised_other.".into() });
        }
        if statement.trim().is_empty() || subject.trim().is_empty() {
            return Err(ReaditError::Invalid { detail: "Consent needs a named person and an accepted statement.".into() });
        }
        self.conn.execute(
            "INSERT INTO consent_records (subject_name, relation, statement, processing_location, accepted_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![subject, relation, statement, processing_location, now()],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    /// A voice profile can only be created against a live (unrevoked) consent record.
    pub fn create_voice_profile(&self, name: &str, language: &str, consent_id: i64) -> Result<i64> {
        let live: Option<Option<i64>> = self
            .conn
            .query_row("SELECT revoked_at FROM consent_records WHERE id = ?1", [consent_id], |r| r.get(0))
            .optional()?;
        match live {
            Some(None) => {}
            _ => {
                return Err(ReaditError::Invalid {
                    detail: "A personal voice needs a valid, unrevoked consent record.".into(),
                })
            }
        }
        self.conn.execute(
            "INSERT INTO voice_profiles (name, language, consent_id, created_at) VALUES (?1, ?2, ?3, ?4)",
            params![name, language, consent_id, now()],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    /// Revoking consent disables every profile built on it and returns the
    /// file paths the caller must delete from storage.
    pub fn revoke_consent(&mut self, consent_id: i64) -> Result<Vec<String>> {
        let tx = self.conn.transaction()?;
        tx.execute("UPDATE consent_records SET revoked_at = ?2 WHERE id = ?1", params![consent_id, now()])?;
        let paths = {
            let mut stmt =
                tx.prepare("SELECT recording_path, model_path FROM voice_profiles WHERE consent_id = ?1")?;
            let rows = stmt.query_map([consent_id], |r| {
                Ok((r.get::<_, Option<String>>(0)?, r.get::<_, Option<String>>(1)?))
            })?;
            let mut out = vec![];
            for row in rows {
                let (a, b) = row?;
                out.extend(a);
                out.extend(b);
            }
            out
        };
        tx.execute("DELETE FROM voice_profiles WHERE consent_id = ?1", [consent_id])?;
        tx.commit()?;
        Ok(paths)
    }

    // ---------- reading profiles and settings ----------

    pub fn reading_profiles(&self) -> Result<Vec<ReadingProfile>> {
        let mut stmt = self.conn.prepare("SELECT * FROM reading_profiles ORDER BY id")?;
        let v = stmt
            .query_map([], |r| {
                Ok(ReadingProfile {
                    id: r.get("id")?,
                    name: r.get("name")?,
                    voice_id: r.get("voice_id")?,
                    speed: r.get::<_, f64>("speed")? as f32,
                    pitch: r.get::<_, f64>("pitch")? as f32,
                    paragraph_pause_ms: r.get("paragraph_pause_ms")?,
                    heading_pause_ms: r.get("heading_pause_ms")?,
                    read_speaker_notes: r.get::<_, i64>("read_speaker_notes")? != 0,
                    read_slide_content: r.get::<_, i64>("read_slide_content")? != 0,
                    skip_headers_footers: r.get::<_, i64>("skip_headers_footers")? != 0,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(v)
    }

    pub fn save_reading_profile(&self, p: &ReadingProfile) -> Result<()> {
        let speed = p.speed.clamp(0.5, 3.0) as f64;
        self.conn.execute(
            "UPDATE reading_profiles SET name = ?2, voice_id = ?3, speed = ?4, pitch = ?5,
               paragraph_pause_ms = ?6, heading_pause_ms = ?7, read_speaker_notes = ?8,
               read_slide_content = ?9, skip_headers_footers = ?10 WHERE id = ?1",
            params![
                p.id, p.name, p.voice_id, speed, p.pitch as f64, p.paragraph_pause_ms,
                p.heading_pause_ms, p.read_speaker_notes as i64, p.read_slide_content as i64,
                p.skip_headers_footers as i64
            ],
        )?;
        Ok(())
    }

    pub fn setting(&self, key: &str) -> Result<Option<String>> {
        Ok(self
            .conn
            .query_row("SELECT value FROM settings WHERE key = ?1", [key], |r| r.get(0))
            .optional()?)
    }

    pub fn set_setting(&self, key: &str, value: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = ?2",
            params![key, value],
        )?;
        Ok(())
    }

    // ---------- audio cache ----------

    pub fn cache_audio(&self, segment_id: &str, voice: &str, speed: f32, path: &str, size: u64) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO audio_cache (segment_id, voice_id, speed, path, size_bytes, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![segment_id, voice, speed as f64, path, size as i64, now()],
        )?;
        Ok(())
    }

    pub fn cached_audio(&self, segment_id: &str, voice: &str, speed: f32) -> Result<Option<String>> {
        Ok(self
            .conn
            .query_row(
                "SELECT path FROM audio_cache WHERE segment_id = ?1 AND voice_id = ?2 AND abs(speed - ?3) < 0.001",
                params![segment_id, voice, speed as f64],
                |r| r.get(0),
            )
            .optional()?)
    }

    pub fn audio_cache_bytes(&self) -> Result<u64> {
        Ok(self
            .conn
            .query_row("SELECT COALESCE(SUM(size_bytes), 0) FROM audio_cache", [], |r| r.get::<_, i64>(0))?
            as u64)
    }

    /// Clears cache records and returns the generated audio files to delete.
    /// Source documents are never included.
    pub fn clear_audio_cache(&self) -> Result<Vec<String>> {
        let mut stmt = self.conn.prepare("SELECT path FROM audio_cache")?;
        let paths = stmt.query_map([], |r| r.get(0))?.collect::<rusqlite::Result<Vec<String>>>()?;
        self.conn.execute("DELETE FROM audio_cache", [])?;
        Ok(paths)
    }
}
