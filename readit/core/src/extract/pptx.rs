//! PowerPoint (.pptx) extraction.
//!
//! Slides are read in presentation order (not file-name order). On each
//! slide the title comes first, then the remaining shapes top-to-bottom,
//! left-to-right, then slide furniture (number, date, footer). Speaker notes
//! become `SpeakerNotes` sections so the reader can choose slide content,
//! notes, or both without re-importing.

use super::xml::{attr, relationships, rels_path_for, resolve_path, Ev, Pull};
use super::{clean, zip_text};
use crate::error::{ReaditError, Result};
use crate::model::{Extracted, LocationKind, RawSection, SectionKind};
use std::io::{Read, Seek};
use std::path::Path;

pub fn extract(path: &Path) -> Result<Extracted> {
    let f = std::fs::File::open(path)?;
    let mut zip = zip::ZipArchive::new(f)?;
    let pres = zip_text(&mut zip, "ppt/presentation.xml")?
        .ok_or_else(|| ReaditError::Corrupt { detail: "ppt/presentation.xml is missing".into() })?;
    let pres_rels = relationships(
        &zip_text(&mut zip, "ppt/_rels/presentation.xml.rels")?.unwrap_or_default(),
    )?;

    let mut slide_parts = Vec::new();
    let mut p = Pull::new(&pres);
    while let Some(ev) = p.next()? {
        if let Ev::Empty(n, a) | Ev::Start(n, a) = ev {
            if n == "sldId" {
                if let Some(target) = a.get("@id").and_then(|rid| pres_rels.get(rid)) {
                    slide_parts.push(resolve_path("ppt/presentation.xml", target));
                }
            }
        }
    }
    if slide_parts.is_empty() {
        return Err(ReaditError::NoText { detail: "The presentation has no slides.".into() });
    }

    let mut out = Extracted { location_kind: Some(LocationKind::Slide), ..Default::default() };
    out.location_count = slide_parts.len() as u32;
    let mut first_title = None;
    for (i, part) in slide_parts.iter().enumerate() {
        let n = (i + 1) as u32;
        let Some(xml) = zip_text(&mut zip, part)? else {
            out.warnings.push(format!("Slide {n} is missing from the file and was skipped."));
            continue;
        };
        let shapes = parse_shapes(&xml)?;
        let before = out.sections.len();
        emit_slide(&shapes, n, &mut out.sections, &mut first_title);
        if let Some(notes) = notes_for(&mut zip, part)? {
            for para in notes {
                out.sections.push(RawSection::new(SectionKind::SpeakerNotes, para, n));
            }
        }
        if out.sections.len() == before {
            out.warnings.push(format!("Slide {n} has no text (it may contain only images)."));
        }
    }
    let core = zip_text(&mut zip, "docProps/core.xml")?.unwrap_or_default();
    out.title = core_title(&core)?.or(first_title);
    if out.sections.iter().all(|s| s.kind == SectionKind::PageFurniture) {
        return Err(ReaditError::NoText {
            detail: "The slides contain no readable text. They may be images; export as PDF and use OCR.".into(),
        });
    }
    Ok(out)
}

#[derive(Debug, Default)]
struct Shape {
    placeholder: Option<String>,
    x: i64,
    y: i64,
    has_pos: bool,
    paragraphs: Vec<String>,
    rows: Vec<String>,
}

impl Shape {
    fn is_title(&self) -> bool {
        matches!(self.placeholder.as_deref(), Some("title") | Some("ctrTitle"))
    }
    fn is_furniture(&self) -> bool {
        matches!(self.placeholder.as_deref(), Some("sldNum") | Some("dt") | Some("ftr") | Some("hdr"))
    }
    fn is_body(&self) -> bool {
        matches!(self.placeholder.as_deref(), Some("body") | Some("obj") | Some(""))
    }
}

