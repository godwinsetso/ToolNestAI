//! Word (.docx) extraction: paragraphs, headings (by style or outline level),
//! lists, tables (row by row) and text boxes, in document order. Headers and
//! footers live in separate parts and are therefore not read as body text.

use super::xml::{attr, Ev, Pull};
use super::{clean, zip_text};
use crate::error::{ReaditError, Result};
use crate::model::{Extracted, LocationKind, RawSection, SectionKind};
use std::collections::{HashMap, HashSet};
use std::path::Path;

pub fn extract(path: &Path) -> Result<Extracted> {
    let f = std::fs::File::open(path)?;
    let mut zip = zip::ZipArchive::new(f)?;
    let doc = zip_text(&mut zip, "word/document.xml")?
        .ok_or_else(|| ReaditError::Corrupt { detail: "word/document.xml is missing".into() })?;
    let styles = zip_text(&mut zip, "word/styles.xml")?.unwrap_or_default();
    let core = zip_text(&mut zip, "docProps/core.xml")?.unwrap_or_default();

    let (heading_styles, list_styles) = style_info(&styles)?;
    let mut out = Extracted { location_kind: Some(LocationKind::None), ..Default::default() };
    out.title = core_title(&core)?;
    out.sections = body_sections(&doc, &heading_styles, &list_styles)?;
    if out.sections.is_empty() {
        return Err(ReaditError::NoText {
            detail: "The Word document has no text in its body. If it contains scanned pages as images, export it as PDF and import that.".into(),
        });
    }
    Ok(out)
}

/// styleId -> heading level (from "heading 2", "Title" or outline level), plus
/// the set of styles that carry list numbering ("List Number", "List Bullet").
fn style_info(styles: &str) -> Result<(HashMap<String, u8>, HashSet<String>)> {
    let mut map = HashMap::new();
    let mut lists = HashSet::new();
    let mut p = Pull::new(styles);
    let mut current: Option<String> = None;
    while let Some(ev) = p.next()? {
        match ev {
            Ev::Start(n, a) if n == "style" => current = attr(&a, "styleId").cloned(),
            Ev::End(n) if n == "style" => current = None,
            Ev::Empty(n, a) | Ev::Start(n, a) if current.is_some() => {
                let id = current.clone().unwrap();
                if n == "numPr" {
                    lists.insert(id.clone());
                }
                if n == "name" {
                    if let Some(v) = attr(&a, "val") {
                        let v = v.to_ascii_lowercase();
                        if v.starts_with("list ") || v == "list paragraph" {
                            lists.insert(id.clone());
                        }
                        if v == "title" {
                            map.insert(id, 1);
                        } else if v == "subtitle" {
                            map.insert(id, 2);
                        } else if let Some(l) = v.strip_prefix("heading ") {
                            if let Ok(l) = l.trim().parse::<u8>() {
                                map.insert(id, l.clamp(1, 9));
                            }
                        }
                    }
                } else if n == "outlineLvl" {
                    if let Some(Ok(l)) = attr(&a, "val").map(|v| v.parse::<u8>()) {
                        if l < 9 {
                            map.entry(id).or_insert(l + 1);
                        }
                    }
                }
            }
            _ => {}
        }
    }
    Ok((map, lists))
}

fn core_title(core: &str) -> Result<Option<String>> {
    let mut p = Pull::new(core);
    let mut in_title = false;
    let mut title = String::new();
    while let Some(ev) = p.next()? {
        match ev {
            Ev::Start(n, _) if n == "title" => in_title = true,
            Ev::End(n) if n == "title" => in_title = false,
            Ev::Text(t) if in_title => title.push_str(&t),
            _ => {}
        }
    }
    let t = clean(&title);
    Ok(if t.is_empty() { None } else { Some(t) })
}

#[derive(Default)]
struct Para {
    text: String,
    style: Option<String>,
    list: bool,
    outline: Option<u8>,
}

