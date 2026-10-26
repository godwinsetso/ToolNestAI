//! HTML and XHTML: readable content only. Scripts, styles, navigation,
//! headers, footers, forms and hidden elements are ignored; nothing is executed.

use super::clean;
use crate::error::{ReaditError, Result};
use crate::model::{Extracted, LocationKind, RawSection, SectionKind};
use ego_tree::NodeRef;
use scraper::{ElementRef, Html, Node};

const SKIP: &[&str] = &[
    "script", "style", "noscript", "template", "nav", "header", "footer", "aside", "form", "button",
    "svg", "iframe", "head", "select", "textarea", "canvas", "object", "embed", "audio", "video",
];
const BLOCKS: &[&str] = &[
    "p", "div", "section", "article", "main", "blockquote", "figure", "figcaption", "ul", "ol", "li",
    "dl", "dt", "dd", "pre", "table", "thead", "tbody", "tfoot", "tr", "td", "th", "caption", "h1",
    "h2", "h3", "h4", "h5", "h6", "hr", "br", "address", "details", "summary", "body", "html",
];

pub fn extract_document(src: &str) -> Result<Extracted> {
    let (title, sections) = parse(src, 0);
    if sections.is_empty() {
        return Err(ReaditError::NoText { detail: "The page has no readable text content.".into() });
    }
    Ok(Extracted { title, location_kind: Some(LocationKind::None), sections, ..Default::default() })
}

/// Parse HTML into sections tagged with `location` (the chapter for EPUB).
pub fn parse(src: &str, location: u32) -> (Option<String>, Vec<RawSection>) {
    let doc = Html::parse_document(src);
    let mut title = None;
    let mut out = Vec::new();
    for node in doc.root_element().descendants() {
        if let Some(el) = ElementRef::wrap(node) {
            if el.value().name() == "title" {
                let t = clean(&el.text().collect::<String>());
                if !t.is_empty() {
                    title = Some(t);
                }
                break;
            }
        }
    }
    let body = doc
        .root_element()
        .descendants()
        .filter_map(ElementRef::wrap)
        .find(|e| e.value().name() == "body")
        .unwrap_or_else(|| doc.root_element());
    let mut buf = String::new();
    walk(*body, &mut buf, &mut out, location);
    flush(&mut buf, &mut out, SectionKind::Paragraph, location);
    if title.is_none() {
        title = out.iter().find(|s| s.kind == SectionKind::Heading).map(|s| s.text.clone());
    }
    (title, out)
}

fn hidden(el: &ElementRef) -> bool {
    let v = el.value();
    v.attr("hidden").is_some()
        || v.attr("aria-hidden") == Some("true")
        || v.attr("style").map(|s| s.replace(' ', "").contains("display:none")).unwrap_or(false)
}

fn flush(buf: &mut String, out: &mut Vec<RawSection>, kind: SectionKind, location: u32) {
    let t = clean(buf);
    buf.clear();
    if !t.is_empty() {
        out.push(RawSection::new(kind, t, location));
    }
}

/// Inline text of an element, skipping hidden and non-content children and,
/// optionally, nested lists (handled separately for list items).
fn inline_text(node: NodeRef<Node>, skip_lists: bool, s: &mut String) {
    for child in node.children() {
        match child.value() {
            Node::Text(t) => s.push_str(t),
            Node::Element(e) => {
                let name = e.name();
                let el = ElementRef::wrap(child).unwrap();
                if SKIP.contains(&name) || hidden(&el) || (skip_lists && (name == "ul" || name == "ol")) {
                    continue;
                }
                if name == "br" {
                    s.push(' ');
                    continue;
                }
                if name == "img" {
                    continue;
                }
                let block = BLOCKS.contains(&name);
                if block {
                    s.push(' ');
                }
                inline_text(child, skip_lists, s);
                if block || name == "td" || name == "th" {
                    s.push(' ');
                }
            }
            _ => {}
        }
    }
}

