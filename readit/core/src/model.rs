//! Core data types shared by every platform.
//!
//! Identifiers are deterministic: a document id is derived from the file's
//! content hash, and a section id is `<document id>:<ordinal>`. Re-importing
//! the same file therefore yields the same ids, which keeps bookmarks and
//! reading positions meaningful across restarts and reinstalls.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, uniffi::Enum)]
pub enum DocFormat {
    Pdf,
    Docx,
    Doc,
    Pptx,
    Ppt,
    Epub,
    Txt,
    Rtf,
    Html,
    Markdown,
    Image,
}

impl DocFormat {
    pub fn as_str(&self) -> &'static str {
        match self {
            DocFormat::Pdf => "pdf",
            DocFormat::Docx => "docx",
            DocFormat::Doc => "doc",
            DocFormat::Pptx => "pptx",
            DocFormat::Ppt => "ppt",
            DocFormat::Epub => "epub",
            DocFormat::Txt => "txt",
            DocFormat::Rtf => "rtf",
            DocFormat::Html => "html",
            DocFormat::Markdown => "md",
            DocFormat::Image => "image",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "pdf" => DocFormat::Pdf,
            "docx" => DocFormat::Docx,
            "doc" => DocFormat::Doc,
            "pptx" => DocFormat::Pptx,
            "ppt" => DocFormat::Ppt,
            "epub" => DocFormat::Epub,
            "txt" => DocFormat::Txt,
            "rtf" => DocFormat::Rtf,
            "html" => DocFormat::Html,
            "md" => DocFormat::Markdown,
            "image" => DocFormat::Image,
            _ => return None,
        })
    }
}

/// The structural role of a section. Drives pauses and reading modes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, uniffi::Enum)]
pub enum SectionKind {
    Heading,
    Paragraph,
    ListItem,
    TableRow,
    SpeakerNotes,
    Code,
    Footnote,
    /// Running headers, footers and page numbers. Kept, never silently dropped;
    /// the reader skips them when the profile asks it to.
    PageFurniture,
}

impl SectionKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            SectionKind::Heading => "heading",
            SectionKind::Paragraph => "paragraph",
            SectionKind::ListItem => "list_item",
            SectionKind::TableRow => "table_row",
            SectionKind::SpeakerNotes => "speaker_notes",
            SectionKind::Code => "code",
            SectionKind::Footnote => "footnote",
            SectionKind::PageFurniture => "page_furniture",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s {
            "heading" => SectionKind::Heading,
            "list_item" => SectionKind::ListItem,
            "table_row" => SectionKind::TableRow,
            "speaker_notes" => SectionKind::SpeakerNotes,
            "code" => SectionKind::Code,
            "footnote" => SectionKind::Footnote,
            "page_furniture" => SectionKind::PageFurniture,
            _ => SectionKind::Paragraph,
        }
    }
}

/// Where a section sits in the original file: page (PDF), slide (PPTX),
/// chapter (EPUB) or none (flowing text such as DOCX or TXT).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, uniffi::Enum)]
pub enum LocationKind {
    Page,
    Slide,
    Chapter,
    None,
}

impl LocationKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            LocationKind::Page => "page",
            LocationKind::Slide => "slide",
            LocationKind::Chapter => "chapter",
            LocationKind::None => "none",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s {
            "page" => LocationKind::Page,
            "slide" => LocationKind::Slide,
            "chapter" => LocationKind::Chapter,
            _ => LocationKind::None,
        }
    }
}

/// A section produced by an extractor, before it is stored.
#[derive(Debug, Clone, PartialEq)]
pub struct RawSection {
    pub kind: SectionKind,
    /// Heading level (1 = top) for headings, 0 otherwise.
    pub level: u8,
    pub text: String,
    /// 1-based page, slide or chapter number; 0 when not applicable.
    pub location: u32,
}

impl RawSection {
    pub fn new(kind: SectionKind, text: impl Into<String>, location: u32) -> Self {
        Self { kind, level: 0, text: text.into(), location }
    }

    pub fn heading(level: u8, text: impl Into<String>, location: u32) -> Self {
        Self { kind: SectionKind::Heading, level, text: text.into(), location }
    }
}

/// Output of any extractor.
#[derive(Debug, Clone, Default)]
pub struct Extracted {
    pub title: Option<String>,
    pub location_kind: Option<LocationKind>,
    /// Number of pages, slides or chapters.
    pub location_count: u32,
    pub sections: Vec<RawSection>,
    /// Non-fatal notes for the user, e.g. "pages 4 and 7 contain no text; OCR available".
    pub warnings: Vec<String>,
    /// Pages (or images) with no extractable text, candidates for OCR.
    pub pages_needing_ocr: Vec<u32>,
}

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct Document {
    pub id: String,
    pub title: String,
    pub file_name: String,
    pub format: DocFormat,
    pub source_path: String,
    pub size_bytes: u64,
    pub location_kind: LocationKind,
    pub location_count: u32,
    pub section_count: u32,
    pub imported_at: i64,
    pub last_opened_at: Option<i64>,
    pub favourite: bool,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct Section {
    pub id: String,
    pub document_id: String,
    pub ordinal: u32,
    pub kind: SectionKind,
    pub level: u8,
    pub text: String,
    pub location: u32,
}

/// One unit handed to a speech engine: roughly a sentence.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct Segment {
    pub id: String,
    pub section_ordinal: u32,
    pub index: u32,
    /// The original text, used for on-screen highlighting.
    pub display_text: String,
    /// Byte offsets of `display_text` within the section text (UTF-8, for Rust).
    pub start: u32,
    pub end: u32,
    /// The same range in UTF-16 code units, for Kotlin and JavaScript strings.
    pub start_utf16: u32,
    pub end_utf16: u32,
    /// Normalised text sent to the speech engine. Never shown as document content.
    pub spoken_text: String,
    /// Pause to insert after this segment, in milliseconds.
    pub pause_after_ms: u32,
}

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct ReadingPosition {
    pub document_id: String,
    pub section_ordinal: u32,
    pub segment_index: u32,
    pub updated_at: i64,
}

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct Bookmark {
    pub id: i64,
    pub document_id: String,
    pub section_ordinal: u32,
    pub segment_index: u32,
    pub label: String,
    pub note: Option<String>,
    pub highlight_text: Option<String>,
    pub created_at: i64,
}

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct PronunciationRule {
    pub id: i64,
    pub term: String,
    pub replacement: String,
    pub case_sensitive: bool,
    /// None means the rule applies to every document.
    pub document_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct InstalledVoice {
    pub id: String,
    pub engine: String,
    pub name: String,
    pub language: String,
    pub accent: String,
    pub gender: String,
    pub quality: String,
    pub licence: String,
    pub size_bytes: u64,
    pub offline: bool,
    pub path: String,
    pub installed_at: i64,
}

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct ConsentRecord {
    pub id: i64,
    pub subject_name: String,
    /// "self" or "authorised_other".
    pub relation: String,
    pub statement: String,
    pub processing_location: String,
    pub accepted_at: i64,
    pub revoked_at: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct ReadingProfile {
    pub id: i64,
    pub name: String,
    pub voice_id: Option<String>,
    pub speed: f32,
    pub pitch: f32,
    pub paragraph_pause_ms: u32,
    pub heading_pause_ms: u32,
    pub read_speaker_notes: bool,
    pub read_slide_content: bool,
    pub skip_headers_footers: bool,
}

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct Collection {
    pub id: i64,
    pub name: String,
    pub document_count: u32,
}
