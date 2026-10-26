//! Player tests against the real core library and sample documents.
//! Same scenarios as android/jvmtest/ReaderControllerTest.kt.

use crate::player::*;
use crate::source::CoreSource;
use parking_lot::Mutex;
use readit_core::model::{Section, SectionKind, Segment};
use readit_core::segment::ReadingOptions;
use readit_core::ReaditLibrary;
use std::sync::Arc;
use std::time::{Duration, Instant};

struct FakeEngine {
    id: String,
    spoken: Arc<Mutex<Vec<(String, f32)>>>,
}

impl SpeechEngine for FakeEngine {
    fn voice_id(&self) -> &str {
        &self.id
    }
    fn sample_rate(&self) -> u32 {
        8000
    }
    fn synthesize(&mut self, text: &str, speed: f32) -> Result<Vec<f32>, String> {
        self.spoken.lock().push((text.to_string(), speed));
        let n = ((text.len() as f32 * 0.03 * 8000.0 / speed) as usize).max(1);
        Ok((0..n).map(|i| (i as f32 * 0.1).sin() * 0.2).collect())
    }
}

/// Plays "in real time" sped up 40x.
#[derive(Default)]
struct TimedSink {
    paused: std::sync::atomic::AtomicBool,
}

impl AudioSink for TimedSink {
    fn open(&self, _r: u32) -> Result<(), String> {
        Ok(())
    }
    fn write(&self, s: &[f32]) {
        while self.paused.load(std::sync::atomic::Ordering::SeqCst) {
            std::thread::sleep(Duration::from_millis(2));
        }
        std::thread::sleep(Duration::from_micros(s.len() as u64 * 1_000_000 / 8000 / 40));
    }
    fn pause(&self) {
        self.paused.store(true, std::sync::atomic::Ordering::SeqCst)
    }
    fn resume(&self) {
        self.paused.store(false, std::sync::atomic::Ordering::SeqCst)
    }
    fn flush(&self) {}
}

#[derive(Default)]
struct Rec {
    heard: Mutex<Vec<Segment>>,
    sections: Mutex<Vec<Section>>,
    states: Mutex<Vec<PlayState>>,
    slept: std::sync::atomic::AtomicBool,
}

impl Listener for Rec {
    fn state_changed(&self, s: PlayState) {
        self.states.lock().push(s)
    }
    fn segment_started(&self, section: &Section, seg: &Segment) {
        self.heard.lock().push(seg.clone());
        self.sections.lock().push(section.clone());
    }
    fn sleep_finished(&self) {
        self.slept.store(true, std::sync::atomic::Ordering::SeqCst)
    }
}

fn wait(mut f: impl FnMut() -> bool) -> bool {
    let end = Instant::now() + Duration::from_secs(15);
    while Instant::now() < end {
        if f() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    f()
}

fn sample(name: &str) -> String {
    format!("{}/../../samples/{name}", env!("CARGO_MANIFEST_DIR"))
}

struct Fixture {
    lib: Arc<ReaditLibrary>,
    rec: Arc<Rec>,
    player: Arc<Player>,
    spoken: Arc<Mutex<Vec<(String, f32)>>>,
    _dir: tempfile::TempDir,
}

fn fixture() -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let lib = ReaditLibrary::new(dir.path().to_string_lossy().into_owned(), std::env::var("READIT_PDFIUM_DIR").ok()).unwrap();
    let rec = Arc::new(Rec::default());
    let player = Player::new(Arc::new(CoreSource(lib.clone())), Arc::new(TimedSink::default()), rec.clone());
    let spoken = Arc::new(Mutex::new(vec![]));
    player.set_engine(Box::new(FakeEngine { id: "fake".into(), spoken: spoken.clone() }));
    Fixture { lib, rec, player, spoken, _dir: dir }
}

fn import(f: &Fixture, name: &str) -> readit_core::model::Document {
    f.lib.import_file(sample(name), None).unwrap().document
}

