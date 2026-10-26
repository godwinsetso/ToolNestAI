// Hide the console window on Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod audio;
mod player;
#[cfg(test)]
mod player_tests;
mod source;
mod voices;

use audio::DeviceSink;
use parking_lot::Mutex;
use player::{Listener, PlayState, Player, Position};
use readit_core::model::*;
use readit_core::segment::{CodeMode, ReadingOptions};
use readit_core::{ReaditLibrary, StorageReport};
use serde::Serialize;
use source::CoreSource;
use std::path::PathBuf;
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager, State};
use voices::{DownloadState, VoiceManager};

type CmdResult<T> = Result<T, String>;

fn err<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

// ------------------------------------------------------------------ state

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
struct NowView {
    document: Option<DocView>,
    state: Option<PlayState>,
    section: Option<SectionView>,
    segment: Option<SegmentView>,
    voice_id: Option<String>,
    speed: f32,
    sleep_remaining_sec: Option<u64>,
    sleep_end_of_location: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct DocView {
    id: String,
    title: String,
    file_name: String,
    format: String,
    size_bytes: u64,
    location_kind: String,
    location_count: u32,
    section_count: u32,
    imported_at: i64,
    last_opened_at: Option<i64>,
    favourite: bool,
    warnings: Vec<String>,
}

impl From<&Document> for DocView {
    fn from(d: &Document) -> Self {
        Self {
            id: d.id.clone(),
            title: d.title.clone(),
            file_name: d.file_name.clone(),
            format: d.format.as_str().into(),
            size_bytes: d.size_bytes,
            location_kind: d.location_kind.as_str().into(),
            location_count: d.location_count,
            section_count: d.section_count,
            imported_at: d.imported_at,
            last_opened_at: d.last_opened_at,
            favourite: d.favourite,
            warnings: d.warnings.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct SectionView {
    ordinal: u32,
    kind: String,
    level: u8,
    text: String,
    location: u32,
}

impl From<&Section> for SectionView {
    fn from(s: &Section) -> Self {
        Self { ordinal: s.ordinal, kind: s.kind.as_str().into(), level: s.level, text: s.text.clone(), location: s.location }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct SegmentView {
    section_ordinal: u32,
    index: u32,
    /// UTF-16 offsets: what JavaScript strings use.
    start: u32,
    end: u32,
    text: String,
}

impl From<&Segment> for SegmentView {
    fn from(s: &Segment) -> Self {
        Self { section_ordinal: s.section_ordinal, index: s.index, start: s.start_utf16, end: s.end_utf16, text: s.display_text.clone() }
    }
}

struct App {
    lib: Arc<ReaditLibrary>,
    player: Arc<Player>,
    voices: Arc<VoiceManager>,
    sink: Arc<DeviceSink>,
    now: Arc<Mutex<NowView>>,
    startup_files: Mutex<Vec<String>>,
}

/// Forwards player events to the window.
struct EventBridge {
    app: AppHandle,
    now: Arc<Mutex<NowView>>,
}

impl Listener for EventBridge {
    fn state_changed(&self, s: PlayState) {
        self.now.lock().state = Some(s);
        let _ = self.app.emit("player-state", s);
    }
    fn segment_started(&self, section: &Section, segment: &Segment) {
        let (sv, gv) = (SectionView::from(section), SegmentView::from(segment));
        {
            let mut n = self.now.lock();
            n.section = Some(sv.clone());
            n.segment = Some(gv.clone());
        }
        let _ = self.app.emit("player-segment", serde_json::json!({ "section": sv, "segment": gv }));
    }
    fn error(&self, message: &str) {
        let _ = self.app.emit("player-error", message);
    }
    fn sleep_finished(&self) {
        let _ = self.app.emit("player-message", "Sleep timer: reading paused.");
    }
}

impl App {
    fn snapshot(&self) -> NowView {
        let mut n = self.now.lock().clone();
        n.state = Some(self.player.state());
        n.speed = self.player.speed();
        n.voice_id = self.player.voice_id();
        n.sleep_remaining_sec = self.player.sleep_remaining().map(|d| d.as_secs());
        n.sleep_end_of_location = self.player.sleeps_at_end_of_location();
        n
    }

    fn active_profile(&self) -> Option<ReadingProfile> {
        let name = self.lib.setting("active_profile".into()).ok().flatten().unwrap_or_else(|| "My Reader".into());
        self.lib.reading_profiles().ok()?.into_iter().find(|p| p.name == name)
    }

    fn reading_options(&self) -> ReadingOptions {
        let p = self.active_profile();
        ReadingOptions {
            skip_page_furniture: p.as_ref().map(|p| p.skip_headers_footers).unwrap_or(true),
            read_slide_content: p.as_ref().map(|p| p.read_slide_content).unwrap_or(true),
            read_speaker_notes: p.as_ref().map(|p| p.read_speaker_notes).unwrap_or(false),
            paragraph_pause_ms: p.as_ref().map(|p| p.paragraph_pause_ms).unwrap_or(350),
            heading_pause_ms: p.as_ref().map(|p| p.heading_pause_ms).unwrap_or(700),
            spell_mode: self.lib.setting("spell_mode".into()).ok().flatten().as_deref() == Some("1"),
            code_mode: CodeMode::Announce,
            ..Default::default()
        }
    }

    /// Load the chosen voice, or the best installed one, into the player.
    fn ensure_voice(&self) -> Result<(), String> {
        if self.player.voice_id().is_some() {
            return Ok(());
        }
        let saved = self.lib.setting("voice".into()).ok().flatten();
        let candidates: Vec<String> = saved.into_iter().chain(self.voices.default_voice()).collect();
        for id in candidates {
            if let Ok(e) = self.voices.create_engine(&id) {
                self.player.set_engine(e);
                return Ok(());
            }
        }
        Err("No voice is installed yet. Open Voices and download one; it then works offline.".into())
    }
}

// ------------------------------------------------------------------ library

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ImportOutcome {
    name: String,
    document: Option<DocView>,
    already_in_library: bool,
    error: Option<String>,
}

#[tauri::command]
fn library(app: State<Arc<App>>, filter: String, query: Option<String>) -> CmdResult<Vec<DocView>> {
    Ok(app.lib.documents(filter, query).map_err(err)?.iter().map(DocView::from).collect())
}

#[tauri::command]
async fn import_files(app: State<'_, Arc<App>>, paths: Vec<String>) -> CmdResult<Vec<ImportOutcome>> {
    let lib = app.lib.clone();
    tauri::async_runtime::spawn_blocking(move || {
        paths
            .into_iter()
            .map(|p| {
                let name = PathBuf::from(&p).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or(p.clone());
                match lib.import_file(p, None) {
                    Ok(r) => ImportOutcome { name, document: Some(DocView::from(&r.document)), already_in_library: r.already_in_library, error: None },
                    Err(e) => ImportOutcome { name, document: None, already_in_library: false, error: Some(e.to_string()) },
                }
            })
            .collect()
    })
    .await
    .map_err(err)
}

#[tauri::command]
fn startup_files(app: State<Arc<App>>) -> Vec<String> {
    std::mem::take(&mut *app.startup_files.lock())
}

#[tauri::command]
fn sections(app: State<Arc<App>>, id: String, from: u32, limit: u32) -> CmdResult<Vec<SectionView>> {
    Ok(app.lib.sections(id, from, limit).map_err(err)?.iter().map(SectionView::from).collect())
}

#[tauri::command]
fn outline(app: State<Arc<App>>, id: String) -> CmdResult<Vec<SectionView>> {
    Ok(app.lib.outline(id).map_err(err)?.iter().map(SectionView::from).collect())
}

#[tauri::command]
fn search(app: State<Arc<App>>, id: String, query: String) -> CmdResult<Vec<SectionView>> {
    Ok(app.lib.search(id, query).map_err(err)?.iter().map(SectionView::from).collect())
}

#[tauri::command]
fn set_favourite(app: State<Arc<App>>, id: String, favourite: bool) -> CmdResult<()> {
    app.lib.set_favourite(id, favourite).map_err(err)
}

#[tauri::command]
fn rename_document(app: State<Arc<App>>, id: String, title: String) -> CmdResult<()> {
    app.lib.rename_document(id, title).map_err(err)
}

#[tauri::command]
fn remove_document(app: State<Arc<App>>, id: String) -> CmdResult<()> {
    if app.player.document().as_deref() == Some(id.as_str()) {
        app.player.stop();
        *app.now.lock() = NowView::default();
    }
    app.lib.remove_document(id).map_err(err)
}

#[tauri::command]
fn collections(app: State<Arc<App>>) -> CmdResult<Vec<serde_json::Value>> {
    Ok(app
        .lib
        .collections()
        .map_err(err)?
        .into_iter()
        .map(|c| serde_json::json!({"id": c.id, "name": c.name, "documentCount": c.document_count}))
        .collect())
}

#[tauri::command]
fn add_to_collection(app: State<Arc<App>>, name: String, id: String) -> CmdResult<()> {
    let existing = app.lib.collections().map_err(err)?.into_iter().find(|c| c.name == name).map(|c| c.id);
    let cid = match existing {
        Some(c) => c,
        None => app.lib.create_collection(name).map_err(err)?,
    };
    app.lib.add_to_collection(cid, id).map_err(err)
}

// ------------------------------------------------------------------ player

#[tauri::command]
async fn open_document(app: State<'_, Arc<App>>, id: String) -> CmdResult<NowView> {
    let app = app.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let doc = app.lib.document(id.clone()).map_err(err)?;
        let pos = app.lib.open_document(id.clone()).map_err(err)?;
        app.player.load(&id, app.reading_options(), pos.map(|p| Position { section: p.section_ordinal, segment: p.segment_index }));
        let cur = app.player.current();
        let section = cur.and_then(|c| app.lib.sections(id.clone(), c.section, 1).ok()?.into_iter().next());
        {
            let mut n = app.now.lock();
            n.document = Some(DocView::from(&doc));
            n.section = section.as_ref().map(SectionView::from);
            n.segment = None;
        }
        let _ = app.ensure_voice();
        Ok(app.snapshot())
    })
    .await
    .map_err(err)?
}

#[tauri::command]
fn now(app: State<Arc<App>>) -> NowView {
    app.snapshot()
}

#[tauri::command]
async fn play(app: State<'_, Arc<App>>) -> CmdResult<()> {
    let app = app.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        app.ensure_voice()?;
        app.player.play()
    })
    .await
    .map_err(err)?
}

#[tauri::command]
fn pause(app: State<Arc<App>>) {
    app.player.pause()
}

#[tauri::command]
async fn toggle(app: State<'_, Arc<App>>) -> CmdResult<()> {
    let app = app.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        if matches!(app.player.state(), PlayState::Playing | PlayState::Preparing) {
            app.player.pause();
            Ok(())
        } else {
            app.ensure_voice()?;
            app.player.play()
        }
    })
    .await
    .map_err(err)?
}

