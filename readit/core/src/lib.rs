//! READIT core: document import, extraction, normalisation, segmentation and
//! the local library database. Shared by the Android app (through UniFFI
//! Kotlin bindings) and the desktop app (directly, from Rust).
//!
//! Nothing in this crate opens a network connection.

pub mod db;
pub mod error;
pub mod extract;
pub mod model;
pub mod normalize;
pub mod segment;

use db::Db;
use error::{ReaditError, Result};
use extract::ExtractOptions;
use model::*;
use segment::ReadingOptions;
use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

uniffi::setup_scaffolding!();

#[derive(Debug, Clone, uniffi::Record)]
pub struct ImportResult {
    pub document: Document,
    /// True when this exact file was already in the library.
    pub already_in_library: bool,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct StorageReport {
    pub documents_bytes: u64,
    pub audio_cache_bytes: u64,
    pub voices_bytes: u64,
    pub database_bytes: u64,
}

#[derive(uniffi::Object)]
pub struct ReaditLibrary {
    db: Mutex<Db>,
    data_dir: PathBuf,
    extract_opts: ExtractOptions,
}

fn lock<'a>(m: &'a Mutex<Db>) -> std::sync::MutexGuard<'a, Db> {
    // A panic in another thread must not lock the user out of their library.
    m.lock().unwrap_or_else(|p| p.into_inner())
}

fn file_id(path: &Path) -> Result<String> {
    let mut f = std::fs::File::open(path)?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1 << 16];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(hex::encode(&h.finalize()[..10]))
}

fn dir_size(p: &Path) -> u64 {
    std::fs::read_dir(p)
        .map(|rd| {
            rd.flatten()
                .map(|e| match e.metadata() {
                    Ok(m) if m.is_dir() => dir_size(&e.path()),
                    Ok(m) => m.len(),
                    Err(_) => 0,
                })
                .sum()
        })
        .unwrap_or(0)
}

impl ReaditLibrary {
    pub fn documents_dir(&self) -> PathBuf {
        self.data_dir.join("documents")
    }
    pub fn voices_dir(&self) -> PathBuf {
        self.data_dir.join("voices")
    }
    pub fn cache_dir(&self) -> PathBuf {
        self.data_dir.join("audio-cache")
    }

    /// Direct access for the desktop backend and tests.
    pub fn with_db<T>(&self, f: impl FnOnce(&mut Db) -> T) -> T {
        f(&mut lock(&self.db))
    }
}

#[uniffi::export]
impl ReaditLibrary {
    /// Open (or create) the library. `pdfium_dir` is where libpdfium lives
    /// when it is not on the system library path (always the case on Android).
    #[uniffi::constructor]
    pub fn new(data_dir: String, pdfium_dir: Option<String>) -> Result<Arc<Self>> {
        let data_dir = PathBuf::from(data_dir);
        for d in ["documents", "voices", "audio-cache", "voice-profiles"] {
            std::fs::create_dir_all(data_dir.join(d))?;
        }
        let db = Db::open(&data_dir.join("readit.db"))?;
        Ok(Arc::new(Self { db: Mutex::new(db), data_dir, extract_opts: ExtractOptions { pdfium_dir } }))
    }

    /// Import a file. READIT keeps its own copy in private storage so the
    /// library survives the original being moved; the original is never changed.
    pub fn import_file(&self, path: String, display_name: Option<String>) -> Result<ImportResult> {
        let src = PathBuf::from(&path);
        let meta = std::fs::metadata(&src)?;
        if !meta.is_file() {
            return Err(ReaditError::Invalid { detail: "Only files can be imported.".into() });
        }
        let id = file_id(&src)?;
        if lock(&self.db).document_exists(&id)? {
            let document = lock(&self.db).get_document(&id)?;
            return Ok(ImportResult { document, already_in_library: true });
        }
        let file_name = display_name
            .unwrap_or_else(|| src.file_name().and_then(|n| n.to_str()).unwrap_or("document").to_string());
        // Extract with the original name so extension-based detection works.
        let (format, extracted) = extract::extract(&src, &self.extract_opts)
            .or_else(|e| {
                // Content URIs copied to temp files may lack an extension; retry with the display name's.
                if let (ReaditError::UnsupportedFormat { .. }, Some(ext)) =
                    (&e, Path::new(&file_name).extension().and_then(|x| x.to_str()))
                {
                    let tmp = self.data_dir.join(format!("import-{id}.{ext}"));
                    std::fs::copy(&src, &tmp)?;
                    let r = extract::extract(&tmp, &self.extract_opts);
                    let _ = std::fs::remove_file(&tmp);
                    r
                } else {
                    Err(e)
                }
            })?;
        let ext = Path::new(&file_name)
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_ascii_lowercase())
            .unwrap_or_else(|| format.as_str().to_string());
        let stored = self.documents_dir().join(format!("{id}.{ext}"));
        std::fs::copy(&src, &stored)?;

