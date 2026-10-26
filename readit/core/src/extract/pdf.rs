//! PDF extraction through PDFium (the engine used by Chrome).
//!
//! Lines are rebuilt from characters in content order, which follows the
//! author's reading order in most generated PDFs, including multi-column
//! layouts. Lines are then grouped into paragraphs using spacing, font size
//! and indentation; headings are recognised by size relative to body text.
//! Running headers, footers and page numbers are tagged as page furniture so
//! the reader can skip them without losing them.

use super::ExtractOptions;
use crate::error::{ReaditError, Result};
use crate::model::{Extracted, LocationKind, RawSection, SectionKind};
use once_cell::sync::OnceCell;
use pdfium_render::prelude::*;
use regex::Regex;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

static PDFIUM: OnceCell<Pdfium> = OnceCell::new();

fn pdfium(opts: &ExtractOptions) -> Result<&'static Pdfium> {
    PDFIUM.get_or_try_init(|| {
        let mut candidates: Vec<PathBuf> = Vec::new();
        if let Some(dir) = &opts.pdfium_dir {
            candidates.push(Pdfium::pdfium_platform_library_name_at_path(dir));
        }
        if let Ok(dir) = std::env::var("READIT_PDFIUM_DIR") {
            candidates.push(Pdfium::pdfium_platform_library_name_at_path(&dir));
        }
        if let Some(dir) = std::env::current_exe().ok().and_then(|p| p.parent().map(|p| p.to_path_buf())) {
            candidates.push(Pdfium::pdfium_platform_library_name_at_path(&dir));
        }
        for c in &candidates {
            if let Ok(b) = Pdfium::bind_to_library(c) {
                return Ok(Pdfium::new(b));
            }
        }
        Pdfium::bind_to_system_library().map(Pdfium::new).map_err(|e| ReaditError::Io {
            detail: format!("the PDF engine (PDFium) could not be loaded: {e}"),
        })
    })
}

fn map_err(e: PdfiumError) -> ReaditError {
    match e {
        PdfiumError::PdfiumLibraryInternalError(PdfiumInternalError::PasswordError) => ReaditError::Encrypted,
        PdfiumError::PdfiumLibraryInternalError(PdfiumInternalError::FormatError) => {
            ReaditError::Corrupt { detail: "the PDF structure is invalid".into() }
        }
        other => ReaditError::Corrupt { detail: format!("the PDF could not be read ({other})") },
    }
}

#[derive(Debug, Clone)]
struct Line {
    text: String,
    size: f32,
    x: f32,
    y: f32,
    page: u32,
}

pub fn extract(path: &Path, opts: &ExtractOptions) -> Result<Extracted> {
    let pdfium = pdfium(opts)?;
    let doc = pdfium.load_pdf_from_file(path, None).map_err(map_err)?;
    let mut out = Extracted { location_kind: Some(LocationKind::Page), ..Default::default() };
    out.title = doc
        .metadata()
        .get(PdfDocumentMetadataTagType::Title)
        .map(|t| t.value().trim().to_string())
        .filter(|t| !t.is_empty() && !t.eq_ignore_ascii_case("untitled") && !t.ends_with(".doc") && !t.ends_with(".docx"));

    let mut pages: Vec<Vec<Line>> = Vec::new();
    for (i, page) in doc.pages().iter().enumerate() {
        let n = (i + 1) as u32;
        let lines = page_lines(&page, n)?;
        if lines.iter().all(|l| l.text.trim().is_empty()) {
            let has_images = page
                .objects()
                .iter()
                .any(|o| o.object_type() == PdfPageObjectType::Image);
            if has_images {
                out.pages_needing_ocr.push(n);
            }
        }
        pages.push(lines);
    }
    out.location_count = pages.len() as u32;
    if !out.pages_needing_ocr.is_empty() {
        let n = out.pages_needing_ocr.len();
        out.warnings.push(if n as u32 == out.location_count {
            "This PDF is scanned: its pages are images with no text layer. Run OCR to read it.".to_string()
        } else {
            format!("{n} page(s) are scanned images without text: {}. Run OCR to include them.", list(&out.pages_needing_ocr))
        });
    }

    let furniture = furniture_keys(&pages);
    let body_size = body_font_size(&pages);
    let heading_levels = heading_levels(&pages, body_size);
    for lines in &pages {
        build_paragraphs(lines, body_size, &heading_levels, &furniture, &mut out.sections);
    }
    if out.sections.iter().all(|s| s.kind == SectionKind::PageFurniture) {
        if out.pages_needing_ocr.is_empty() {
            return Err(ReaditError::NoText { detail: "This PDF contains no text layer or images to read.".into() });
        }
        out.sections.clear(); // OCR will provide the content
    }
    Ok(out)
}