fn body_sections(doc: &str, heading_styles: &HashMap<String, u8>, list_styles: &HashSet<String>) -> Result<Vec<RawSection>> {
    let mut out = Vec::new();
    let mut p = Pull::new(doc);
    let mut paras: Vec<Para> = Vec::new();
    let mut in_t = false;
    let mut run_depth = 0usize;
    let mut skip_depth = 0usize; // inside mc:Fallback (duplicate of mc:Choice)
    let mut table_depth = 0usize;
    let mut rows: Vec<Vec<String>> = Vec::new(); // current row per table level
    let mut cell: Vec<String> = Vec::new(); // current cell text per table level

    while let Some(ev) = p.next()? {
        if skip_depth > 0 {
            match ev {
                Ev::Start(n, _) if n == "Fallback" => skip_depth += 1,
                Ev::End(n) if n == "Fallback" => skip_depth -= 1,
                _ => {}
            }
            continue;
        }
        match ev {
            Ev::Start(n, _) if n == "Fallback" => skip_depth = 1,
            Ev::Start(n, _) if n == "tbl" => {
                table_depth += 1;
                rows.push(Vec::new());
                cell.push(String::new());
            }
            Ev::End(n) if n == "tbl" => {
                table_depth = table_depth.saturating_sub(1);
                rows.pop();
                cell.pop();
            }
            Ev::Start(n, _) if n == "tr" => {
                if let Some(r) = rows.last_mut() {
                    r.clear();
                }
            }
            Ev::End(n) if n == "tr" => {
                if let Some(r) = rows.last() {
                    let cells: Vec<&str> = r.iter().map(|c| c.as_str()).filter(|c| !c.is_empty()).collect();
                    if !cells.is_empty() {
                        out.push(RawSection::new(SectionKind::TableRow, cells.join("; "), 0));
                    }
                }
            }
            Ev::Start(n, _) if n == "tc" => {
                if let Some(c) = cell.last_mut() {
                    c.clear();
                }
            }
            Ev::End(n) if n == "tc" => {
                if let (Some(c), Some(r)) = (cell.last(), rows.last_mut()) {
                    r.push(clean(c));
                }
            }
            Ev::Start(n, _) if n == "p" => paras.push(Para::default()),
            Ev::Empty(n, _) if n == "p" => {}
            Ev::End(n) if n == "p" => {
                if let Some(para) = paras.pop() {
                    let text = clean(&para.text);
                    if text.is_empty() {
                        continue;
                    }
                    if let Some(parent) = paras.last_mut() {
                        // Text box inside a paragraph: read it after the host text.
                        parent.text.push(' ');
                        parent.text.push_str(&text);
                    } else if table_depth > 0 {
                        if let Some(c) = cell.last_mut() {
                            if !c.is_empty() {
                                c.push(' ');
                            }
                            c.push_str(&text);
                        }
                    } else {
                        let level = para
                            .outline
                            .or_else(|| para.style.as_ref().and_then(|s| heading_styles.get(s).copied()));
                        let section = match level {
                            Some(l) if text.chars().count() <= 300 => RawSection::heading(l, text, 0),
                            _ if para.list || para.style.as_ref().map(|s| list_styles.contains(s)).unwrap_or(false) => {
                                RawSection::new(SectionKind::ListItem, text, 0)
                            }
                            _ => RawSection::new(SectionKind::Paragraph, text, 0),
                        };
                        out.push(section);
                    }
                }
            }
            Ev::Empty(n, a) | Ev::Start(n, a) if n == "pStyle" => {
                if let Some(para) = paras.last_mut() {
                    para.style = attr(&a, "val").cloned();
                }
            }
            Ev::Empty(n, a) | Ev::Start(n, a) if n == "outlineLvl" => {
                if let (Some(para), Some(Ok(l))) = (paras.last_mut(), attr(&a, "val").map(|v| v.parse::<u8>())) {
                    if l < 9 {
                        para.outline = Some(l + 1);
                    }
                }
            }
            Ev::Start(n, _) | Ev::Empty(n, _) if n == "numPr" => {
                if let Some(para) = paras.last_mut() {
                    para.list = true;
                }
            }
            Ev::Start(n, _) if n == "r" => run_depth += 1,
            Ev::End(n) if n == "r" => run_depth = run_depth.saturating_sub(1),
            Ev::Start(n, _) if n == "t" => in_t = true,
            Ev::End(n) if n == "t" => in_t = false,
            Ev::Empty(n, _) if run_depth > 0 && (n == "tab" || n == "br" || n == "cr") => {
                if let Some(para) = paras.last_mut() {
                    para.text.push(' ');
                }
            }
            Ev::Empty(n, _) if run_depth > 0 && n == "noBreakHyphen" => {
                if let Some(para) = paras.last_mut() {
                    para.text.push('-');
                }
            }
            Ev::Text(t) if in_t => {
                if let Some(para) = paras.last_mut() {
                    para.text.push_str(&t);
                }
            }
            _ => {}
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const DOC: &str = r#"<w:document xmlns:w="w"><w:body>
      <w:p><w:pPr><w:pStyle w:val="Heading1"/></w:pPr><w:r><w:t>Introduction</w:t></w:r></w:p>
      <w:p><w:r><w:t xml:space="preserve">Money laundering </w:t></w:r><w:r><w:t>costs GH₵ 2.5m.</w:t></w:r></w:p>
      <w:p><w:pPr><w:numPr><w:ilvl w:val="0"/></w:numPr></w:pPr><w:r><w:t>First point</w:t></w:r></w:p>
      <w:tbl><w:tr><w:tc><w:p><w:r><w:t>Name</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>Role</w:t></w:r></w:p></w:tc></w:tr></w:tbl>
      <w:p><w:r><w:delText>removed</w:delText><w:t>Kept</w:t><w:tab/><w:t>text</w:t></w:r></w:p>
    </w:body></w:document>"#;

    const STYLES: &str = r#"<w:styles xmlns:w="w"><w:style w:type="paragraph" w:styleId="Heading1"><w:name w:val="heading 1"/></w:style></w:styles>"#;

    #[test]
    fn reads_structure_in_order() {
        let (h, l) = style_info(STYLES).unwrap();
        let s = body_sections(DOC, &h, &l).unwrap();
        assert_eq!(s[0], RawSection::heading(1, "Introduction", 0));
        assert_eq!(s[1].text, "Money laundering costs GH₵ 2.5m.");
        assert_eq!(s[2].kind, SectionKind::ListItem);
        assert_eq!(s[3], RawSection::new(SectionKind::TableRow, "Name; Role", 0));
        assert_eq!(s[4].text, "Kept text");
        assert_eq!(s.len(), 5);
    }
}