fn parse_shapes(xml: &str) -> Result<Vec<Shape>> {
    let mut shapes = Vec::new();
    let mut stack: Vec<(String, Shape)> = Vec::new();
    let mut para: Option<String> = None;
    let mut cell: Option<Vec<String>> = None;
    let mut row: Vec<String> = Vec::new();
    let mut in_t = false;
    let mut p = Pull::new(xml);
    while let Some(ev) = p.next()? {
        match ev {
            Ev::Start(n, _) if n == "sp" || n == "graphicFrame" || n == "cxnSp" => {
                stack.push((n, Shape::default()))
            }
            Ev::End(n) if stack.last().map(|(k, _)| *k == n).unwrap_or(false) => {
                let (_, s) = stack.pop().unwrap();
                if !s.paragraphs.is_empty() || !s.rows.is_empty() {
                    shapes.push(s);
                }
            }
            Ev::Empty(n, a) | Ev::Start(n, a) if n == "ph" => {
                if let Some((_, s)) = stack.last_mut() {
                    s.placeholder = Some(a.get("type").cloned().unwrap_or_default());
                }
            }
            Ev::Empty(n, a) | Ev::Start(n, a) if n == "off" => {
                if let Some((_, s)) = stack.last_mut() {
                    if !s.has_pos {
                        s.x = attr(&a, "x").and_then(|v| v.parse().ok()).unwrap_or(0);
                        s.y = attr(&a, "y").and_then(|v| v.parse().ok()).unwrap_or(0);
                        s.has_pos = true;
                    }
                }
            }
            Ev::Start(n, _) if n == "tr" => row.clear(),
            Ev::End(n) if n == "tr" => {
                let cells: Vec<&str> = row.iter().map(|c| c.as_str()).filter(|c| !c.is_empty()).collect();
                if !cells.is_empty() {
                    if let Some((_, s)) = stack.last_mut() {
                        s.rows.push(cells.join("; "));
                    }
                }
            }
            Ev::Start(n, _) if n == "tc" => cell = Some(Vec::new()),
            Ev::End(n) if n == "tc" => {
                if let Some(c) = cell.take() {
                    row.push(c.join(" "));
                }
            }
            Ev::Start(n, _) if n == "p" => para = Some(String::new()),
            Ev::End(n) if n == "p" => {
                if let Some(text) = para.take().map(|t| clean(&t)).filter(|t| !t.is_empty()) {
                    if let Some(c) = cell.as_mut() {
                        c.push(text);
                    } else if let Some((_, s)) = stack.last_mut() {
                        s.paragraphs.push(text);
                    }
                }
            }
            Ev::Start(n, _) if n == "t" => in_t = true,
            Ev::End(n) if n == "t" => in_t = false,
            Ev::Empty(n, _) if n == "br" => {
                if let Some(t) = para.as_mut() {
                    t.push(' ');
                }
            }
            Ev::Text(t) if in_t => {
                if let Some(p) = para.as_mut() {
                    p.push_str(&t);
                }
            }
            _ => {}
        }
    }
    Ok(shapes)
}

fn emit_slide(shapes: &[Shape], n: u32, out: &mut Vec<RawSection>, first_title: &mut Option<String>) {
    let mut titles: Vec<&Shape> = shapes.iter().filter(|s| s.is_title()).collect();
    let mut content: Vec<&Shape> = shapes.iter().filter(|s| !s.is_title() && !s.is_furniture()).collect();
    let furniture: Vec<&Shape> = shapes.iter().filter(|s| s.is_furniture()).collect();
    titles.sort_by_key(|s| (s.y, s.x));
    // Bucket rows by ~1 cm (360000 EMU) so slightly misaligned boxes read left to right.
    content.sort_by_key(|s| (s.y / 360_000, s.x));

    for t in titles {
        let text = t.paragraphs.join(" ");
        if first_title.is_none() {
            *first_title = Some(text.clone());
        }
        out.push(RawSection::heading(2, text, n));
    }
    for s in content {
        let kind = if s.is_body() && s.paragraphs.len() > 1 { SectionKind::ListItem } else { SectionKind::Paragraph };
        for para in &s.paragraphs {
            out.push(RawSection::new(kind, para.clone(), n));
        }
        for row in &s.rows {
            out.push(RawSection::new(SectionKind::TableRow, row.clone(), n));
        }
    }
    for s in furniture {
        for para in &s.paragraphs {
            out.push(RawSection::new(SectionKind::PageFurniture, para.clone(), n));
        }
    }
}

