//! Plain text: encoding detection and paragraph reconstruction.

use super::clean;
use crate::error::{ReaditError, Result};
use crate::model::{Extracted, LocationKind, RawSection, SectionKind};

/// Decode bytes as UTF-8 (with or without BOM), UTF-16 (with BOM), or
/// Windows-1252 as the last resort for older files.
pub fn decode(bytes: &[u8]) -> String {
    if let Some(rest) = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        return String::from_utf8_lossy(rest).into_owned();
    }
    if bytes.starts_with(&[0xFF, 0xFE]) || bytes.starts_with(&[0xFE, 0xFF]) {
        let le = bytes[0] == 0xFF;
        let units: Vec<u16> = bytes[2..]
            .chunks_exact(2)
            .map(|c| if le { u16::from_le_bytes([c[0], c[1]]) } else { u16::from_be_bytes([c[0], c[1]]) })
            .collect();
        return String::from_utf16_lossy(&units);
    }
    match std::str::from_utf8(bytes) {
        Ok(s) => s.to_string(),
        Err(_) => bytes.iter().map(|&b| cp1252(b)).collect(),
    }
}

fn cp1252(b: u8) -> char {
    const HIGH: [char; 32] = [
        '€', '\u{81}', '‚', 'ƒ', '„', '…', '†', '‡', 'ˆ', '‰', 'Š', '‹', 'Œ', '\u{8D}', 'Ž', '\u{8F}',
        '\u{90}', '‘', '’', '“', '”', '•', '–', '—', '˜', '™', 'š', '›', 'œ', '\u{9D}', 'ž', 'Ÿ',
    ];
    match b {
        0x80..=0x9F => HIGH[(b - 0x80) as usize],
        _ => b as char,
    }
}

pub fn extract(bytes: &[u8]) -> Result<Extracted> {
    let text = decode(bytes);
    let sections = paragraphs(&text);
    if sections.is_empty() {
        return Err(ReaditError::NoText { detail: "The text file contains only whitespace.".into() });
    }
    Ok(Extracted { location_kind: Some(LocationKind::None), sections, ..Default::default() })
}

/// Split on blank lines; re-join hard-wrapped lines; recognise simple headings.
pub fn paragraphs(text: &str) -> Vec<RawSection> {
    let mut out = Vec::new();
    let lines: Vec<&str> = text.lines().collect();
    let mut buf: Vec<&str> = Vec::new();
    let flush = |buf: &mut Vec<&str>, out: &mut Vec<RawSection>| {
        if buf.is_empty() {
            return;
        }
        let joined = join_wrapped(buf);
        buf.clear();
        let t = clean(&joined);
        if t.is_empty() {
            return;
        }
        if is_heading_line(&t) {
            out.push(RawSection::heading(2, t, 0));
        } else if is_bullet(&t) {
            out.push(RawSection::new(SectionKind::ListItem, strip_bullet(&t), 0));
        } else {
            out.push(RawSection::new(SectionKind::Paragraph, t, 0));
        }
    };
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        let trimmed = line.trim();
        // Setext underline: the previous single line is a heading.
        if !buf.is_empty() && buf.len() == 1 && is_rule(trimmed) {
            let t = clean(buf[0]);
            buf.clear();
            out.push(RawSection::heading(if trimmed.starts_with('=') { 1 } else { 2 }, t, 0));
        } else if trimmed.is_empty() {
            flush(&mut buf, &mut out);
        } else if is_bullet(trimmed) && !buf.is_empty() {
            // Each bullet is its own item even without blank lines between them.
            flush(&mut buf, &mut out);
            buf.push(line);
        } else {
            buf.push(line);
        }
        i += 1;
    }
    flush(&mut buf, &mut out);
    out
}

fn join_wrapped(lines: &[&str]) -> String {
    let mut s = String::new();
    for l in lines {
        let l = l.trim();
        if s.ends_with('-') && l.chars().next().map(|c| c.is_lowercase()).unwrap_or(false) {
            // "infor-\nmation" -> "information"
            s.pop();
            s.push_str(l);
        } else {
            if !s.is_empty() {
                s.push(' ');
            }
            s.push_str(l);
        }
    }
    s
}

fn is_rule(s: &str) -> bool {
    s.len() >= 3 && (s.chars().all(|c| c == '=') || s.chars().all(|c| c == '-'))
}

fn is_heading_line(s: &str) -> bool {
    let letters: Vec<char> = s.chars().filter(|c| c.is_alphabetic()).collect();
    s.chars().count() <= 80
        && letters.len() >= 3
        && letters.iter().all(|c| c.is_uppercase())
        && !s.ends_with('.')
}

fn is_bullet(s: &str) -> bool {
    let mut it = s.chars();
    match (it.next(), it.next()) {
        (Some('-' | '*' | '•' | '–'), Some(' ')) => true,
        _ => {
            let digits: String = s.chars().take_while(|c| c.is_ascii_digit()).collect();
            !digits.is_empty()
                && digits.len() <= 3
                && matches!(s[digits.len()..].chars().next(), Some('.' | ')'))
                && s[digits.len() + 1..].starts_with(' ')
        }
    }
}

fn strip_bullet(s: &str) -> String {
    let s = s.trim_start_matches(['-', '*', '•', '–']).trim_start();
    s.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rebuilds_paragraphs_and_headings() {
        let t = "REPORT SUMMARY\n\nThe analysis of trans-\nactions shows a pattern\nof structuring.\n\n- First item\n- Second item\n\nFindings\n--------\nDone.";
        let s = paragraphs(t);
        assert_eq!(s[0], RawSection::heading(2, "REPORT SUMMARY", 0));
        assert_eq!(s[1].text, "The analysis of transactions shows a pattern of structuring.");
        assert_eq!(s[2], RawSection::new(SectionKind::ListItem, "First item", 0));
        assert_eq!(s[3], RawSection::new(SectionKind::ListItem, "Second item", 0));
        assert_eq!(s[4], RawSection::heading(2, "Findings", 0));
        assert_eq!(s[5].text, "Done.");
    }

    #[test]
    fn decodes_legacy_and_utf16() {
        assert_eq!(decode(&[0x93, b'H', b'i', 0x94]), "“Hi”");
        assert_eq!(decode(&[0xFF, 0xFE, b'O', 0, b'K', 0]), "OK");
    }
}
