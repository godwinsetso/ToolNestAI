use crate::player::DocumentSource;
use readit_core::model::{Section, Segment};
use readit_core::segment::ReadingOptions;
use readit_core::ReaditLibrary;
use std::sync::Arc;

/// The core library as the player's document source.
pub struct CoreSource(pub Arc<ReaditLibrary>);

impl DocumentSource for CoreSource {
    fn segments(&self, doc: &str, ordinal: u32, o: &ReadingOptions) -> Vec<Segment> {
        self.0.segments(doc.into(), ordinal, o.clone()).unwrap_or_default()
    }
    fn next_readable(&self, doc: &str, from: u32, forward: bool, o: &ReadingOptions) -> Option<u32> {
        self.0.next_readable(doc.into(), from, forward, o.clone()).ok().flatten()
    }
    fn section_at_location(&self, doc: &str, location: u32) -> Option<u32> {
        self.0.section_at_location(doc.into(), location).ok().flatten()
    }
    fn section(&self, doc: &str, ordinal: u32) -> Option<Section> {
        self.0.sections(doc.into(), ordinal, 1).ok()?.into_iter().next().filter(|s| s.ordinal == ordinal)
    }
    fn save_position(&self, doc: &str, section: u32, segment: u32) {
        let _ = self.0.save_position(doc.into(), section, segment);
    }
    fn log_listening(&self, doc: &str, seconds: u32) {
        let _ = self.0.log_listening(doc.into(), seconds);
    }
}
