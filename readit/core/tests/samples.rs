//! End-to-end tests over the sample documents in /samples.
//! Requires PDFium: set READIT_PDFIUM_DIR (see README).

use readit_core::error::ReaditError;
use readit_core::model::{DocFormat, LocationKind, SectionKind};
use readit_core::segment::ReadingOptions;
use readit_core::ReaditLibrary as Library;
use std::path::PathBuf;
use std::sync::Arc;

fn sample(name: &str) -> String {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../samples").join(name).to_string_lossy().into_owned()
}

fn lib() -> (Arc<Library>, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let lib = Library::new(dir.path().to_string_lossy().into_owned(), std::env::var("READIT_PDFIUM_DIR").ok()).unwrap();
    (lib, dir)
}

fn kinds_and_text(lib: &Library, id: &str) -> Vec<(SectionKind, u32, String)> {
    lib.sections(id.into(), 0, 1000).unwrap().into_iter().map(|s| (s.kind, s.location, s.text)).collect()
}

#[test]
fn docx_structure() {
    let (lib, _d) = lib();
    let doc = lib.import_file(sample("kyc-basics.docx"), None).unwrap().document;
    assert_eq!(doc.format, DocFormat::Docx);
    assert_eq!(doc.title, "Know Your Customer Basics");
    let s = kinds_and_text(&lib, &doc.id);
    assert_eq!(s[0].0, SectionKind::Heading);
    assert!(s.iter().filter(|x| x.0 == SectionKind::ListItem).count() == 3, "{s:?}");
    assert!(s.iter().any(|x| x.0 == SectionKind::TableRow && x.2 == "High; Every 12 months"));
    // Header and footer parts must not leak into the body.
    assert!(!s.iter().any(|x| x.2.contains("CONFIDENTIAL HEADER") || x.2.contains("Footer text")));
}

#[test]
fn pptx_slides_and_notes() {
    let (lib, _d) = lib();
    let doc = lib.import_file(sample("kyc-training.pptx"), None).unwrap().document;
    assert_eq!(doc.location_kind, LocationKind::Slide);
    assert_eq!(doc.location_count, 3);
    let s = kinds_and_text(&lib, &doc.id);
    assert_eq!((s[0].0, s[0].1), (SectionKind::Heading, 1));
    assert!(s.iter().any(|x| x.0 == SectionKind::SpeakerNotes && x.1 == 2));

    // Notes are skipped by default, read when asked; slide content can be turned off.
    let notes_ord = lib.sections(doc.id.clone(), 0, 100).unwrap().into_iter().find(|x| x.kind == SectionKind::SpeakerNotes).unwrap().ordinal;
    assert!(lib.segments(doc.id.clone(), notes_ord, ReadingOptions::default()).unwrap().is_empty());
    let notes_only = ReadingOptions { read_speaker_notes: true, read_slide_content: false, ..Default::default() };
    assert!(!lib.segments(doc.id.clone(), notes_ord, notes_only.clone()).unwrap().is_empty());
    assert!(lib.segments(doc.id.clone(), 0, notes_only.clone()).unwrap().is_empty());
    assert_eq!(lib.next_readable(doc.id.clone(), 0, true, notes_only).unwrap(), Some(notes_ord));
}