fn walk(node: NodeRef<Node>, buf: &mut String, out: &mut Vec<RawSection>, loc: u32) {
    for child in node.children() {
        match child.value() {
            Node::Text(t) => buf.push_str(t),
            Node::Element(e) => {
                let name = e.name();
                let el = ElementRef::wrap(child).unwrap();
                if SKIP.contains(&name) || hidden(&el) {
                    continue;
                }
                match name {
                    "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                        flush(buf, out, SectionKind::Paragraph, loc);
                        let mut s = String::new();
                        inline_text(child, false, &mut s);
                        let t = clean(&s);
                        if !t.is_empty() {
                            out.push(RawSection::heading(name[1..].parse().unwrap_or(2), t, loc));
                        }
                    }
                    "p" | "blockquote" | "figcaption" | "dt" | "dd" | "caption" | "address" | "summary" => {
                        flush(buf, out, SectionKind::Paragraph, loc);
                        let mut s = String::new();
                        inline_text(child, false, &mut s);
                        flush(&mut s, out, SectionKind::Paragraph, loc);
                    }
                    "li" => {
                        flush(buf, out, SectionKind::Paragraph, loc);
                        let mut s = String::new();
                        inline_text(child, true, &mut s);
                        flush(&mut s, out, SectionKind::ListItem, loc);
                        for nested in child.children() {
                            if let Some(n) = ElementRef::wrap(nested) {
                                if matches!(n.value().name(), "ul" | "ol") {
                                    walk(nested, buf, out, loc);
                                }
                            }
                        }
                    }
                    "pre" => {
                        flush(buf, out, SectionKind::Paragraph, loc);
                        let t: String = el.text().collect::<String>().trim().to_string();
                        if !t.is_empty() {
                            out.push(RawSection::new(SectionKind::Code, t, loc));
                        }
                    }
                    "tr" => {
                        flush(buf, out, SectionKind::Paragraph, loc);
                        let cells: Vec<String> = child
                            .children()
                            .filter_map(ElementRef::wrap)
                            .filter(|c| matches!(c.value().name(), "td" | "th"))
                            .map(|c| {
                                let mut s = String::new();
                                inline_text(*c, false, &mut s);
                                clean(&s)
                            })
                            .filter(|c| !c.is_empty())
                            .collect();
                        if !cells.is_empty() {
                            out.push(RawSection::new(SectionKind::TableRow, cells.join("; "), loc));
                        }
                    }
                    "br" | "hr" => buf.push(' '),
                    "img" => {}
                    n if BLOCKS.contains(&n) => {
                        flush(buf, out, SectionKind::Paragraph, loc);
                        walk(child, buf, out, loc);
                        flush(buf, out, SectionKind::Paragraph, loc);
                    }
                    _ => walk(child, buf, out, loc), // inline element: keep accumulating
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn readable_content_only() {
        let html = r#"<html><head><title>Guide</title><script>alert(1)</script></head><body>
          <nav>Home | About</nav><header>Site header</header>
          <h1>Customer Due Diligence</h1>
          <div>Loose text in a <b>div</b>.</div>
          <p>Verify <a href="x">identity</a>.</p>
          <ul><li>Name<ul><li>Middle name</li></ul></li><li>Address</li></ul>
          <table><tr><th>Tier</th><th>Check</th></tr><tr><td>High</td><td>Enhanced</td></tr></table>
          <p hidden>secret</p><p style="display: none">also hidden</p>
          <footer>Copyright</footer></body></html>"#;
        let (title, s) = parse(html, 0);
        assert_eq!(title.as_deref(), Some("Guide"));
        let v: Vec<&str> = s.iter().map(|x| x.text.as_str()).collect();
        assert_eq!(
            v,
            vec!["Customer Due Diligence", "Loose text in a div.", "Verify identity.", "Name", "Middle name", "Address", "Tier; Check", "High; Enhanced"]
        );
    }
}