#[tauri::command]
fn stop(app: State<Arc<App>>) {
    app.player.stop()
}

/// One command for all movement, so the interface stays simple.
#[tauri::command]
fn navigate(app: State<Arc<App>>, action: String, value: Option<i64>) -> CmdResult<()> {
    let p = &app.player;
    match action.as_str() {
        "nextParagraph" => p.next_paragraph(),
        "previousParagraph" => p.previous_paragraph(),
        "nextLocation" => p.next_location(),
        "previousLocation" => p.previous_location(),
        "goToLocation" => p.go_to_location(value.unwrap_or(1).max(1) as u32),
        "seekSection" => p.seek(Position { section: value.unwrap_or(0).max(0) as u32, segment: 0 }),
        "skip" => p.skip_seconds(value.unwrap_or(15) as i32),
        "restartSentence" => p.restart_sentence(),
        other => return Err(format!("Unknown action {other}")),
    }
    Ok(())
}

#[tauri::command]
fn seek_to(app: State<Arc<App>>, section: u32, segment: u32) {
    app.player.seek(Position { section, segment })
}

#[tauri::command]
fn set_speed(app: State<Arc<App>>, speed: f32) -> CmdResult<f32> {
    app.player.set_speed(speed);
    let s = app.player.speed();
    app.lib.set_setting("speed".into(), s.to_string()).map_err(err)?;
    Ok(s)
}

