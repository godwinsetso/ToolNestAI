//! Splits sections into speakable segments (about one sentence each) that
//! keep byte offsets into the original text, so the reader can highlight
//! exactly what is being spoken.

use crate::model::{PronunciationRule, ReadingProfile, Section, SectionKind, Segment};
use crate::normalize::{normalize, NormalizeOptions};

/// Segments longer than this are split at clause boundaries so speech starts
/// quickly and highlighting stays precise.
const MAX_SEGMENT_CHARS: usize = 280;

const NO_BREAK_AFTER: &[&str] = &[
    "mr", "mrs", "ms", "dr", "prof", "rev", "hon", "st", "no", "nos", "fig", "figs", "vs", "etc",
    "e.g", "i.e", "approx", "dept", "ltd", "co", "inc", "jan", "feb", "mar", "apr", "jun", "jul",
    "aug", "sep", "sept", "oct", "nov", "dec", "vol", "pp", "p", "ch", "sec", "art", "para", "cf", "al",
];

/// How a reading profile treats special content.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum CodeMode {
    Read,
    Announce,
    Skip,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct ReadingOptions {
    pub skip_page_furniture: bool,
    pub read_slide_content: bool,
    pub read_speaker_notes: bool,
    pub read_tables: bool,
    pub read_footnotes: bool,
    pub code_mode: CodeMode,
    pub paragraph_pause_ms: u32,
    pub heading_pause_ms: u32,
    pub spell_acronyms: bool,
    pub spell_mode: bool,
    pub day_first_dates: bool,
}

impl Default for ReadingOptions {
    fn default() -> Self {
        Self {
            skip_page_furniture: true,
            read_slide_content: true,
            read_speaker_notes: false,
            read_tables: true,
            read_footnotes: false,
            code_mode: CodeMode::Announce,
            paragraph_pause_ms: 350,
            heading_pause_ms: 700,
            spell_acronyms: true,
            spell_mode: false,
            day_first_dates: true,
        }
    }
}

impl From<&ReadingProfile> for ReadingOptions {
    fn from(p: &ReadingProfile) -> Self {
        Self {
            skip_page_furniture: p.skip_headers_footers,
            read_slide_content: p.read_slide_content,
            read_speaker_notes: p.read_speaker_notes,
            paragraph_pause_ms: p.paragraph_pause_ms,
            heading_pause_ms: p.heading_pause_ms,
            ..Default::default()
        }
    }
}

/// Whether a section is read under the given options. Skipped sections stay
/// visible in the document view; they are only left out of the audio.
pub fn is_read(section: &Section, o: &ReadingOptions, is_slide_deck: bool) -> bool {
    match section.kind {
        SectionKind::PageFurniture => !o.skip_page_furniture,
        SectionKind::SpeakerNotes => o.read_speaker_notes,
        SectionKind::TableRow => o.read_tables && (!is_slide_deck || o.read_slide_content),
        SectionKind::Footnote => o.read_footnotes,
        SectionKind::Code => o.code_mode != CodeMode::Skip,
        _ => !is_slide_deck || o.read_slide_content,
    }
}

/// Byte ranges of sentences in `text`.
pub fn sentence_ranges(text: &str) -> Vec<(usize, usize)> {
    let bytes = text.as_bytes();
    let mut ranges = Vec::new();
    let mut start = 0;
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    let mut i = 0;
    while i < chars.len() {
        let (pos, c) = chars[i];
        if matches!(c, '.' | '!' | '?' | '…' | '。') {
            // absorb runs like "?!" and closing quotes/brackets
            let mut j = i + 1;
            while j < chars.len() && matches!(chars[j].1, '.' | '!' | '?' | '"' | '\'' | '”' | '’' | ')' | ']') {
                j += 1;
            }
            let end = if j < chars.len() { chars[j].0 } else { text.len() };
            let next_is_space = j < chars.len() && chars[j].1.is_whitespace();
            let after = chars[j..].iter().find(|(_, ch)| !ch.is_whitespace()).map(|(_, ch)| *ch);
            let starts_sentence = after.map(|ch| ch.is_uppercase() || ch.is_ascii_digit() || matches!(ch, '"' | '“' | '(' | '‘')).unwrap_or(true);
            let token_before: String = {
                let before = &text[start..pos];
                before.rsplit(|ch: char| ch.is_whitespace() || ch == '(').next().unwrap_or("").to_lowercase()
            };
            // A list marker at the very start ("1.", "a)", "iv.") is not a sentence.
            let leading_marker = text[..pos].trim().chars().count() <= 4
                && !text[..pos].trim().is_empty()
                && (text[..pos].trim().chars().all(|ch| ch.is_ascii_digit())
                    || text[..pos].trim().chars().all(|ch| "ivxlcIVXLC".contains(ch))
                    || (text[..pos].trim().chars().count() == 1 && text[..pos].trim().chars().all(|ch| ch.is_alphabetic())));
            let is_abbrev = c == '.'
                && (leading_marker
                    || NO_BREAK_AFTER.contains(&token_before.trim_end_matches('.'))
                    || (token_before.chars().count() == 1 && token_before.chars().all(|ch| ch.is_alphabetic())));
            let decimal = c == '.' && j < chars.len() && chars[j].1.is_ascii_digit();
            if (next_is_space || j >= chars.len()) && starts_sentence && !is_abbrev && !decimal {
                if text[start..end].trim().len() > 0 {
                    ranges.push((start, end));
                }
                start = end;
                while start < bytes.len() && (bytes[start] as char).is_whitespace() {
                    start += 1;
                }
            }
            i = j;
            continue;
        }
        i += 1;
    }
    if start < text.len() && !text[start..].trim().is_empty() {
        ranges.push((start, text.trim_end().len().max(start)));
    }
    // Split overly long sentences at ; : , boundaries.
    let mut out = Vec::new();
    for (s, e) in ranges {
        split_long(text, s, e, &mut out);
    }
    out
}

