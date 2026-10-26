//! Rich Text Format: a tokenizer that keeps body text and drops font tables,
//! styles, pictures, embedded objects, headers and footers.

use super::clean;
use super::text::paragraphs;
use crate::error::{ReaditError, Result};
use crate::model::{Extracted, LocationKind, RawSection, SectionKind};

const SKIP_DESTINATIONS: &[&str] = &[
    "fonttbl", "colortbl", "stylesheet", "info", "pict", "object", "header", "headerl", "headerr",
    "headerf", "footer", "footerl", "footerr", "footerf", "fldinst", "themedata", "colorschememapping",
    "latentstyles", "datastore", "xmlnstbl", "listtable", "listoverridetable", "rsidtbl", "generator",
    "pgdsctbl", "mmathPr", "filetbl", "revtbl", "bkmkstart", "bkmkend", "nonshppict", "shpinst",
    "listtext", "pntext", "pntxta", "pntxtb", "xe", "tc",
];

#[derive(Clone, Copy)]
struct Group {
    skip: bool,
    uc: usize,
    outline: Option<u8>,
}

pub fn extract(bytes: &[u8]) -> Result<Extracted> {
    let paras = to_paragraphs(bytes)?;
    if paras.is_empty() {
        return Err(ReaditError::NoText { detail: "The RTF document contains no text.".into() });
    }
    Ok(Extracted { location_kind: Some(LocationKind::None), sections: paras, ..Default::default() })
}

fn to_paragraphs(bytes: &[u8]) -> Result<Vec<RawSection>> {
    if !bytes.starts_with(b"{\\rtf") {
        return Err(ReaditError::Corrupt { detail: "not an RTF file".into() });
    }
    let mut stack = vec![Group { skip: false, uc: 1, outline: None }];
    let mut out: Vec<RawSection> = Vec::new();
    let mut para = String::new();
    let mut para_outline: Option<u8> = None;
    let mut pending_skip_chars = 0usize;
    let mut star = false;
    let mut i = 0;

    let finish = |para: &mut String, outline: &mut Option<u8>, out: &mut Vec<RawSection>| {
        let t = clean(para);
        para.clear();
        if !t.is_empty() {
            match outline.take() {
                Some(l) => out.push(RawSection::heading(l, t, 0)),
                None => {
                    // Reuse plain-text heuristics for bullets and all-caps headings.
                    let mut parsed = paragraphs(&t);
                    if parsed.len() == 1 {
                        out.push(parsed.remove(0));
                    } else {
                        out.push(RawSection::new(SectionKind::Paragraph, t, 0));
                    }
                }
            }
        }
        *outline = None;
    };

    while i < bytes.len() {
        let b = bytes[i];
        let g = *stack.last().unwrap();
        match b {
            b'{' => {
                stack.push(g);
                i += 1;
            }
            b'}' => {
                if stack.len() > 1 {
                    stack.pop();
                }
                i += 1;
            }
            b'\\' => {
                i += 1;
                if i >= bytes.len() {
                    break;
                }
                let c = bytes[i];
                if c.is_ascii_alphabetic() {
                    let start = i;
                    while i < bytes.len() && bytes[i].is_ascii_alphabetic() {
                        i += 1;
                    }
                    let word = std::str::from_utf8(&bytes[start..i]).unwrap_or("");
                    let mut param: Option<i32> = None;
                    let pstart = i;
                    if i < bytes.len() && (bytes[i] == b'-' || bytes[i].is_ascii_digit()) {
                        i += 1;
                        while i < bytes.len() && bytes[i].is_ascii_digit() {
                            i += 1;
                        }
                        param = std::str::from_utf8(&bytes[pstart..i]).ok().and_then(|s| s.parse().ok());
                    }
                    if i < bytes.len() && bytes[i] == b' ' {
                        i += 1;
                    }
                    let top = stack.last_mut().unwrap();
                    if star || SKIP_DESTINATIONS.contains(&word) {
                        top.skip = true;
                        star = false;
                        continue;
                    }
                    if top.skip {
                        continue;
                    }
                    match word {
                        "par" | "sect" | "page" => finish(&mut para, &mut para_outline, &mut out),
                        "line" | "tab" | "cell" => para.push(' '),
                        "row" => finish(&mut para, &mut para_outline, &mut out),
                        "uc" => top.uc = param.unwrap_or(1).max(0) as usize,
                        "u" => {
                            if let Some(p) = param {
                                let code = if p < 0 { (p + 65536) as u32 } else { p as u32 };
                                if let Some(ch) = char::from_u32(code) {
                                    para.push(ch);
                                }
                                pending_skip_chars = top.uc;
                            }
                        }
                        "outlinelevel" => {
                            para_outline = param.map(|p| (p.clamp(0, 8) + 1) as u8);
                            top.outline = para_outline;
                        }
                        "emdash" => para.push('\u{2014}'),
                        "endash" => para.push('\u{2013}'),
                        "bullet" => para.push('\u{2022}'),
                        "lquote" => para.push('\u{2018}'),
                        "rquote" => para.push('\u{2019}'),
                        "ldblquote" => para.push('\u{201C}'),
                        "rdblquote" => para.push('\u{201D}'),
                        _ => {}
                    }
                } else {
                    i += 1;
                    match c {
                        b'*' => star = true,
                        b'\'' => {
                            if i + 1 < bytes.len() {
                                let hex = std::str::from_utf8(&bytes[i..i + 2]).unwrap_or("");
                                i += 2;
                                if g.skip {
                                    continue;
                                }
                                if pending_skip_chars > 0 {
                                    pending_skip_chars -= 1;
                                    continue;
                                }
                                if let Ok(v) = u8::from_str_radix(hex, 16) {
                                    para.push_str(&super::text::decode(&[v]));
                                }
                            }
                        }
                        b'\\' | b'{' | b'}' if !g.skip => para.push(c as char),
                        b'~' if !g.skip => para.push(' '),
                        b'_' if !g.skip => para.push('-'),
                        b'\n' | b'\r' if !g.skip => finish(&mut para, &mut para_outline, &mut out),
                        _ => {}
                    }
                }
            }
            b'\r' | b'\n' => i += 1,
            _ => {
                i += 1;
                if g.skip {
                    continue;
                }
                if pending_skip_chars > 0 {
                    pending_skip_chars -= 1;
                    continue;
                }
                para.push(b as char);
            }
        }
    }
    finish(&mut para, &mut para_outline, &mut out);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_body_text_only() {
        // Built with format! so the source never contains a literal backslash-u escape.
        let bs = "BS";
        let rtf = format!(
            "{{BSrtf1BSansi{{BSfonttbl{{BSf0 Times;}}}}{{BScolortbl;BSred0;}}{{BSheader Running headBSpar}}{{BS*BSgenerator Word;}}\nBSpardBSoutlinelevel0 Annual ReportBSpar\nBSpard The company earned GHBSu8373? 5m in cafBS'e9 sales BSu8212? a record.BSpar\nSecondBSline para with BS{{bracesBS}}.BSpar}}"
        )
        .replace(bs, "BACKSLASH");
        let rtf = rtf.replace("BACKSLASH", &char::from(92).to_string());
        let s = to_paragraphs(rtf.as_bytes()).unwrap();
        let v: Vec<(SectionKind, &str)> = s.iter().map(|x| (x.kind, x.text.as_str())).collect();
        assert_eq!(
            v,
            vec![
                (SectionKind::Heading, "Annual Report"),
                (SectionKind::Paragraph, "The company earned GH₵ 5m in café sales — a record."),
                (SectionKind::Paragraph, "Second para with {braces}."),
            ]
        );
    }
}