#[test]
fn pdf_pages_headings_and_furniture() {
    let (lib, _d) = lib();
    let doc = lib.import_file(sample("kyc-guide.pdf"), None).unwrap().document;
    assert_eq!(doc.location_kind, LocationKind::Page);
    assert_eq!(doc.location_count, 3);
    assert_eq!(doc.title, "Know Your Customer Basics");
    let s = kinds_and_text(&lib, &doc.id);
    let heads: Vec<&str> = s.iter().filter(|x| x.0 == SectionKind::Heading).map(|x| x.2.as_str()).collect();
    assert_eq!(heads, vec!["Know Your Customer Basics", "Risk rating", "Onboarding steps", "Record keeping"]);
    // Running header and "Page n of 3" are kept but tagged as furniture.
    assert_eq!(s.iter().filter(|x| x.0 == SectionKind::PageFurniture).count(), 6);
    assert!(s.iter().any(|x| x.0 == SectionKind::Paragraph && x.2.contains("GH₵ 1,250.50")), "cedi sign survives extraction");
    // Furniture is skipped by default and read when asked.
    let furn = lib.sections(doc.id.clone(), 0, 100).unwrap().into_iter().find(|x| x.kind == SectionKind::PageFurniture).unwrap();
    assert!(lib.segments(doc.id.clone(), furn.ordinal, ReadingOptions::default()).unwrap().is_empty());
    let all = ReadingOptions { skip_page_furniture: false, ..Default::default() };
    assert!(!lib.segments(doc.id.clone(), furn.ordinal, all).unwrap().is_empty());
    // Page navigation.
    let p3 = lib.section_at_location(doc.id.clone(), 3).unwrap().unwrap();
    assert_eq!(lib.sections(doc.id.clone(), p3, 1).unwrap()[0].location, 3);
}

#[test]
fn epub_chapters() {
    let (lib, _d) = lib();
    let doc = lib.import_file(sample("kyc-book.epub"), None).unwrap().document;
    assert_eq!(doc.location_kind, LocationKind::Chapter);
    assert_eq!(doc.location_count, 2);
    let s = kinds_and_text(&lib, &doc.id);
    assert_eq!(s.iter().filter(|x| x.0 == SectionKind::ListItem).count(), 3);
}

#[test]
fn text_formats() {
    let (lib, _d) = lib();
    for (f, fmt) in [
        ("kyc-notes.md", DocFormat::Markdown),
        ("kyc-notes.txt", DocFormat::Txt),
        ("kyc-page.html", DocFormat::Html),
        ("kyc-notes.rtf", DocFormat::Rtf),
    ] {
        let doc = lib.import_file(sample(f), None).unwrap().document;
        assert_eq!(doc.format, fmt, "{f}");
        let s = kinds_and_text(&lib, &doc.id);
        assert!(!s.is_empty(), "{f}");
        assert!(!s.iter().any(|x| x.2.contains("alert(") || x.2.contains("Home | Contact")), "{f}: no scripts or nav");
    }
}

#[test]
fn scanned_documents_flag_ocr_instead_of_failing() {
    let (lib, _d) = lib();
    let doc = lib.import_file(sample("scanned-notice.pdf"), None).unwrap().document;
    assert_eq!(doc.section_count, 0);
    assert!(doc.warnings[0].contains("OCR"));
    assert_eq!(lib.pages_needing_ocr(doc.id).unwrap(), vec![1]);
    let img = lib.import_file(sample("scanned-notice.png"), None).unwrap().document;
    assert_eq!(img.format, DocFormat::Image);
}

#[test]
fn clear_errors_for_bad_files() {
    let (lib, _d) = lib();
    assert!(matches!(lib.import_file(sample("protected.pdf"), None), Err(ReaditError::Encrypted)));
    assert!(matches!(lib.import_file(sample("broken.docx"), None), Err(ReaditError::Corrupt { .. })));
    assert!(matches!(lib.import_file(sample("unknown.xyz"), None), Err(ReaditError::UnsupportedFormat { .. })));
    assert!(matches!(lib.import_file(sample("archive.zip"), None), Err(ReaditError::UnsupportedFormat { .. })));
    // Nothing half-imported is left behind.
    assert!(lib.documents("all".into(), None).unwrap().is_empty());
    assert_eq!(std::fs::read_dir(lib.documents_dir()).unwrap().count(), 0);
}