fn list(v: &[u32]) -> String {
    let s: Vec<String> = v.iter().take(12).map(|n| n.to_string()).collect();
    if v.len() > 12 { format!("{} and {} more", s.join(", "), v.len() - 12) } else { s.join(", ") }
}

fn page_lines(page: &PdfPage, n: u32) -> Result<Vec<Line>> {
    let text = page.text().map_err(map_err)?;
    let mut lines = Vec::new();
    let mut cur = String::new();
    let mut sizes: Vec<f32> = Vec::new();
    let mut origin: Option<(f32, f32)> = None;
    let push_line = |cur: &mut String, sizes: &mut Vec<f32>, origin: &mut Option<(f32, f32)>, lines: &mut Vec<Line>| {
        let t = cur.trim().to_string();
        if !t.is_empty() {
            sizes.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
            let size = if sizes.is_empty() { 0.0 } else { sizes[sizes.len() / 2] };
            let (x, y) = origin.unwrap_or((0.0, 0.0));
            lines.push(Line { text: t, size, x, y, page: n });
        }
        cur.clear();
        sizes.clear();
        *origin = None;
    };
    for ch in text.chars().iter() {
        let Some(c) = ch.unicode_char() else { continue };
        match c {
            '\r' => continue,
            '\n' => push_line(&mut cur, &mut sizes, &mut origin, &mut lines),
            c => {
                if !c.is_whitespace() {
                    let size = ch.scaled_font_size().value;
                    if size > 0.0 {
                        sizes.push(size);
                    }
                    if origin.is_none() {
                        if let Ok((x, y)) = ch.origin() {
                            origin = Some((x.value, y.value));
                        }
                    }
                }
                // pdfium reports U+0002/U+FFFE for soft hyphens at line ends.
                if c == '\u{2}' || c == '\u{FFFE}' {
                    cur.push('-');
                } else {
                    cur.push(c);
                }
            }
        }
    }
    push_line(&mut cur, &mut sizes, &mut origin, &mut lines);
    Ok(lines)
}

/// Normalised form used to spot running headers and footers: digits collapsed.
fn furniture_key(s: &str) -> String {
    let re = page_number_re();
    if re.is_match(s.trim()) {
        return "#pagenum".into();
    }
    s.to_lowercase().chars().map(|c| if c.is_ascii_digit() { '#' } else { c }).collect::<String>()
}

fn page_number_re() -> &'static Regex {
    static RE: OnceCell<Regex> = OnceCell::new();
    RE.get_or_init(|| {
        Regex::new(r"(?i)^(page\s*)?[-–(\[]?\s*\d{1,4}\s*[-–)\]]?(\s*(of|/)\s*\d{1,4})?$|^[ivxlc]{1,7}$").unwrap()
    })
}

/// Lines in the top or bottom two positions of a page that repeat on at least
/// 40% of pages (minimum three), plus bare page numbers.
fn furniture_keys(pages: &[Vec<Line>]) -> std::collections::HashSet<String> {
    let mut counts: HashMap<String, usize> = HashMap::new();
    for lines in pages {
        let mut seen = std::collections::HashSet::new();
        let n = lines.len();
        for (i, l) in lines.iter().enumerate() {
            if i < 2 || i + 2 >= n {
                let k = furniture_key(&l.text);
                if seen.insert(k.clone()) {
                    *counts.entry(k).or_default() += 1;
                }
            }
        }
    }
    let threshold = ((pages.len() as f32) * 0.4).ceil().max(3.0) as usize;
    let mut keys: std::collections::HashSet<String> =
        counts.into_iter().filter(|(_, c)| *c >= threshold).map(|(k, _)| k).collect();
    keys.insert("#pagenum".into());
    keys
}

fn numbered_re() -> &'static Regex {
    static RE: OnceCell<Regex> = OnceCell::new();
    RE.get_or_init(|| Regex::new(r"^(\d{1,3}|[a-z]|[ivx]{1,4})[.)]\s+\S").unwrap())
}