#[tauri::command]
fn set_volume(app: State<Arc<App>>, volume: f32) -> CmdResult<()> {
    app.sink.set_volume(volume);
    app.lib.set_setting("volume".into(), volume.to_string()).map_err(err)
}

#[tauri::command]
fn sleep_timer(app: State<Arc<App>>, minutes: Option<u32>, end_of_location: bool) {
    if end_of_location {
        app.player.sleep_at_end_of_location()
    } else {
        app.player.set_sleep_minutes(minutes)
    }
}

#[tauri::command]
fn add_bookmark(app: State<Arc<App>>, note: Option<String>) -> CmdResult<i64> {
    let n = app.now.lock().clone();
    let doc = n.document.ok_or("Open a document first.")?;
    let pos = app.player.current().ok_or("Nothing to bookmark yet.")?;
    let text = n.segment.filter(|s| s.section_ordinal == pos.section && s.index == pos.segment).map(|s| s.text);
    let label = text.clone().or(n.section.map(|s| s.text)).unwrap_or_default().chars().take(90).collect::<String>();
    app.lib.add_bookmark(doc.id, pos.section, pos.segment, label, note, text).map_err(err)
}

#[tauri::command]
fn bookmarks(app: State<Arc<App>>, id: String) -> CmdResult<Vec<serde_json::Value>> {
    Ok(app
        .lib
        .bookmarks(id)
        .map_err(err)?
        .into_iter()
        .map(|b| {
            serde_json::json!({"id": b.id, "section": b.section_ordinal, "segment": b.segment_index,
                "label": b.label, "note": b.note, "createdAt": b.created_at})
        })
        .collect())
}