#[test]
fn reads_whole_document_in_order_and_saves_position() {
    let f = fixture();
    let doc = import(&f, "kyc-basics.docx");
    let o = ReadingOptions::default();
    let expected: Vec<String> = (0..doc.section_count)
        .flat_map(|i| f.lib.segments(doc.id.clone(), i, o.clone()).unwrap())
        .map(|s| s.id)
        .collect();
    f.player.load(&doc.id, o, None);
    f.player.play().unwrap();
    assert!(wait(|| f.player.state() == PlayState::Ended), "states {:?}", f.rec.states.lock());
    let heard: Vec<String> = f.rec.heard.lock().iter().map(|s| s.id.clone()).collect();
    assert_eq!(heard, expected);
    let pos = f.lib.position(doc.id.clone()).unwrap().unwrap();
    assert_eq!(format!("{}:{}:{}", doc.id, pos.section_ordinal, pos.segment_index), *expected.last().unwrap());
    assert!(f.spoken.lock().iter().any(|s| s.0.contains("see-deez")));
    f.player.release();
}

#[test]
fn pause_resume_navigation_and_furniture_on_pdf() {
    let f = fixture();
    let doc = import(&f, "kyc-guide.pdf");
    f.player.load(&doc.id, ReadingOptions::default(), None);
    f.player.play().unwrap();
    assert!(wait(|| !f.rec.sections.lock().is_empty()));
    assert_eq!(f.rec.sections.lock()[0].kind, SectionKind::Heading, "starts at the title, not the running header");

    f.player.pause();
    let n = f.rec.heard.lock().len();
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(f.rec.heard.lock().len(), n, "pause stops progress");
    assert_eq!(f.player.state(), PlayState::Paused);
    f.player.play().unwrap();
    assert!(wait(|| f.rec.heard.lock().len() > n), "resume continues");

    let before = f.player.current().unwrap().section;
    f.player.next_paragraph();
    assert!(wait(|| f.rec.sections.lock().last().unwrap().ordinal > before));

    f.player.go_to_location(3);
    assert!(wait(|| f.rec.sections.lock().last().unwrap().location == 3));
    f.player.previous_location();
    assert!(wait(|| f.rec.sections.lock().last().unwrap().location == 2));
    assert!(f.rec.sections.lock().iter().all(|s| s.kind != SectionKind::PageFurniture), "headers and page numbers not spoken");

    let cur = f.player.current().unwrap();
    f.player.set_speed(1.5);
    assert!(wait(|| f.spoken.lock().iter().any(|s| s.1 == 1.5)));
    assert!(f.player.current().unwrap().section >= cur.section, "speed change keeps the place");
    f.player.set_speed(9.0);
    assert_eq!(f.player.speed(), 3.0);
    f.player.release();
}

#[test]
fn resume_and_skip_back_across_sentences() {
    let f = fixture();
    let doc = import(&f, "kyc-basics.docx");
    f.player.load(&doc.id, ReadingOptions::default(), Some(Position { section: 1, segment: 2 }));
    f.player.play().unwrap();
    assert!(wait(|| !f.rec.heard.lock().is_empty()));
    let first = f.rec.heard.lock()[0].clone();
    assert_eq!((first.section_ordinal, first.index), (1, 2), "resumes at saved sentence");
    std::thread::sleep(Duration::from_millis(30));
    f.player.skip_seconds(-4);
    assert!(wait(|| f.rec.heard.lock().iter().any(|s| s.section_ordinal == 1 && s.index == 1)), "skip back reaches previous sentence");
    f.player.release();
}

#[test]
fn speaker_notes_only() {
    let f = fixture();
    let doc = import(&f, "kyc-training.pptx");
    let o = ReadingOptions { read_speaker_notes: true, read_slide_content: false, ..Default::default() };
    f.player.load(&doc.id, o, None);
    f.player.play().unwrap();
    assert!(wait(|| f.player.state() == PlayState::Ended));
    let s = f.rec.sections.lock();
    assert!(!s.is_empty() && s.iter().all(|x| x.kind == SectionKind::SpeakerNotes));
    f.player.release();
}

#[test]
fn sleep_at_end_of_page_then_resume() {
    let f = fixture();
    let doc = import(&f, "kyc-guide.pdf");
    f.player.load(&doc.id, ReadingOptions::default(), None);
    f.player.play().unwrap();
    assert!(wait(|| !f.rec.heard.lock().is_empty()));
    f.player.sleep_at_end_of_location();
    assert!(wait(|| f.rec.slept.load(std::sync::atomic::Ordering::SeqCst)));
    assert_eq!(f.player.state(), PlayState::Paused);
    assert!(f.rec.sections.lock().iter().all(|s| s.location == 1), "nothing from page 2 was played");
    f.player.play().unwrap();
    assert!(wait(|| f.rec.sections.lock().last().unwrap().location == 2));
    f.player.release();
}

