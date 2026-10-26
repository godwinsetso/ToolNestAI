//! EPUB 2/3: chapters in spine order, each parsed as XHTML.
//! DRM-protected books are detected and reported rather than read as gibberish.

use super::xml::{attr, resolve_path, Ev, Pull};
use super::{clean, html, zip_text};
use crate::error::{ReaditError, Result};
use crate::model::{Extracted, LocationKind};
use std::collections::HashMap;
use std::path::Path;

pub fn extract(path: &Path) -> Result<Extracted> {
    let f = std::fs::File::open(path)?;
    let mut zip = zip::ZipArchive::new(f)?;

    if let Some(enc) = zip_text(&mut zip, "META-INF/encryption.xml")? {
        let lower = enc.to_ascii_lowercase();
        if lower.contains(".xhtml") || lower.contains(".html") || lower.contains(".htm\"") {
            return Err(ReaditError::Encrypted);
        }
    }
    let container = zip_text(&mut zip, "META-INF/container.xml")?
        .ok_or_else(|| ReaditError::Corrupt { detail: "META-INF/container.xml is missing".into() })?;
    let opf_path = {
        let mut p = Pull::new(&container);
        let mut found = None;
        while let Some(ev) = p.next()? {
            if let Ev::Empty(n, a) | Ev::Start(n, a) = ev {
                if n == "rootfile" {
                    found = attr(&a, "full-path").cloned();
                    break;
                }
            }
        }
        found.ok_or_else(|| ReaditError::Corrupt { detail: "the EPUB has no package file".into() })?
    };
    let opf = zip_text(&mut zip, &opf_path)?
        .ok_or_else(|| ReaditError::Corrupt { detail: format!("{opf_path} is missing") })?;

    let mut title = None;
    let mut manifest: HashMap<String, (String, String)> = HashMap::new();
    let mut spine: Vec<String> = Vec::new();
    {
        let mut p = Pull::new(&opf);
        let mut in_title = false;
        let mut t = String::new();
        while let Some(ev) = p.next()? {
            match ev {
                Ev::Start(n, _) if n == "title" && title.is_none() => in_title = true,
                Ev::End(n) if n == "title" && in_title => {
                    in_title = false;
                    let c = clean(&t);
                    if !c.is_empty() {
                        title = Some(c);
                    }
                }
                Ev::Text(s) if in_title => t.push_str(&s),
                Ev::Empty(n, a) | Ev::Start(n, a) if n == "item" => {
                    if let (Some(id), Some(href)) = (attr(&a, "id"), attr(&a, "href")) {
                        let media = attr(&a, "media-type").cloned().unwrap_or_default();
                        manifest.insert(id.clone(), (href.clone(), media));
                    }
                }
                Ev::Empty(n, a) | Ev::Start(n, a) if n == "itemref" => {
                    if attr(&a, "linear").map(|v| v != "no").unwrap_or(true) {
                        if let Some(id) = attr(&a, "idref") {
                            spine.push(id.clone());
                        }
                    }
                }
                _ => {}
            }
        }
    }

    let mut out = Extracted { title, location_kind: Some(LocationKind::Chapter), ..Default::default() };
    let mut chapter = 0u32;
    for id in &spine {
        let Some((href, media)) = manifest.get(id) else { continue };
        if !media.contains("html") && !href.ends_with("html") && !href.ends_with(".htm") {
            continue;
        }
        let href = percent_decode(href.split('#').next().unwrap_or(href));
        let part = resolve_path(&opf_path, &href);
        let Some(xhtml) = zip_text(&mut zip, &part)? else {
            out.warnings.push(format!("A chapter file ({href}) is missing and was skipped."));
            continue;
        };
        let (_, sections) = html::parse(&xhtml, chapter + 1);
        if sections.is_empty() {
            continue; // cover images and blank pages do not count as chapters
        }
        chapter += 1;
        out.sections.extend(sections);
    }
    out.location_count = chapter;
    if out.sections.is_empty() {
        return Err(ReaditError::NoText { detail: "The EPUB contains no readable text (it may be image-only).".into() });
    }
    Ok(out)
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}