fn split_long(text: &str, s: usize, e: usize, out: &mut Vec<(usize, usize)>) {
    if text[s..e].chars().count() <= MAX_SEGMENT_CHARS {
        out.push((s, e));
        return;
    }
    let slice = &text[s..e];
    // Prefer the boundary closest to the middle, strongest punctuation first.
    let mid = slice.len() / 2;
    for set in [&[';', ':'][..], &[','][..], &[' '][..]] {
        let best = slice
            .char_indices()
            .filter(|(i, c)| set.contains(c) && *i > 20 && *i + 20 < slice.len())
            .min_by_key(|(i, _)| (*i as isize - mid as isize).abs());
        if let Some((i, c)) = best {
            let cut = s + i + c.len_utf8();
            let mut next = cut;
            while next < e && (text.as_bytes()[next] as char).is_whitespace() {
                next += 1;
            }
            split_long(text, s, cut, out);
            split_long(text, next, e, out);
            return;
        }
    }
    out.push((s, e));
}

fn utf16_len(s: &str) -> u32 {
    s.encode_utf16().count() as u32
}

/// Build the segments for one section.
pub fn segments_for(section: &Section, rules: &[PronunciationRule], o: &ReadingOptions) -> Vec<Segment> {
    let nopts = NormalizeOptions {
        spell_acronyms: o.spell_acronyms,
        day_first_dates: o.day_first_dates,
        spell_mode: o.spell_mode,
    };
    let pause_end = match section.kind {
        SectionKind::Heading => o.heading_pause_ms,
        _ => o.paragraph_pause_ms,
    };
    if section.kind == SectionKind::Code {
        let spoken = match o.code_mode {
            CodeMode::Announce => "Code block.".to_string(),
            _ => section.text.lines().map(|l| l.trim()).filter(|l| !l.is_empty()).collect::<Vec<_>>().join(". "),
        };
        return vec![Segment {
            id: format!("{}:0", section.id),
            section_ordinal: section.ordinal,
            index: 0,
            display_text: section.text.clone(),
            start: 0,
            end: section.text.len() as u32,
            start_utf16: 0,
            end_utf16: utf16_len(&section.text),
            spoken_text: spoken,
            pause_after_ms: pause_end,
        }];
    }
    let ranges = if section.kind == SectionKind::Heading || section.kind == SectionKind::TableRow {
        vec![(0, section.text.len())]
    } else {
        sentence_ranges(&section.text)
    };
    let n = ranges.len();
    ranges
        .into_iter()
        .enumerate()
        .map(|(i, (s, e))| {
            let display = section.text[s..e].to_string();
            let mut spoken = normalize(&display, rules, &nopts);
            if section.kind == SectionKind::TableRow {
                spoken = spoken.replace(';', ",");
            }
            Segment {
                id: format!("{}:{}", section.id, i),
                section_ordinal: section.ordinal,
                index: i as u32,
                display_text: display,
                start: s as u32,
                end: e as u32,
                start_utf16: utf16_len(&section.text[..s]),
                end_utf16: utf16_len(&section.text[..e]),
                spoken_text: spoken,
                pause_after_ms: if i + 1 == n { pause_end } else { 0 },
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sec(text: &str) -> Section {
        Section { id: "d:0".into(), document_id: "d".into(), ordinal: 0, kind: SectionKind::Paragraph, level: 0, text: text.into(), location: 1 }
    }

    #[test]
    fn splits_sentences_but_not_abbreviations_or_decimals() {
        let t = "Dr. Osei reviewed 2.5 million records. Was it enough? Yes! See Fig. 4 and J. Mensah's note.";
        let r: Vec<&str> = sentence_ranges(t).into_iter().map(|(s, e)| &t[s..e]).collect();
        assert_eq!(r, vec!["Dr. Osei reviewed 2.5 million records.", "Was it enough?", "Yes!", "See Fig. 4 and J. Mensah's note."]);
    }

    #[test]
    fn list_markers_stay_with_their_item() {
        for t in ["1. Collect a valid Ghana Card.", "iv. Review the file.", "b. Screen the name. Then log it."] {
            let r: Vec<&str> = sentence_ranges(t).into_iter().map(|(s, e)| &t[s..e]).collect();
            assert!(r[0].len() > 4, "{t}: {r:?}");
        }
        let t = "In 2024. Then 2025.";
        assert_eq!(sentence_ranges(t).len(), 2, "a year ending a sentence still splits");
    }

    #[test]
    fn offsets_map_back_to_original_text() {
        let s = sec("First sentence here. Second one follows with GH₵ 5.");
        let segs = segments_for(&s, &[], &ReadingOptions::default());
        assert_eq!(segs.len(), 2);
        for g in &segs {
            assert_eq!(&s.text[g.start as usize..g.end as usize], g.display_text);
        }
        assert_eq!(segs[1].spoken_text, "Second one follows with five cedis.");
        assert_eq!(segs[0].pause_after_ms, 0);
        assert_eq!(segs[1].pause_after_ms, 350);
        assert_eq!(segs[1].id, "d:0:1");
    }

    #[test]
    fn long_sentences_are_split_at_clauses() {
        let long = format!("{}; {}.", "word ".repeat(40).trim(), "more ".repeat(40).trim());
        let r = sentence_ranges(&long);
        assert!(r.len() >= 2);
        assert!(r.iter().all(|(s, e)| long[*s..*e].chars().count() <= MAX_SEGMENT_CHARS));
    }
}