#[tauri::command]
fn update_bookmark(app: State<Arc<App>>, id: i64, note: Option<String>) -> CmdResult<()> {
    app.lib.update_bookmark_note(id, note.filter(|n| !n.trim().is_empty())).map_err(err)
}

#[tauri::command]
fn delete_bookmark(app: State<Arc<App>>, id: i64) -> CmdResult<()> {
    app.lib.delete_bookmark(id).map_err(err)
}

// ------------------------------------------------------------------ voices

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PackView {
    #[serde(flatten)]
    pack: voices::VoicePack,
    state: DownloadState,
    rtf: std::collections::HashMap<String, f64>,
}

#[tauri::command]
fn voices(app: State<Arc<App>>) -> Vec<PackView> {
    app.voices
        .packs
        .iter()
        .map(|p| PackView {
            pack: p.clone(),
            state: app.voices.state(&p.id),
            rtf: p.voices.iter().filter_map(|v| app.voices.measured_rtf(&v.id).map(|r| (v.id.clone(), r))).collect(),
        })
        .collect()
}

#[tauri::command]
fn free_disk(app: State<Arc<App>>) -> Option<u64> {
    app.voices.free_bytes()
}

#[tauri::command]
async fn select_voice(app: State<'_, Arc<App>>, id: String) -> CmdResult<()> {
    let app = app.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let engine = app.voices.create_engine(&id)?;
        app.player.set_engine(engine);
        app.lib.set_setting("voice".into(), id).map_err(err)
    })
    .await
    .map_err(err)?
}

