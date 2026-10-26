//! Reads a document aloud, one sentence at a time. The Rust twin of the
//! Android `ReaderController`: same behaviour, same tests.
//!
//! A synthesis thread renders up to `LOOK_AHEAD` sentences ahead; a playback
//! thread writes audio to the sink and reports which sentence is sounding.
//! Every jump increments `generation`, so audio rendered for an old position
//! is discarded rather than played.

use parking_lot::{Condvar, Mutex};
use readit_core::model::{Section, Segment};
use readit_core::segment::ReadingOptions;
use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

pub const LOOK_AHEAD: usize = 3;

pub trait SpeechEngine: Send {
    fn voice_id(&self) -> &str;
    fn sample_rate(&self) -> u32;
    fn synthesize(&mut self, text: &str, speed: f32) -> Result<Vec<f32>, String>;
}

pub trait AudioSink: Send + Sync {
    fn open(&self, sample_rate: u32) -> Result<(), String>;
    /// Blocks until the samples are queued for playback.
    fn write(&self, samples: &[f32]);
    fn pause(&self);
    fn resume(&self);
    /// Drop audio queued but not yet heard.
    fn flush(&self);
}

pub trait DocumentSource: Send + Sync {
    fn segments(&self, doc: &str, ordinal: u32, o: &ReadingOptions) -> Vec<Segment>;
    fn next_readable(&self, doc: &str, from: u32, forward: bool, o: &ReadingOptions) -> Option<u32>;
    fn section_at_location(&self, doc: &str, location: u32) -> Option<u32>;
    fn section(&self, doc: &str, ordinal: u32) -> Option<Section>;
    fn save_position(&self, doc: &str, section: u32, segment: u32);
    fn log_listening(&self, doc: &str, seconds: u32);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum PlayState {
    Idle,
    Preparing,
    Playing,
    Paused,
    Ended,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub struct Position {
    pub section: u32,
    pub segment: u32,
}

pub trait Listener: Send + Sync {
    fn state_changed(&self, _s: PlayState) {}
    fn segment_started(&self, _section: &Section, _segment: &Segment) {}
    fn error(&self, _message: &str) {}
    fn sleep_finished(&self) {}
}

struct Item {
    generation: u64,
    section: Option<Section>,
    segment: Option<Segment>,
    samples: Vec<f32>,
    pause_samples: usize,
    start_offset: usize,
    end: bool,
}

struct Played {
    pos: Position,
    samples: usize,
    rate: u32,
}

struct Inner {
    doc: Option<String>,
    options: ReadingOptions,
    engine: Option<Box<dyn SpeechEngine>>,
    /// Kept separately: the engine itself is lent to the synthesis thread while it works.
    voice: Option<String>,
    engine_busy: bool,
    speed: f32,
    state: PlayState,
    synth_cursor: Option<Position>,
    pending_offset: usize,
    current: Option<Position>,
    queue: VecDeque<Item>,
    history: VecDeque<Played>,
    cache: HashMap<u32, (Section, Vec<Segment>)>,
    played_in_segment: usize,
    segment_len: usize,
    skip_request: Option<usize>,
    sleep_deadline: Option<Instant>,
    sleep_at_end_of: Option<u32>,
    unlogged_samples: u64,
    opened_rate: u32,
    sample_rate: u32,
}

pub struct Player {
    inner: Mutex<Inner>,
    wake: Condvar,
    generation: AtomicU64,
    released: AtomicBool,
    source: Arc<dyn DocumentSource>,
    sink: Arc<dyn AudioSink>,
    listener: Arc<dyn Listener>,
}

impl Player {
    pub fn new(source: Arc<dyn DocumentSource>, sink: Arc<dyn AudioSink>, listener: Arc<dyn Listener>) -> Arc<Self> {
        let p = Arc::new(Self {
            inner: Mutex::new(Inner {
                doc: None,
                options: ReadingOptions::default(),
                engine: None,
                voice: None,
                engine_busy: false,
                speed: 1.0,
                state: PlayState::Idle,
                synth_cursor: None,
                pending_offset: 0,
                current: None,
                queue: VecDeque::new(),
                history: VecDeque::new(),
                cache: HashMap::new(),
                played_in_segment: 0,
                segment_len: 0,
                skip_request: None,
                sleep_deadline: None,
                sleep_at_end_of: None,
                unlogged_samples: 0,
                opened_rate: 0,
                sample_rate: 24000,
            }),
            wake: Condvar::new(),
            generation: AtomicU64::new(0),
            released: AtomicBool::new(false),
            source,
            sink,
            listener,
        });
        let a = p.clone();
        std::thread::Builder::new().name("readit-synth".into()).spawn(move || a.synth_loop()).unwrap();
        let b = p.clone();
        std::thread::Builder::new().name("readit-play".into()).spawn(move || b.play_loop()).unwrap();
        p
    }

    // ------------------------------------------------------------ queries

    pub fn state(&self) -> PlayState {
        self.inner.lock().state
    }
    pub fn current(&self) -> Option<Position> {
        self.inner.lock().current
    }
    pub fn speed(&self) -> f32 {
        self.inner.lock().speed
    }
    pub fn document(&self) -> Option<String> {
        self.inner.lock().doc.clone()
    }
    pub fn voice_id(&self) -> Option<String> {
        self.inner.lock().voice.clone()
    }
    pub fn sleep_remaining(&self) -> Option<Duration> {
        self.inner.lock().sleep_deadline.map(|d| d.saturating_duration_since(Instant::now()))
    }
    pub fn sleeps_at_end_of_location(&self) -> bool {
        self.inner.lock().sleep_at_end_of.is_some()
    }

    // ------------------------------------------------------------ setup

    pub fn load(&self, doc: &str, options: ReadingOptions, start: Option<Position>) {
        let mut g = self.inner.lock();
        self.flush_log(&mut g);
        g.doc = Some(doc.to_string());
        g.options = options;
        g.cache.clear();
        g.history.clear();
        let first = start.or_else(|| self.first_readable(&mut g)).unwrap_or(Position { section: 0, segment: 0 });
        g.current = Some(first);
        self.restart(&mut g, first, 0, false);
        self.set_state(&mut g, PlayState::Idle);
    }

    /// Install a voice. The engine is moved in; playback resumes from the current sentence.
    pub fn set_engine(&self, engine: Box<dyn SpeechEngine>) {
        let mut g = self.inner.lock();
        // Wait for any synthesis in progress to finish with the old engine.
        while g.engine_busy {
            self.wake.wait_for(&mut g, Duration::from_millis(50));
        }
        let changed = g.voice.as_deref() != Some(engine.voice_id());
        g.voice = Some(engine.voice_id().to_string());
        g.sample_rate = engine.sample_rate();
        g.engine = Some(engine);
        if changed {
            if let Some(c) = g.current {
                let keep = self.active(&g);
                self.restart(&mut g, c, 0, keep);
            }
        }
    }

    pub fn set_options(&self, options: ReadingOptions) {
        let mut g = self.inner.lock();
        g.options = options;
        g.cache.clear();
        if let Some(c) = g.current {
            let keep = self.active(&g);
            self.restart(&mut g, c, 0, keep);
        }
    }

    /// Changing speed restarts the current sentence, never the document.
    pub fn set_speed(&self, speed: f32) {
        let mut g = self.inner.lock();
        let v = speed.clamp(0.5, 3.0);
        if (v - g.speed).abs() < 0.001 {
            return;
        }
        g.speed = v;
        if let Some(c) = g.current {
            let keep = self.active(&g);
            self.restart(&mut g, c, 0, keep);
        }
    }

    // ------------------------------------------------------------ transport

    pub fn play(&self) -> Result<(), String> {
        let mut g = self.inner.lock();
        if g.doc.is_none() {
            return Err("Open a document first.".into());
        }
        if g.engine.is_none() {
            return Err("Choose a voice first. Voices > Download gives you one that works offline.".into());
        }
        if g.state == PlayState::Ended {
            if let Some(p) = self.first_readable(&mut g) {
                self.restart(&mut g, p, 0, false);
            }
        }
        if g.state == PlayState::Paused {
            self.sink.resume();
        }
        let next = if g.queue.is_empty() && g.played_in_segment == 0 { PlayState::Preparing } else { PlayState::Playing };
        self.set_state(&mut g, next);
        self.wake.notify_all();
        Ok(())
    }

    pub fn pause(&self) {
        let mut g = self.inner.lock();
        if self.active(&g) {
            self.sink.pause();
            self.set_state(&mut g, PlayState::Paused);
            self.flush_log(&mut g);
            if let Some(c) = g.current {
                self.save(&g, c);
            }
        }
    }

    #[allow(dead_code)]
    pub fn toggle(&self) -> Result<(), String> {
        if self.active(&self.inner.lock()) {
            self.pause();
            Ok(())
        } else {
            self.play()
        }
    }

    pub fn stop(&self) {
        let mut g = self.inner.lock();
        if let Some(c) = g.current {
            self.save(&g, c);
        }
        self.flush_log(&mut g);
        self.generation.fetch_add(1, Ordering::SeqCst);
        g.queue.clear();
        self.sink.flush();
        self.sink.pause();
        self.set_state(&mut g, PlayState::Idle);
        self.wake.notify_all();
    }

    pub fn seek(&self, pos: Position) {
        let mut g = self.inner.lock();
        let keep = self.active(&g);
        self.restart(&mut g, pos, 0, keep);
    }

    pub fn restart_sentence(&self) {
        let mut g = self.inner.lock();
        if let Some(c) = g.current {
            let keep = self.active(&g);
            self.restart(&mut g, c, 0, keep);
        }
    }

    pub fn next_paragraph(&self) {
        let mut g = self.inner.lock();
        let (Some(doc), Some(cur)) = (g.doc.clone(), g.current) else { return };
        if let Some(n) = self.source.next_readable(&doc, cur.section, true, &g.options) {
            let keep = self.active(&g);
            self.restart(&mut g, Position { section: n, segment: 0 }, 0, keep);
        }
    }

    /// Back to the start of this paragraph, or the previous one when already at its start.
    pub fn previous_paragraph(&self) {
        let mut g = self.inner.lock();
        let (Some(doc), Some(cur)) = (g.doc.clone(), g.current) else { return };
        let into = g.played_in_segment as f32 / g.sample_rate.max(1) as f32;
        let target = if cur.segment > 0 || into > 2.0 {
            cur.section
        } else {
            self.source.next_readable(&doc, cur.section, false, &g.options).unwrap_or(cur.section)
        };
        let keep = self.active(&g);
        self.restart(&mut g, Position { section: target, segment: 0 }, 0, keep);
    }

    pub fn go_to_location(&self, location: u32) {
        let mut g = self.inner.lock();
        let Some(doc) = g.doc.clone() else { return };
        let Some(ord) = self.source.section_at_location(&doc, location) else { return };
        let target = if self.is_readable(&mut g, ord) {
            Some(ord)
        } else {
            self.source.next_readable(&doc, ord, true, &g.options)
        };
        if let Some(t) = target {
            let keep = self.active(&g);
            self.restart(&mut g, Position { section: t, segment: 0 }, 0, keep);
        }
    }

    pub fn current_location(&self) -> Option<u32> {
        let mut g = self.inner.lock();
        let c = g.current?;
        self.section_data(&mut g, c.section).map(|d| d.0.location)
    }

    pub fn next_location(&self) {
        if let Some(l) = self.current_location() {
            self.go_to_location(l + 1);
        }
    }

    pub fn previous_location(&self) {
        if let Some(l) = self.current_location() {
            if l > 1 {
                self.go_to_location(l - 1);
            }
        }
    }

    /// Skip by seconds, crossing sentence boundaries when needed.
    pub fn skip_seconds(&self, seconds: i32) {
        let mut g = self.inner.lock();
        let rate = g.sample_rate as i64;
        let delta = seconds as i64 * rate;
        let Some(cur) = g.current else { return };
        let keep = self.active(&g);
        if seconds > 0 {
            let target = g.played_in_segment as i64 + delta;
            if (target as usize) < g.segment_len {
                g.skip_request = Some(target as usize);
            } else if let Some(n) = self.next_position(&mut g, cur) {
                self.restart(&mut g, n, 0, keep);
            }
            return;
        }
        let mut remaining = -delta - g.played_in_segment as i64;
        if remaining <= 0 {
            g.skip_request = Some((g.played_in_segment as i64 + delta).max(0) as usize);
            return;
        }
        let past: Vec<(Position, i64)> =
            g.history.iter().rev().skip(1).map(|p| (p.pos, p.samples as i64 * rate / p.rate as i64)).collect();
        for (pos, n) in &past {
            if remaining <= *n {
                self.restart(&mut g, *pos, (*n - remaining) as usize, keep);
                return;
            }
            remaining -= n;
        }
        // Earlier than anything heard this session: estimate from the text.
        let mut pos = past.last().map(|p| p.0).unwrap_or(cur);
        while let Some(prev) = self.previous_position(&mut g, pos) {
            let chars = self
                .section_data(&mut g, prev.section)
                .and_then(|d| d.1.get(prev.segment as usize).map(|s| s.spoken_text.len()))
                .unwrap_or(40);
            let n = ((chars as f64 * 0.065 / g.speed as f64) * rate as f64) as i64;
            pos = prev;
            if remaining <= n.max(rate / 2) {
                break;
            }
            remaining -= n.max(rate / 2);
        }
        self.restart(&mut g, pos, 0, keep);
    }

    pub fn set_sleep_minutes(&self, minutes: Option<u32>) {
        let mut g = self.inner.lock();
        g.sleep_at_end_of = None;
        g.sleep_deadline = minutes.map(|m| Instant::now() + Duration::from_secs(m as u64 * 60));
    }

    #[cfg(test)]
    pub fn set_sleep_deadline(&self, d: Duration) {
        self.inner.lock().sleep_deadline = Some(Instant::now() + d);
    }

    pub fn sleep_at_end_of_location(&self) {
        let loc = self.current_location();
        let mut g = self.inner.lock();
        g.sleep_deadline = None;
        g.sleep_at_end_of = loc;
    }

    pub fn release(&self) {
        let mut g = self.inner.lock();
        if let Some(c) = g.current {
            self.save(&g, c);
        }
        self.flush_log(&mut g);
        self.released.store(true, Ordering::SeqCst);
        self.generation.fetch_add(1, Ordering::SeqCst);
        g.queue.clear();
        self.wake.notify_all();
    }

    // ------------------------------------------------------------ internals

    fn active(&self, g: &Inner) -> bool {
        matches!(g.state, PlayState::Playing | PlayState::Preparing)
    }

    fn set_state(&self, g: &mut Inner, s: PlayState) {
        if g.state != s {
            g.state = s;
            self.listener.state_changed(s);
        }
    }

    fn restart(&self, g: &mut Inner, pos: Position, offset: usize, keep_playing: bool) {
        self.generation.fetch_add(1, Ordering::SeqCst);
        g.queue.clear();
        self.sink.flush();
        g.synth_cursor = Some(pos);
        g.pending_offset = offset;
        g.current = Some(pos);
        g.played_in_segment = 0;
        g.skip_request = None;
        if keep_playing {
            self.set_state(g, PlayState::Preparing);
        } else if g.state == PlayState::Ended {
            self.set_state(g, PlayState::Idle);
        }
        self.save(g, pos);
        self.wake.notify_all();
    }

    fn save(&self, g: &Inner, pos: Position) {
        if let Some(d) = &g.doc {
            self.source.save_position(d, pos.section, pos.segment);
        }
    }

    fn flush_log(&self, g: &mut Inner) {
        let (Some(doc), rate) = (g.doc.clone(), g.opened_rate as u64) else { return };
        if rate == 0 {
            return;
        }
        let secs = g.unlogged_samples / rate;
        if secs > 0 {
            self.source.log_listening(&doc, secs as u32);
            g.unlogged_samples -= secs * rate;
        }
    }

    fn section_data(&self, g: &mut Inner, ordinal: u32) -> Option<(Section, Vec<Segment>)> {
        if let Some(v) = g.cache.get(&ordinal) {
            return Some(v.clone());
        }
        let doc = g.doc.clone()?;
        let s = self.source.section(&doc, ordinal)?;
        let segs = self.source.segments(&doc, ordinal, &g.options);
        if g.cache.len() > 64 {
            g.cache.clear();
        }
        g.cache.insert(ordinal, (s.clone(), segs.clone()));
        Some((s, segs))
    }

    fn is_readable(&self, g: &mut Inner, ordinal: u32) -> bool {
        self.section_data(g, ordinal).map(|d| !d.1.is_empty()).unwrap_or(false)
    }

    fn first_readable(&self, g: &mut Inner) -> Option<Position> {
        if self.is_readable(g, 0) {
            return Some(Position { section: 0, segment: 0 });
        }
        let doc = g.doc.clone()?;
        self.source.next_readable(&doc, 0, true, &g.options).map(|s| Position { section: s, segment: 0 })
    }

    fn next_position(&self, g: &mut Inner, p: Position) -> Option<Position> {
        let n = self.section_data(g, p.section).map(|d| d.1.len()).unwrap_or(0) as u32;
        if p.segment + 1 < n {
            return Some(Position { section: p.section, segment: p.segment + 1 });
        }
        let doc = g.doc.clone()?;
        self.source.next_readable(&doc, p.section, true, &g.options).map(|s| Position { section: s, segment: 0 })
    }

    fn previous_position(&self, g: &mut Inner, p: Position) -> Option<Position> {
        if p.segment > 0 {
            return Some(Position { section: p.section, segment: p.segment - 1 });
        }
        let doc = g.doc.clone()?;
        let prev = self.source.next_readable(&doc, p.section, false, &g.options)?;
        let n = self.section_data(g, prev)?.1.len() as u32;
        Some(Position { section: prev, segment: n.saturating_sub(1) })
    }

    fn released(&self) -> bool {
        self.released.load(Ordering::SeqCst)
    }

    fn synth_loop(&self) {
        while !self.released() {
            let mut g = self.inner.lock();
            while !self.released()
                && (!self.active(&g) || g.synth_cursor.is_none() || g.engine.is_none() || g.queue.len() >= LOOK_AHEAD)
            {
                self.wake.wait_for(&mut g, Duration::from_millis(200));
            }
            if self.released() {
                return;
            }
            let genr = self.generation.load(Ordering::SeqCst);
            let speed = g.speed;
            let offset = std::mem::take(&mut g.pending_offset);

            // Resolve to a real sentence, passing over sections the options skip.
            let mut p = g.synth_cursor;
            let mut found = None;
            while let Some(pos) = p {
                match self.section_data(&mut g, pos.section) {
                    Some((sec, segs)) => match segs.get(pos.segment as usize) {
                        Some(seg) => {
                            found = Some((pos, sec, seg.clone()));
                            break;
                        }
                        None => p = self.next_position(&mut g, Position { section: pos.section, segment: segs.len() as u32 }),
                    },
                    None => p = None,
                }
            }
            let Some((pos, section, seg)) = found else {
                g.queue.push_back(Item { generation: genr, section: None, segment: None, samples: vec![], pause_samples: 0, start_offset: 0, end: true });
                g.synth_cursor = None;
                self.wake.notify_all();
                continue;
            };

            // Synthesise without holding the lock.
            let mut engine = g.engine.take().unwrap();
            g.engine_busy = true;
            drop(g);
            let result = engine.synthesize(&seg.spoken_text, speed);
            let rate = engine.sample_rate();
            let mut g = self.inner.lock();
            g.engine_busy = false;
            if g.engine.is_none() {
                g.engine = Some(engine);
            }
            self.wake.notify_all();

            if genr != self.generation.load(Ordering::SeqCst) {
                continue;
            }
            match result {
                Err(e) => {
                    self.listener.error(&format!("The voice could not read this passage: {e}"));
                    self.sink.pause();
                    self.set_state(&mut g, PlayState::Paused);
                }
                Ok(samples) => {
                    let pause = (seg.pause_after_ms as u64 * rate as u64 / 1000) as usize;
                    g.synth_cursor = self.next_position(&mut g, pos);
                    g.queue.push_back(Item {
                        generation: genr,
                        section: Some(section),
                        segment: Some(seg),
                        samples,
                        pause_samples: pause,
                        start_offset: offset,
                        end: false,
                    });
                    if g.synth_cursor.is_none() {
                        g.queue.push_back(Item { generation: genr, section: None, segment: None, samples: vec![], pause_samples: 0, start_offset: 0, end: true });
                    }
                    self.wake.notify_all();
                }
            }
        }
    }

    fn play_loop(&self) {
        const CHUNK: usize = 2048;
        let silence = vec![0f32; CHUNK];
        while !self.released() {
            // Take the next item only when playing (a paused player never starts a new sentence).
            let mut g = self.inner.lock();
            while !self.released() && (!self.active(&g) || g.queue.is_empty()) {
                self.wake.wait_for(&mut g, Duration::from_millis(200));
            }
            if self.released() {
                return;
            }
            let item = g.queue.pop_front().unwrap();
            self.wake.notify_all();
            let genr = self.generation.load(Ordering::SeqCst);
            if item.generation != genr {
                continue;
            }
            if item.end {
                self.flush_log(&mut g);
                self.set_state(&mut g, PlayState::Ended);
                continue;
            }
            let section = item.section.unwrap();
            let seg = item.segment.unwrap();
            if let Some(end_loc) = g.sleep_at_end_of {
                if section.location != end_loc {
                    self.finish_sleep(&mut g, &seg);
                    continue;
                }
            }
            let rate = g.sample_rate;
            if g.opened_rate != rate {
                if let Err(e) = self.sink.open(rate) {
                    self.listener.error(&e);
                    self.set_state(&mut g, PlayState::Paused);
                    continue;
                }
                g.opened_rate = rate;
            }
            let pos = Position { section: seg.section_ordinal, segment: seg.index };
            g.current = Some(pos);
            g.segment_len = item.samples.len();
            g.played_in_segment = item.start_offset.min(item.samples.len());
            g.history.push_back(Played { pos, samples: item.samples.len(), rate });
            while g.history.len() > 50 {
                g.history.pop_front();
            }
            if g.state == PlayState::Preparing {
                self.set_state(&mut g, PlayState::Playing);
            }
            self.save(&g, pos);
            self.listener.segment_started(&section, &seg);
            let mut i = g.played_in_segment;
            drop(g);

            let total = item.samples.len() + item.pause_samples;
            while i < total && !self.released() && self.generation.load(Ordering::SeqCst) == genr {
                {
                    let mut g = self.inner.lock();
                    if matches!(g.state, PlayState::Paused | PlayState::Idle) {
                        self.wake.wait_for(&mut g, Duration::from_millis(200));
                        continue;
                    }
                    if let Some(req) = g.skip_request.take() {
                        self.sink.flush();
                        i = req.min(item.samples.len());
                    }
                    if g.sleep_deadline.map(|d| Instant::now() >= d).unwrap_or(false) {
                        self.finish_sleep(&mut g, &seg);
                        break;
                    }
                }
                let n = CHUNK.min(total - i);
                if i < item.samples.len() {
                    let m = n.min(item.samples.len() - i);
                    self.sink.write(&item.samples[i..i + m]);
                    i += m;
                } else {
                    self.sink.write(&silence[..n]);
                    i += n;
                }
                let mut g = self.inner.lock();
                g.played_in_segment = i.min(item.samples.len());
                g.unlogged_samples += n as u64;
                if g.unlogged_samples > rate as u64 * 60 {
                    self.flush_log(&mut g);
                }
            }
        }
    }

    /// Pause, keeping the interrupted sentence so play resumes exactly there.
    fn finish_sleep(&self, g: &mut Inner, seg: &Segment) {
        g.sleep_deadline = None;
        g.sleep_at_end_of = None;
        self.flush_log(g);
        self.restart(g, Position { section: seg.section_ordinal, segment: seg.index }, 0, false);
        self.sink.pause();
        self.set_state(g, PlayState::Paused);
        self.listener.sleep_finished();
    }
}
