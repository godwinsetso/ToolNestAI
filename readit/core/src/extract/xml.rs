//! A small pull parser over quick-xml that yields owned, namespace-stripped
//! events with entities already resolved. Office and EPUB parts are simple
//! enough that this keeps each extractor readable.

use crate::error::Result;
use quick_xml::events::Event;
use quick_xml::Reader;
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub enum Ev {
    Start(String, HashMap<String, String>),
    Empty(String, HashMap<String, String>),
    End(String),
    Text(String),
}

pub struct Pull<'a> {
    reader: Reader<&'a [u8]>,
}

fn local(name: &str) -> String {
    match name.rfind(':') {
        Some(i) => name[i + 1..].to_string(),
        None => name.to_string(),
    }
}

fn attrs(e: &quick_xml::events::BytesStart) -> HashMap<String, String> {
    let mut m = HashMap::new();
    for a in e.attributes().with_checks(false).flatten() {
        let raw: String = a.key.as_ref().to_string();
        let key = local(a.key.as_ref());
        let val = a.normalized_value(quick_xml::XmlVersion::Implicit1_0).map(|v| v.into_owned()).unwrap_or_else(|_| {
            a.value.to_string()
        });
        // Unprefixed attributes are stored by name. Prefixed ones (r:id, xml:lang)
        // are stored as "@local" so they never collide with an unprefixed twin.
        if raw.contains(':') {
            m.insert(format!("@{key}"), val);
        } else {
            m.insert(key, val);
        }
    }
    m
}

fn named_entity(name: &str) -> Option<&'static str> {
    Some(match name {
        "amp" => "&",
        "lt" => "<",
        "gt" => ">",
        "quot" => "\"",
        "apos" => "'",
        "nbsp" => "\u{00A0}",
        "ndash" => "\u{2013}",
        "mdash" => "\u{2014}",
        "hellip" => "\u{2026}",
        "rsquo" => "\u{2019}",
        "lsquo" => "\u{2018}",
        "rdquo" => "\u{201D}",
        "ldquo" => "\u{201C}",
        "copy" => "\u{00A9}",
        "reg" => "\u{00AE}",
        "trade" => "\u{2122}",
        "euro" => "\u{20AC}",
        "pound" => "\u{00A3}",
        "bull" => "\u{2022}",
        "middot" => "\u{00B7}",
        "deg" => "\u{00B0}",
        _ => return None,
    })
}

impl<'a> Pull<'a> {
    pub fn new(xml: &'a str) -> Self {
        let mut reader = Reader::from_str(xml);
        let cfg = reader.config_mut();
        cfg.check_end_names = false;
        cfg.expand_empty_elements = false;
        Self { reader }
    }

    /// Next event, or None at end of document. Malformed trailing content ends the stream.
    pub fn next(&mut self) -> Result<Option<Ev>> {
        loop {
            let ev = match self.reader.read_event() {
                Ok(ev) => ev,
                Err(e) => return Err(e.into()),
            };
            return Ok(Some(match ev {
                Event::Start(e) => Ev::Start(local(e.name().as_ref()), attrs(&e)),
                Event::Empty(e) => Ev::Empty(local(e.name().as_ref()), attrs(&e)),
                Event::End(e) => Ev::End(local(e.name().as_ref())),
                Event::Text(t) => Ev::Text(t.xml10_content().into_owned()),
                Event::CData(t) => Ev::Text(t.xml10_content().into_owned()),
                Event::GeneralRef(r) => {
                    if let Ok(Some(c)) = r.resolve_char_ref() {
                        Ev::Text(c.to_string())
                    } else {
                        let name: String = r.as_ref().to_string();
                        match named_entity(&name) {
                            Some(s) => Ev::Text(s.to_string()),
                            None => continue,
                        }
                    }
                }
                Event::Eof => return Ok(None),
                _ => continue,
            }));
        }
    }
}

/// Attribute by local name, accepting either the unprefixed or prefixed form.
pub fn attr<'m>(a: &'m HashMap<String, String>, name: &str) -> Option<&'m String> {
    a.get(name).or_else(|| a.get(&format!("@{name}")))
}

/// Resolve a relative zip path such as `../media/x.png` against a base part path.
pub fn resolve_path(base: &str, target: &str) -> String {
    if let Some(stripped) = target.strip_prefix('/') {
        return stripped.to_string();
    }
    let mut parts: Vec<&str> = base.split('/').collect();
    parts.pop(); // drop the file name
    for seg in target.split('/') {
        match seg {
            "." | "" => {}
            ".." => {
                parts.pop();
            }
            s => parts.push(s),
        }
    }
    parts.join("/")
}

/// Parse a `.rels` part into id -> target.
pub fn relationships(xml: &str) -> Result<HashMap<String, String>> {
    let mut map = HashMap::new();
    let mut p = Pull::new(xml);
    while let Some(ev) = p.next()? {
        if let Ev::Empty(name, a) | Ev::Start(name, a) = ev {
            if name == "Relationship" {
                if let (Some(id), Some(t)) = (a.get("Id"), a.get("Target")) {
                    map.insert(id.clone(), t.clone());
                }
            }
        }
    }
    Ok(map)
}

/// The `.rels` path that belongs to a part, e.g. ppt/slides/slide1.xml ->
/// ppt/slides/_rels/slide1.xml.rels
pub fn rels_path_for(part: &str) -> String {
    match part.rfind('/') {
        Some(i) => format!("{}/_rels/{}.rels", &part[..i], &part[i + 1..]),
        None => format!("_rels/{part}.rels"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_relative_paths() {
        assert_eq!(resolve_path("ppt/slides/slide1.xml", "../notesSlides/notesSlide1.xml"), "ppt/notesSlides/notesSlide1.xml");
        assert_eq!(resolve_path("ppt/presentation.xml", "slides/slide2.xml"), "ppt/slides/slide2.xml");
        assert_eq!(resolve_path("OEBPS/content.opf", "text/ch1.xhtml"), "OEBPS/text/ch1.xhtml");
        assert_eq!(rels_path_for("ppt/slides/slide1.xml"), "ppt/slides/_rels/slide1.xml.rels");
    }

    #[test]
    fn resolves_entities() {
        let mut p = Pull::new("<a>Fish &amp; chips &#8364;5</a>");
        let mut text = String::new();
        while let Some(ev) = p.next().unwrap() {
            if let Ev::Text(t) = ev {
                text.push_str(&t);
            }
        }
        assert_eq!(text, "Fish & chips €5");
    }
}