#[tauri::command]
fn download_voice(handle: AppHandle, app: State<Arc<App>>, pack: String) -> CmdResult<()> {
    let vm = app.voices.clone();
    std::thread::spawn(move || {
        let h = handle.clone();
        let id = pack.clone();
        let _ = vm.install(&pack, &|s| {
            let _ = h.emit("voice-download", serde_json::json!({"pack": id, "state": s}));
        });
    });
    Ok(())
}

#[tauri::command]
fn pause_download(app: State<Arc<App>>, pack: String) {
    app.voices.request_pause(&pack)
}

#[tauri::command]
fn remove_voice_pack(app: State<Arc<App>>, pack: String) -> CmdResult<()> {
    let current = app.player.voice_id();
    let affected = current.as_deref().and_then(|v| app.voices.voice(v)).map(|(p, _)| p.id == pack).unwrap_or(false);
    if affected {
        return Err("This voice is reading right now. Choose another voice first, then remove it.".into());
    }
    app.voices.remove(&pack)
}

#[tauri::command]
async fn test_voice(app: State<'_, Arc<App>>, id: String) -> CmdResult<f64> {
    let vm = app.voices.clone();
    tauri::async_runtime::spawn_blocking(move || vm.benchmark(&id)).await.map_err(err)?
}

/// Render a short sample and return it as a WAV data URL for the page to play.
#[tauri::command]
async fn preview_voice(app: State<'_, Arc<App>>, id: String, text: Option<String>) -> CmdResult<String> {
    let app = app.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let text = text.unwrap_or_else(|| "Hello. This is how I will sound reading your documents. The fee is GH₵ 25.50.".into());
        let spoken = app.lib.preview_spoken(text, None, app.reading_options()).map_err(err)?;
        let mut e = app.voices.create_engine(&id)?;
        let samples = e.synthesize(&spoken, app.player.speed())?;
        let sink = audio::WavSink::default();
        use player::AudioSink;
        sink.open(e.sample_rate())?;
        sink.write(&samples);
        let tmp = std::env::temp_dir().join(format!("readit-preview-{}.wav", std::process::id()));
        sink.save(&tmp).map_err(err)?;
        let bytes = std::fs::read(&tmp).map_err(err)?;
        let _ = std::fs::remove_file(&tmp);
        Ok(format!("data:audio/wav;base64,{}", base64(&bytes)))
    })
    .await
    .map_err(err)?
}

fn base64(b: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut s = String::with_capacity(b.len() * 4 / 3 + 4);
    for c in b.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        s.push(T[(n >> 18) as usize & 63] as char);
        s.push(T[(n >> 12) as usize & 63] as char);
        s.push(if c.len() > 1 { T[(n >> 6) as usize & 63] as char } else { '=' });
        s.push(if c.len() > 2 { T[n as usize & 63] as char } else { '=' });
    }
    s
}

// ------------------------------------------------------------------ settings

#[tauri::command]
fn get_settings(app: State<Arc<App>>) -> serde_json::Value {
    let g = |k: &str| app.lib.setting(k.into()).ok().flatten();
    serde_json::json!({
        "theme": g("theme").unwrap_or_else(|| "system".into()),
        "textScale": g("text_scale").and_then(|v| v.parse::<f32>().ok()).unwrap_or(1.0),
        "highContrast": g("high_contrast").as_deref() == Some("1"),
        "skipSeconds": g("skip_seconds").and_then(|v| v.parse::<i32>().ok()).unwrap_or(15),
        "activeProfile": g("active_profile").unwrap_or_else(|| "My Reader".into()),
        "volume": g("volume").and_then(|v| v.parse::<f32>().ok()).unwrap_or(1.0),
        "spellMode": g("spell_mode").as_deref() == Some("1"),
        "onboardingDone": g("onboarding_done").as_deref() == Some("1"),
    })
}