#[test]
fn timed_sleep_pauses() {
    let f = fixture();
    let doc = import(&f, "kyc-guide.pdf");
    f.player.load(&doc.id, ReadingOptions::default(), None);
    f.player.play().unwrap();
    assert!(wait(|| !f.rec.heard.lock().is_empty()));
    f.player.set_sleep_deadline(Duration::from_millis(50));
    assert!(wait(|| f.player.state() == PlayState::Paused));
    f.player.release();
}

#[test]
fn voice_change_keeps_place() {
    let f = fixture();
    let doc = import(&f, "kyc-basics.docx");
    f.player.load(&doc.id, ReadingOptions::default(), Some(Position { section: 3, segment: 0 }));
    f.player.play().unwrap();
    assert!(wait(|| !f.rec.heard.lock().is_empty()));
    let b = Arc::new(Mutex::new(vec![]));
    f.player.set_engine(Box::new(FakeEngine { id: "b".into(), spoken: b.clone() }));
    assert!(wait(|| !b.lock().is_empty()), "new voice takes over");
    assert!(f.player.current().unwrap().section >= 3);
    assert_eq!(f.player.voice_id().as_deref(), Some("b"));
    f.player.release();
}

#[test]
fn play_without_voice_or_document_explains() {
    let dir = tempfile::tempdir().unwrap();
    let lib = ReaditLibrary::new(dir.path().to_string_lossy().into_owned(), None).unwrap();
    let p = Player::new(Arc::new(CoreSource(lib)), Arc::new(TimedSink::default()), Arc::new(Rec::default()));
    assert!(p.play().unwrap_err().contains("Open a document"));
    p.load("x", ReadingOptions::default(), None);
    assert!(p.play().unwrap_err().contains("voice"));
    p.release();
}

/// End-to-end with a real downloaded voice. Run with:
/// READIT_TEST_VOICES=<dir with voice packs> READIT_TEST_VOICE=kokoro:bf_emma READIT_TEST_OUT=out.wav \
///   cargo test e2e -- --ignored --nocapture
#[test]
#[ignore]
fn e2e_real_voice_reads_pdf() {
    let voices_dir = std::env::var("READIT_TEST_VOICES").expect("READIT_TEST_VOICES");
    let voice = std::env::var("READIT_TEST_VOICE").unwrap_or_else(|_| "piper:alba".into());
    let out = std::env::var("READIT_TEST_OUT").unwrap_or_else(|_| "e2e.wav".into());
    let dir = tempfile::tempdir().unwrap();
    let lib = ReaditLibrary::new(dir.path().to_string_lossy().into_owned(), std::env::var("READIT_PDFIUM_DIR").ok()).unwrap();
    let vm = crate::voices::VoiceManager::new(lib.clone(), dir.path(), Some(voices_dir.into()));
    assert_eq!(vm.default_voice().is_some(), true, "bundled voices registered");
    let sink = Arc::new(crate::audio::WavSink::default());
    let rec = Arc::new(Rec::default());
    let player = Player::new(Arc::new(CoreSource(lib.clone())), sink.clone(), rec.clone());
    let t = Instant::now();
    player.set_engine(vm.create_engine(&voice).unwrap());
    let load = t.elapsed();
    let doc = lib.import_file(sample("kyc-guide.pdf"), None).unwrap().document;
    player.load(&doc.id, ReadingOptions::default(), None);
    let t = Instant::now();
    player.play().unwrap();
    let end = Instant::now() + Duration::from_secs(600);
    let mut first = None;
    while player.state() != PlayState::Ended && Instant::now() < end {
        if first.is_none() && !rec.heard.lock().is_empty() {
            first = Some(t.elapsed());
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(player.state(), PlayState::Ended);
    sink.save(std::path::Path::new(&out)).unwrap();
    let secs = sink.samples.lock().len() as f64 / sink.rate.load(std::sync::atomic::Ordering::SeqCst) as f64;
    println!(
        "voice={voice} load={:.1}s first_sentence={:.2}s total={:.1}s audio={:.1}s sentences={}",
        load.as_secs_f64(),
        first.unwrap().as_secs_f64(),
        t.elapsed().as_secs_f64(),
        secs,
        rec.heard.lock().len()
    );
    player.release();
}