fn body_font_size(pages: &[Vec<Line>]) -> f32 {
    // Weighted by characters: the size carrying the most text is the body size.
    let mut hist: HashMap<i32, usize> = HashMap::new();
    for l in pages.iter().flatten() {
        *hist.entry((l.size * 2.0).round() as i32).or_default() += l.text.chars().count();
    }
    hist.into_iter().max_by_key(|(_, c)| *c).map(|(k, _)| k as f32 / 2.0).unwrap_or(11.0)
}

fn is_heading_candidate(l: &Line, body: f32) -> bool {
    let chars = l.text.chars().count();
    l.size >= body * 1.18 && chars <= 160 && l.text.chars().any(|c| c.is_alphabetic())
}

fn heading_levels(pages: &[Vec<Line>], body: f32) -> HashMap<i32, u8> {
    let mut sizes: Vec<i32> = pages
        .iter()
        .flatten()
        .filter(|l| is_heading_candidate(l, body))
        .map(|l| (l.size * 2.0).round() as i32)
        .collect();
    sizes.sort_unstable_by(|a, b| b.cmp(a));
    sizes.dedup();
    sizes.into_iter().enumerate().map(|(i, s)| (s, (i as u8 + 1).min(3))).collect()
}

fn ends_sentence(s: &str) -> bool {
    matches!(s.trim_end().chars().last(), Some('.' | '!' | '?' | ':' | ';' | '"' | '”' | ')'))
}

fn build_paragraphs(
    lines: &[Line],
    body: f32,
    levels: &HashMap<i32, u8>,
    furniture: &std::collections::HashSet<String>,
    out: &mut Vec<RawSection>,
) {
    let n = lines.len();
    let mut cur: Option<(SectionKind, u8, String, f32, f32, f32, u32)> = None; // kind, level, text, size, start_x, last_y, page
    let flush = |cur: &mut Option<(SectionKind, u8, String, f32, f32, f32, u32)>, out: &mut Vec<RawSection>| {
        if let Some((kind, level, text, _, _, _, page)) = cur.take() {
            let t = super::clean(&text);
            if !t.is_empty() {
                out.push(RawSection { kind, level, text: t, location: page });
            }
        }
    };
    for (i, l) in lines.iter().enumerate() {
        let edge = i < 2 || i + 2 >= n;
        if edge && furniture.contains(&furniture_key(&l.text)) {
            flush(&mut cur, out);
            out.push(RawSection::new(SectionKind::PageFurniture, l.text.trim(), l.page));
            continue;
        }
        let heading = is_heading_candidate(l, body);
        let is_numbered = numbered_re().is_match(&l.text);
        let is_bullet = l.text.starts_with(['•', '▪', '◦', '●', '‣']) || l.text.starts_with("- ") || is_numbered;
        let start_new = match &cur {
            None => true,
            Some((kind, _, text, size, start_x, last_y, _)) => {
                let gap = last_y - l.y;
                let spacing = size.max(l.size) * 1.75;
                *kind == SectionKind::Heading && !heading
                    || heading && *kind != SectionKind::Heading
                    || (size - l.size).abs() > 1.0
                    || gap < -2.0
                    || gap > spacing
                    || is_bullet
                    || (l.x > start_x + l.size * 1.2 && ends_sentence(text) && *kind != SectionKind::Heading)
            }
        };
        if start_new {
            flush(&mut cur, out);
            let (kind, level, text) = if heading {
                (SectionKind::Heading, *levels.get(&((l.size * 2.0).round() as i32)).unwrap_or(&2), l.text.clone())
            } else if is_numbered {
                (SectionKind::ListItem, 0, l.text.clone())
            } else if is_bullet {
                (SectionKind::ListItem, 0, l.text.trim_start_matches(['•', '▪', '◦', '●', '‣', '-']).trim().to_string())
            } else {
                (SectionKind::Paragraph, 0, l.text.clone())
            };
            cur = Some((kind, level, text, l.size, l.x, l.y, l.page));
        } else if let Some((_, _, text, _, _, last_y, _)) = cur.as_mut() {
            let next = l.text.trim();
            if text.ends_with('-') && next.chars().next().map(|c| c.is_lowercase()).unwrap_or(false) {
                text.pop();
            } else {
                text.push(' ');
            }
            text.push_str(next);
            *last_y = l.y;
        }
    }
    flush(&mut cur, out);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_number_patterns() {
        for s in ["12", "Page 3", "page 3 of 10", "- 4 -", "(7)", "xiv", "3 / 20"] {
            assert_eq!(furniture_key(s), "#pagenum", "{s}");
        }
        assert_ne!(furniture_key("2025 Annual Report"), "#pagenum");
    }
}