#[tauri::command]
fn set_setting(app: State<Arc<App>>, key: String, value: String) -> CmdResult<()> {
    const ALLOWED: &[&str] = &["theme", "text_scale", "high_contrast", "skip_seconds", "spell_mode", "onboarding_done"];
    if !ALLOWED.contains(&key.as_str()) {
        return Err(format!("Unknown setting {key}"));
    }
    app.lib.set_setting(key.clone(), value).map_err(err)?;
    if key == "spell_mode" {
        app.player.set_options(app.reading_options());
    }
    Ok(())
}

#[tauri::command]
fn profiles(app: State<Arc<App>>) -> CmdResult<Vec<serde_json::Value>> {
    Ok(app
        .lib
        .reading_profiles()
        .map_err(err)?
        .into_iter()
        .map(|p| {
            serde_json::json!({"id": p.id, "name": p.name, "voiceId": p.voice_id, "speed": p.speed,
                "paragraphPauseMs": p.paragraph_pause_ms, "headingPauseMs": p.heading_pause_ms,
                "readSpeakerNotes": p.read_speaker_notes, "readSlideContent": p.read_slide_content,
                "skipHeadersFooters": p.skip_headers_footers})
        })
        .collect())
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProfileIn {
    id: i64,
    name: String,
    voice_id: Option<String>,
    speed: f32,
    paragraph_pause_ms: u32,
    heading_pause_ms: u32,
    read_speaker_notes: bool,
    read_slide_content: bool,
    skip_headers_footers: bool,
}

#[tauri::command]
fn save_profile(app: State<Arc<App>>, profile: ProfileIn, activate: bool) -> CmdResult<()> {
    let p = ReadingProfile {
        id: profile.id,
        name: profile.name.clone(),
        voice_id: profile.voice_id.clone(),
        speed: profile.speed,
        pitch: 1.0,
        paragraph_pause_ms: profile.paragraph_pause_ms,
        heading_pause_ms: profile.heading_pause_ms,
        read_speaker_notes: profile.read_speaker_notes || !profile.read_slide_content,
        read_slide_content: profile.read_slide_content,
        skip_headers_footers: profile.skip_headers_footers,
    };
    app.lib.save_reading_profile(p).map_err(err)?;
    if activate {
        app.lib.set_setting("active_profile".into(), profile.name).map_err(err)?;
        app.player.set_speed(profile.speed);
        if let Some(v) = profile.voice_id {
            if let Ok(e) = app.voices.create_engine(&v) {
                app.player.set_engine(e);
            }
        }
    }
    app.player.set_options(app.reading_options());
    Ok(())
}

#[tauri::command]
fn pronunciations(app: State<Arc<App>>) -> CmdResult<Vec<serde_json::Value>> {
    Ok(app
        .lib
        .pronunciations(None)
        .map_err(err)?
        .into_iter()
        .map(|r| serde_json::json!({"id": r.id, "term": r.term, "replacement": r.replacement, "caseSensitive": r.case_sensitive}))
        .collect())
}

#[tauri::command]
fn add_pronunciation(app: State<Arc<App>>, term: String, replacement: String, case_sensitive: bool) -> CmdResult<()> {
    app.lib.add_pronunciation(term, replacement, case_sensitive, None).map_err(err)?;
    app.player.set_options(app.reading_options());
    Ok(())
}

#[tauri::command]
fn delete_pronunciation(app: State<Arc<App>>, id: i64) -> CmdResult<()> {
    app.lib.delete_pronunciation(id).map_err(err)?;
    app.player.set_options(app.reading_options());
    Ok(())
}

#[tauri::command]
fn preview_spoken(app: State<Arc<App>>, text: String) -> CmdResult<String> {
    app.lib.preview_spoken(text, None, app.reading_options()).map_err(err)
}

#[tauri::command]
fn storage(app: State<Arc<App>>) -> serde_json::Value {
    let r: StorageReport = app.lib.storage_report();
    serde_json::json!({"documents": r.documents_bytes, "audioCache": r.audio_cache_bytes,
        "voices": r.voices_bytes, "database": r.database_bytes, "free": app.voices.free_bytes()})
}

#[tauri::command]
fn clear_audio_cache(app: State<Arc<App>>) -> CmdResult<u64> {
    app.lib.clear_audio_cache().map_err(err)
}

#[tauri::command]
fn erase_all(app: State<Arc<App>>) -> CmdResult<()> {
    app.player.stop();
    *app.now.lock() = NowView::default();
    for p in app.voices.packs.clone() {
        let _ = app.voices.remove(&p.id);
    }
    app.lib.erase_all_data().map_err(err)
}

#[tauri::command]
fn about() -> serde_json::Value {
    serde_json::json!({
        "version": env!("CARGO_PKG_VERSION"),
        "core": readit_core::core_version(),
        "engine": "sherpa-onnx 1.12.9 (Kokoro, Piper)",
        "pdf": "PDFium chromium/7881",
    })
}

// ------------------------------------------------------------------ setup

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|tauri_app| {
            let data_dir = tauri_app.path().app_data_dir()?;
            std::fs::create_dir_all(&data_dir)?;
            let resources = tauri_app.path().resource_dir().ok().map(|r| r.join("resources"));
            // PDFium: bundled with the installer, or READIT_PDFIUM_DIR during development.
            let pdfium_dir = std::env::var("READIT_PDFIUM_DIR")
                .ok()
                .or_else(|| resources.as_ref().map(|r| r.join("pdfium").to_string_lossy().into_owned()));
            let lib = ReaditLibrary::new(data_dir.to_string_lossy().into_owned(), pdfium_dir).map_err(|e| e.to_string())?;
            let bundled_voices = std::env::var("READIT_BUNDLED_VOICES").ok().map(PathBuf::from).or_else(|| resources.map(|r| r.join("voices")));
            let voices = Arc::new(VoiceManager::new(lib.clone(), &data_dir, bundled_voices));
            let now = Arc::new(Mutex::new(NowView::default()));
            let sink = Arc::new(DeviceSink::default());
            if let Some(v) = lib.setting("volume".into()).ok().flatten().and_then(|v| v.parse().ok()) {
                sink.set_volume(v);
            }
            let player = Player::new(
                Arc::new(CoreSource(lib.clone())),
                sink.clone(),
                Arc::new(EventBridge { app: tauri_app.handle().clone(), now: now.clone() }),
            );
            if let Some(s) = lib.setting("speed".into()).ok().flatten().and_then(|v| v.parse().ok()) {
                player.set_speed(s);
            }
            // Files passed on the command line ("Open with READIT", double-click with a file association).
            let files: Vec<String> = std::env::args().skip(1).filter(|a| !a.starts_with('-') && PathBuf::from(a).is_file()).collect();
            tauri_app.manage(Arc::new(App { lib, player, voices, sink, now, startup_files: Mutex::new(files) }));
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::Destroyed = event {
                if let Some(app) = window.try_state::<Arc<App>>() {
                    app.player.release();
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            library, import_files, startup_files, sections, outline, search, set_favourite, rename_document,
            remove_document, collections, add_to_collection, open_document, now, play, pause, toggle, stop, navigate,
            seek_to, set_speed, set_volume, sleep_timer, add_bookmark, bookmarks, update_bookmark, delete_bookmark,
            voices, free_disk, select_voice, download_voice, pause_download, remove_voice_pack, test_voice,
            preview_voice, get_settings, set_setting, profiles, save_profile, pronunciations, add_pronunciation,
            delete_pronunciation, preview_spoken, storage, clear_audio_cache, erase_all, about
        ])
        .run(tauri::generate_context!())
        .expect("READIT failed to start");
}