        let title = extracted
            .title
            .clone()
            .filter(|t| t.chars().count() <= 200)
            .unwrap_or_else(|| {
                Path::new(&file_name).file_stem().and_then(|s| s.to_str()).unwrap_or(&file_name).replace(['_'], " ")
            });
        let doc = Document {
            id: id.clone(),
            title,
            file_name,
            format,
            source_path: stored.to_string_lossy().into_owned(),
            size_bytes: meta.len(),
            location_kind: extracted.location_kind.unwrap_or(LocationKind::None),
            location_count: extracted.location_count,
            section_count: extracted.sections.len() as u32,
            imported_at: db::now(),
            last_opened_at: None,
            favourite: false,
            warnings: extracted.warnings.clone(),
        };
        if let Err(e) = lock(&self.db).insert_document(&doc, &extracted.sections) {
            let _ = std::fs::remove_file(&stored);
            return Err(e);
        }
        let document = lock(&self.db).get_document(&id)?;
        Ok(ImportResult { document, already_in_library: false })
    }

    /// Pages that are images without text, for the OCR flow.
    pub fn pages_needing_ocr(&self, document_id: String) -> Result<Vec<u32>> {
        let doc = lock(&self.db).get_document(&document_id)?;
        let (_, e) = extract::extract(Path::new(&doc.source_path), &self.extract_opts)?;
        Ok(e.pages_needing_ocr)
    }

    pub fn documents(&self, filter: String, query: Option<String>) -> Result<Vec<Document>> {
        lock(&self.db).list_documents(&filter, query.as_deref().filter(|q| !q.trim().is_empty()))
    }

    pub fn document(&self, id: String) -> Result<Document> {
        lock(&self.db).get_document(&id)
    }

    pub fn open_document(&self, id: String) -> Result<Option<ReadingPosition>> {
        let db = lock(&self.db);
        db.mark_opened(&id)?;
        db.position(&id)
    }

    pub fn set_favourite(&self, id: String, favourite: bool) -> Result<()> {
        lock(&self.db).set_favourite(&id, favourite)
    }

    pub fn rename_document(&self, id: String, title: String) -> Result<()> {
        lock(&self.db).rename_document(&id, title.trim())
    }

    /// Remove from the library. Deletes READIT's private copy and generated
    /// audio; the user's original file elsewhere is not touched.
    pub fn remove_document(&self, id: String) -> Result<()> {
        let doc = lock(&self.db).get_document(&id)?;
        lock(&self.db).remove_document(&id)?;
        let stored = PathBuf::from(&doc.source_path);
        if stored.starts_with(self.documents_dir()) {
            let _ = std::fs::remove_file(stored);
        }
        let _ = std::fs::remove_dir_all(self.cache_dir().join(&id));
        Ok(())
    }

    pub fn sections(&self, document_id: String, from: u32, limit: u32) -> Result<Vec<Section>> {
        lock(&self.db).sections(&document_id, from, limit.min(2000))
    }

    pub fn outline(&self, document_id: String) -> Result<Vec<Section>> {
        lock(&self.db).outline(&document_id)
    }

    pub fn search(&self, document_id: String, query: String) -> Result<Vec<Section>> {
        lock(&self.db).search(&document_id, &query, 200)
    }

    /// First section at or after a page, slide or chapter number.
    pub fn section_at_location(&self, document_id: String, location: u32) -> Result<Option<u32>> {
        lock(&self.db).first_section_at(&document_id, location)
    }

    /// Speech segments for one section, or an empty list if the reading
    /// options skip it. Pronunciation rules are applied here.
    pub fn segments(&self, document_id: String, ordinal: u32, options: ReadingOptions) -> Result<Vec<Segment>> {
        let db = lock(&self.db);
        let doc = db.get_document(&document_id)?;
        let section = db.section(&document_id, ordinal)?;
        if !segment::is_read(&section, &options, doc.format == DocFormat::Pptx) {
            return Ok(vec![]);
        }
        let rules = db.pronunciations(Some(&document_id))?;
        Ok(segment::segments_for(&section, &rules, &options))
    }

    /// Next (or previous) section that will actually be read, for automatic
    /// continuation and paragraph skipping.
    pub fn next_readable(
        &self,
        document_id: String,
        from_ordinal: u32,
        forward: bool,
        options: ReadingOptions,
    ) -> Result<Option<u32>> {
        let db = lock(&self.db);
        let doc = db.get_document(&document_id)?;
        let deck = doc.format == DocFormat::Pptx;
        if forward {
            let mut from = from_ordinal + 1;
            loop {
                let batch = db.sections(&document_id, from, 200)?;
                if batch.is_empty() {
                    return Ok(None);
                }
                if let Some(s) = batch.iter().find(|s| segment::is_read(s, &options, deck)) {
                    return Ok(Some(s.ordinal));
                }
                from = batch.last().unwrap().ordinal + 1;
            }
        } else {
            let mut o = from_ordinal;
            while o > 0 {
                o -= 1;
                let s = db.section(&document_id, o)?;
                if segment::is_read(&s, &options, deck) {
                    return Ok(Some(o));
                }
            }
            Ok(None)
        }
    }

    pub fn save_position(&self, document_id: String, section_ordinal: u32, segment_index: u32) -> Result<()> {
        lock(&self.db).save_position(&document_id, section_ordinal, segment_index)
    }

    pub fn position(&self, document_id: String) -> Result<Option<ReadingPosition>> {
        lock(&self.db).position(&document_id)
    }

    pub fn log_listening(&self, document_id: String, seconds: u32) -> Result<()> {
        lock(&self.db).log_listening(&document_id, seconds)
    }

    pub fn add_bookmark(
        &self,
        document_id: String,
        section_ordinal: u32,
        segment_index: u32,
        label: String,
        note: Option<String>,
        highlight_text: Option<String>,
    ) -> Result<i64> {
        lock(&self.db).add_bookmark(&document_id, section_ordinal, segment_index, &label, note.as_deref(), highlight_text.as_deref())
    }

    pub fn bookmarks(&self, document_id: String) -> Result<Vec<Bookmark>> {
        lock(&self.db).bookmarks(&document_id)
    }

    pub fn update_bookmark_note(&self, id: i64, note: Option<String>) -> Result<()> {
        lock(&self.db).update_bookmark_note(id, note.as_deref())
    }

    pub fn delete_bookmark(&self, id: i64) -> Result<()> {
        lock(&self.db).delete_bookmark(id)
    }

    pub fn queue(&self) -> Result<Vec<String>> {
        lock(&self.db).queue()
    }

    pub fn set_queue(&self, document_ids: Vec<String>) -> Result<()> {
        lock(&self.db).set_queue(&document_ids)
    }

    pub fn collections(&self) -> Result<Vec<Collection>> {
        lock(&self.db).collections()
    }

    pub fn create_collection(&self, name: String) -> Result<i64> {
        lock(&self.db).create_collection(name.trim())
    }

    pub fn add_to_collection(&self, collection_id: i64, document_id: String) -> Result<()> {
        lock(&self.db).add_to_collection(collection_id, &document_id)
    }

    pub fn pronunciations(&self, document_id: Option<String>) -> Result<Vec<PronunciationRule>> {
        lock(&self.db).pronunciations(document_id.as_deref())
    }

    pub fn add_pronunciation(
        &self,
        term: String,
        replacement: String,
        case_sensitive: bool,
        document_id: Option<String>,
    ) -> Result<i64> {
        lock(&self.db).add_pronunciation(term.trim(), replacement.trim(), case_sensitive, document_id.as_deref())
    }

    pub fn delete_pronunciation(&self, id: i64) -> Result<()> {
        lock(&self.db).delete_pronunciation(id)
    }

    /// Preview what the speech engine will receive for a piece of text.
    pub fn preview_spoken(&self, text: String, document_id: Option<String>, options: ReadingOptions) -> Result<String> {
        let rules = lock(&self.db).pronunciations(document_id.as_deref())?;
        let o = normalize::NormalizeOptions {
            spell_acronyms: options.spell_acronyms,
            day_first_dates: options.day_first_dates,
            spell_mode: options.spell_mode,
        };
        Ok(normalize::normalize(&text, &rules, &o))
    }

    pub fn register_voice(&self, voice: InstalledVoice) -> Result<()> {
        lock(&self.db).register_voice(&voice)
    }

    pub fn installed_voices(&self) -> Result<Vec<InstalledVoice>> {
        lock(&self.db).installed_voices()
    }

    /// Removes the voice record and its files (only inside READIT's voices folder).
    pub fn remove_voice(&self, id: String) -> Result<()> {
        let voices = lock(&self.db).installed_voices()?;
        if let Some(v) = voices.iter().find(|v| v.id == id) {
            let p = PathBuf::from(&v.path);
            // Several voices can share one model folder (Kokoro); keep it while any still uses it.
            let shared = voices.iter().any(|o| o.id != id && o.path == v.path);
            if !shared && p.starts_with(self.voices_dir()) {
                let _ = if p.is_dir() { std::fs::remove_dir_all(&p) } else { std::fs::remove_file(&p) };
            }
        }
        lock(&self.db).unregister_voice(&id)
    }

    pub fn record_consent(
        &self,
        subject_name: String,
        relation: String,
        statement: String,
        processing_location: String,
    ) -> Result<i64> {
        lock(&self.db).record_consent(&subject_name, &relation, &statement, &processing_location)
    }

    pub fn create_voice_profile(&self, name: String, language: String, consent_id: i64) -> Result<i64> {
        lock(&self.db).create_voice_profile(&name, &language, consent_id)
    }

    /// Revokes consent and deletes the recordings and models built on it.
    pub fn revoke_consent(&self, consent_id: i64) -> Result<u32> {
        let paths = lock(&self.db).revoke_consent(consent_id)?;
        let mut deleted = 0;
        for p in paths {
            let p = PathBuf::from(p);
            if p.starts_with(&self.data_dir) && std::fs::remove_file(&p).is_ok() {
                deleted += 1;
            }
        }
        Ok(deleted)
    }

    pub fn reading_profiles(&self) -> Result<Vec<ReadingProfile>> {
        lock(&self.db).reading_profiles()
    }

    pub fn save_reading_profile(&self, profile: ReadingProfile) -> Result<()> {
        lock(&self.db).save_reading_profile(&profile)
    }

    pub fn setting(&self, key: String) -> Result<Option<String>> {
        lock(&self.db).setting(&key)
    }

    pub fn set_setting(&self, key: String, value: String) -> Result<()> {
        lock(&self.db).set_setting(&key, &value)
    }

    /// Path where generated audio for a segment should be cached.
    pub fn cache_path(&self, segment_id: String, voice_id: String, speed: f32) -> String {
        let doc = segment_id.split(':').next().unwrap_or("misc");
        let safe: String = format!("{segment_id}_{voice_id}_{speed:.2}")
            .chars()
            .map(|c| if c.is_alphanumeric() || c == '.' || c == '_' { c } else { '_' })
            .collect();
        let dir = self.cache_dir().join(doc);
        let _ = std::fs::create_dir_all(&dir);
        dir.join(format!("{safe}.wav")).to_string_lossy().into_owned()
    }

    pub fn record_cached_audio(&self, segment_id: String, voice_id: String, speed: f32, path: String) -> Result<()> {
        let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
        lock(&self.db).cache_audio(&segment_id, &voice_id, speed, &path, size)
    }

    pub fn cached_audio(&self, segment_id: String, voice_id: String, speed: f32) -> Result<Option<String>> {
        let p = lock(&self.db).cached_audio(&segment_id, &voice_id, speed)?;
        Ok(p.filter(|p| Path::new(p).exists()))
    }

    /// Deletes generated audio only. Documents and voices are untouched.
    pub fn clear_audio_cache(&self) -> Result<u64> {
        let before = dir_size(&self.cache_dir());
        lock(&self.db).clear_audio_cache()?;
        let _ = std::fs::remove_dir_all(self.cache_dir());
        std::fs::create_dir_all(self.cache_dir())?;
        Ok(before)
    }

    pub fn storage_report(&self) -> StorageReport {
        StorageReport {
            documents_bytes: dir_size(&self.documents_dir()),
            audio_cache_bytes: dir_size(&self.cache_dir()),
            voices_bytes: dir_size(&self.voices_dir()),
            database_bytes: ["readit.db", "readit.db-wal", "readit.db-shm"]
                .iter()
                .map(|f| std::fs::metadata(self.data_dir.join(f)).map(|m| m.len()).unwrap_or(0))
                .sum(),
        }
    }

    /// Erase every piece of READIT data on this device: library, copies,
    /// audio, voices, personal voice recordings, settings.
    pub fn erase_all_data(&self) -> Result<()> {
        let mut db = lock(&self.db);
        for d in ["documents", "voices", "audio-cache", "voice-profiles"] {
            let _ = std::fs::remove_dir_all(self.data_dir.join(d));
            std::fs::create_dir_all(self.data_dir.join(d))?;
        }
        let path = self.data_dir.join("readit.db");
        *db = Db::open_in_memory()?; // release the file handle
        for f in ["readit.db", "readit.db-wal", "readit.db-shm"] {
            let _ = std::fs::remove_file(self.data_dir.join(f));
        }
        *db = Db::open(&path)?;
        Ok(())
    }
}

/// Schema version, for diagnostics screens.
#[uniffi::export]
pub fn core_version() -> String {
    format!("readit-core {} (schema {})", env!("CARGO_PKG_VERSION"), Db::open_in_memory().and_then(|d| d.schema_version()).unwrap_or(0))
}
