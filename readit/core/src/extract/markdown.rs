//! Markdown: headings, paragraphs, list items, tables and code blocks.

use super::clean;
use crate::error::{ReaditError, Result};
use crate::model::{Extracted, LocationKind, RawSection, SectionKind};
use pulldown_cmark::{Event, HeadingLevel, Options, Parser, Tag, TagEnd};

pub fn extract(src: &str) -> Result<Extracted> {
    let sections = sections(src);
    if sections.is_empty() {
        return Err(ReaditError::NoText { detail: "The Markdown file has no readable text.".into() });
    }
    let title = sections.iter().find(|s| s.kind == SectionKind::Heading && s.level == 1).map(|s| s.text.clone());
    Ok(Extracted { title, location_kind: Some(LocationKind::None), sections, ..Default::default() })
}

pub fn sections(src: &str) -> Vec<RawSection> {
    let mut out = Vec::new();
    let mut buf = String::new();
    let mut item_depth = 0usize;
    let mut heading: Option<u8> = None;
    let mut in_code = false;
    let mut row: Vec<String> = Vec::new();
    let mut cell = String::new();
    let mut in_cell = false;

    let push = |out: &mut Vec<RawSection>, kind: SectionKind, level: u8, text: &str| {
        let t = if kind == SectionKind::Code { text.trim().to_string() } else { clean(text) };
        if !t.is_empty() {
            out.push(RawSection { kind, level, text: t, location: 0 });
        }
    };

    for ev in Parser::new_ext(src, Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH) {
        match ev {
            Event::Start(Tag::Heading { level, .. }) => {
                buf.clear();
                heading = Some(match level {
                    HeadingLevel::H1 => 1,
                    HeadingLevel::H2 => 2,
                    HeadingLevel::H3 => 3,
                    HeadingLevel::H4 => 4,
                    HeadingLevel::H5 => 5,
                    HeadingLevel::H6 => 6,
                });
            }
            Event::End(TagEnd::Heading(_)) => {
                push(&mut out, SectionKind::Heading, heading.take().unwrap_or(2), &buf);
                buf.clear();
            }
            Event::Start(Tag::Item) => {
                if item_depth > 0 {
                    push(&mut out, SectionKind::ListItem, 0, &buf);
                    buf.clear();
                }
                item_depth += 1;
            }
            Event::End(TagEnd::Item) => {
                push(&mut out, SectionKind::ListItem, 0, &buf);
                buf.clear();
                item_depth -= 1;
            }
            Event::End(TagEnd::Paragraph) => {
                if item_depth > 0 {
                    buf.push(' ');
                } else {
                    push(&mut out, SectionKind::Paragraph, 0, &buf);
                    buf.clear();
                }
            }
            Event::Start(Tag::CodeBlock(_)) => {
                buf.clear();
                in_code = true;
            }
            Event::End(TagEnd::CodeBlock) => {
                push(&mut out, SectionKind::Code, 0, &buf);
                buf.clear();
                in_code = false;
            }
            Event::Start(Tag::TableRow) | Event::Start(Tag::TableHead) => row.clear(),
            Event::End(TagEnd::TableRow) | Event::End(TagEnd::TableHead) => {
                let cells: Vec<&str> = row.iter().map(|c| c.as_str()).filter(|c| !c.is_empty()).collect();
                if !cells.is_empty() {
                    out.push(RawSection::new(SectionKind::TableRow, cells.join("; "), 0));
                }
            }
            Event::Start(Tag::TableCell) => {
                cell.clear();
                in_cell = true;
            }
            Event::End(TagEnd::TableCell) => {
                row.push(clean(&cell));
                in_cell = false;
            }
            Event::Text(t) | Event::Code(t) => {
                if in_cell {
                    cell.push_str(&t)
                } else {
                    buf.push_str(&t)
                }
            }
            Event::SoftBreak | Event::HardBreak => {
                if in_code {
                    buf.push('\n')
                } else {
                    buf.push(' ')
                }
            }
            _ => {}
        }
    }
    if !buf.trim().is_empty() {
        push(&mut out, SectionKind::Paragraph, 0, &buf);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markdown_structure() {
        let s = sections("# Title\n\nSome *bold* text\nwrapped.\n\n- one\n- two\n  - nested\n\n```\nlet x = 1;\n```\n\n| A | B |\n|---|---|\n| 1 | 2 |\n");
        let v: Vec<(SectionKind, &str)> = s.iter().map(|x| (x.kind, x.text.as_str())).collect();
        assert_eq!(
            v,
            vec![
                (SectionKind::Heading, "Title"),
                (SectionKind::Paragraph, "Some bold text wrapped."),
                (SectionKind::ListItem, "one"),
                (SectionKind::ListItem, "two"),
                (SectionKind::ListItem, "nested"),
                (SectionKind::Code, "let x = 1;"),
                (SectionKind::TableRow, "A; B"),
                (SectionKind::TableRow, "1; 2"),
            ]
        );
    }
}