fn notes_for<R: Read + Seek>(zip: &mut zip::ZipArchive<R>, slide_part: &str) -> Result<Option<Vec<String>>> {
    let Some(rels_xml) = zip_text(zip, &rels_path_for(slide_part))? else { return Ok(None) };
    // Find the relationship whose target is a notes slide.
    let rels = relationships(&rels_xml)?;
    let Some(target) = rels.values().find(|t| t.contains("notesSlide")) else { return Ok(None) };
    let part = resolve_path(slide_part, target);
    let Some(xml) = zip_text(zip, &part)? else { return Ok(None) };
    let notes: Vec<String> = parse_shapes(&xml)?
        .into_iter()
        .filter(|s| s.placeholder.as_deref() == Some("body"))
        .flat_map(|s| s.paragraphs)
        .collect();
    Ok(if notes.is_empty() { None } else { Some(notes) })
}

fn core_title(core: &str) -> Result<Option<String>> {
    let mut p = Pull::new(core);
    let mut inside = false;
    let mut s = String::new();
    while let Some(ev) = p.next()? {
        match ev {
            Ev::Start(n, _) if n == "title" => inside = true,
            Ev::End(n) if n == "title" => inside = false,
            Ev::Text(t) if inside => s.push_str(&t),
            _ => {}
        }
    }
    let s = clean(&s);
    Ok(if s.is_empty() { None } else { Some(s) })
}

#[cfg(test)]
mod tests {
    use super::*;

    const SLIDE: &str = r#"<p:sld xmlns:p="p" xmlns:a="a"><p:cSld><p:spTree>
      <p:sp><p:nvSpPr><p:nvPr><p:ph type="body" idx="1"/></p:nvPr></p:nvSpPr>
        <p:spPr><a:xfrm><a:off x="100" y="2000000"/></a:xfrm></p:spPr>
        <p:txBody><a:p><a:r><a:t>Point one</a:t></a:r></a:p><a:p><a:r><a:t>Point two</a:t></a:r></a:p></p:txBody></p:sp>
      <p:sp><p:nvSpPr><p:nvPr><p:ph type="sldNum"/></p:nvPr></p:nvSpPr><p:txBody><a:p><a:fld type="slidenum"><a:t>3</a:t></a:fld></a:p></p:txBody></p:sp>
      <p:sp><p:nvSpPr><p:nvPr><p:ph type="title"/></p:nvPr></p:nvSpPr>
        <p:spPr><a:xfrm><a:off x="100" y="100"/></a:xfrm></p:spPr>
        <p:txBody><a:p><a:r><a:t>Know Your Customer</a:t></a:r></a:p></p:txBody></p:sp>
      <p:graphicFrame><p:xfrm><a:off x="0" y="5000000"/></p:xfrm><a:graphic><a:graphicData><a:tbl>
        <a:tr><a:tc><a:txBody><a:p><a:r><a:t>Risk</a:t></a:r></a:p></a:txBody></a:tc><a:tc><a:txBody><a:p><a:r><a:t>High</a:t></a:r></a:p></a:txBody></a:tc></a:tr>
      </a:tbl></a:graphicData></a:graphic></p:graphicFrame>
    </p:spTree></p:cSld></p:sld>"#;

    #[test]
    fn title_first_then_body_then_table_then_furniture() {
        let shapes = parse_shapes(SLIDE).unwrap();
        let mut out = vec![];
        let mut first = None;
        emit_slide(&shapes, 3, &mut out, &mut first);
        let texts: Vec<(&str, SectionKind)> = out.iter().map(|s| (s.text.as_str(), s.kind)).collect();
        assert_eq!(
            texts,
            vec![
                ("Know Your Customer", SectionKind::Heading),
                ("Point one", SectionKind::ListItem),
                ("Point two", SectionKind::ListItem),
                ("Risk; High", SectionKind::TableRow),
                ("3", SectionKind::PageFurniture),
            ]
        );
        assert!(out.iter().all(|s| s.location == 3));
        assert_eq!(first.as_deref(), Some("Know Your Customer"));
    }
}