#[test]
fn reimport_keeps_ids_and_progress_survives_restart() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().to_string_lossy().into_owned();
    let pdfium = std::env::var("READIT_PDFIUM_DIR").ok();
    let id = {
        let lib = Library::new(path.clone(), pdfium.clone()).unwrap();
        let doc = lib.import_file(sample("kyc-basics.docx"), None).unwrap().document;
        lib.save_position(doc.id.clone(), 3, 1).unwrap();
        lib.add_bookmark(doc.id.clone(), 2, 0, "Risk".into(), Some("check this".into()), None).unwrap();
        doc.id
    };
    // "Restart": open the same data directory again.
    let lib = Library::new(path, pdfium).unwrap();
    let again = lib.import_file(sample("kyc-basics.docx"), None).unwrap();
    assert!(again.already_in_library);
    assert_eq!(again.document.id, id);
    let pos = lib.open_document(id.clone()).unwrap().unwrap();
    assert_eq!((pos.section_ordinal, pos.segment_index), (3, 1));
    assert_eq!(lib.bookmarks(id.clone()).unwrap()[0].note.as_deref(), Some("check this"));
    assert_eq!(lib.documents("recent".into(), None).unwrap().len(), 1);
}

#[test]
fn removing_keeps_user_original() {
    let (lib, _d) = lib();
    let original = sample("kyc-notes.md");
    let doc = lib.import_file(original.clone(), None).unwrap().document;
    assert_ne!(doc.source_path, original, "READIT works from its own copy");
    lib.remove_document(doc.id.clone()).unwrap();
    assert!(std::path::Path::new(&original).exists());
    assert!(!std::path::Path::new(&doc.source_path).exists());
    assert!(lib.document(doc.id).is_err());
}

#[test]
fn search_pronunciation_and_spoken_text() {
    let (lib, _d) = lib();
    let doc = lib.import_file(sample("kyc-basics.docx"), None).unwrap().document;
    let hits = lib.search(doc.id.clone(), "sanctions".into()).unwrap();
    assert_eq!(hits.len(), 1);
    assert!(lib.search(doc.id.clone(), "\"; DROP TABLE x; --".into()).is_ok(), "search input cannot inject FTS syntax");

    let segs = lib.segments(doc.id.clone(), 1, ReadingOptions::default()).unwrap();
    assert_eq!(segs.len(), 3);
    assert_eq!(segs[0].pause_after_ms, 0);
    assert!(segs[2].spoken_text.contains("one thousand, two hundred and fifty see-deez and fifty peh-SEH-wahz"), "{}", segs[2].spoken_text);

    lib.add_pronunciation("Mensah".into(), "MEN-sah".into(), false, Some(doc.id.clone())).unwrap();
    let segs = lib.segments(doc.id.clone(), 1, ReadingOptions::default()).unwrap();
    assert!(segs[1].spoken_text.contains("Doctor MEN-sah"), "{}", segs[1].spoken_text);
}

#[test]
fn personal_voice_requires_live_consent() {
    let (lib, _d) = lib();
    assert!(lib.create_voice_profile("Me".into(), "en".into(), 999).is_err());
    assert!(lib.record_consent("".into(), "self".into(), "I agree".into(), "on_device".into()).is_err());
    assert!(lib.record_consent("Ama".into(), "friend".into(), "I agree".into(), "on_device".into()).is_err());
    let c = lib.record_consent("Godwin".into(), "self".into(), "I confirm this is my voice.".into(), "on_device".into()).unwrap();
    lib.create_voice_profile("My Voice".into(), "en-GH".into(), c).unwrap();
    lib.revoke_consent(c).unwrap();
    assert!(lib.create_voice_profile("Again".into(), "en".into(), c).is_err(), "revoked consent cannot be reused");
}

#[test]
fn storage_and_erase() {
    let (lib, _d) = lib();
    lib.import_file(sample("kyc-guide.pdf"), None).unwrap();
    assert!(lib.storage_report().documents_bytes > 0);
    lib.erase_all_data().unwrap();
    assert!(lib.documents("all".into(), None).unwrap().is_empty());
    assert_eq!(lib.storage_report().documents_bytes, 0);
    assert_eq!(lib.reading_profiles().unwrap().len(), 5, "defaults are recreated");
}
