//! Document extraction. Every extractor is local, reads only the file it is
//! given, and never executes embedded content (macros, scripts, links).

pub mod docx;
pub mod epub;
pub mod html;
pub mod markdown;
pub mod pdf;
pub mod pptx;
pub mod rtf;
pub mod text;
mod xml;

use crate::error::{ReaditError, Result};
use crate::model::{DocFormat, Extracted};
use std::io::{Read, Seek};
use std::path::Path;

/// Hard ceiling on what an archive entry may expand to, to defuse zip bombs.
pub const MAX_ENTRY_BYTES: u64 = 200 * 1024 * 1024;

/// Options that change what is extracted. Most reading choices (skipping
/// headers, speaker notes) are applied at reading time instead, so that
/// changing them never needs a re-import.
#[derive(Debug, Clone, Default)]
pub struct ExtractOptions {
    /// Directory holding the PDFium library, if not on the system path.
    pub pdfium_dir: Option<String>,
}

/// Identify a file by its content first and its extension second.
pub fn detect_format(path: &Path, head: &[u8]) -> Result<DocFormat> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .unwrap_or_default();

    if head.starts_with(b"%PDF") || (head.len() > 1024 && find(&head[..1024], b"%PDF").is_some()) {
        return Ok(DocFormat::Pdf);
    }
    if head.starts_with(b"PK\x03\x04") {
        return detect_zip(path, &ext);
    }
    if head.starts_with(&[0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1]) {
        return match ext.as_str() {
            "ppt" | "pps" => Ok(DocFormat::Ppt),
            "doc" | "dot" => Ok(DocFormat::Doc),
            _ => Err(ReaditError::UnsupportedFormat {
                detail: "this is a legacy Microsoft Office file that READIT cannot identify".into(),
            }),
        };
    }
    if head.starts_with(b"{\\rtf") {
        return Ok(DocFormat::Rtf);
    }
    if head.starts_with(&[0x89, b'P', b'N', b'G'])
        || head.starts_with(&[0xFF, 0xD8, 0xFF])
        || head.starts_with(b"GIF8")
        || (head.starts_with(b"RIFF") && head.len() > 11 && &head[8..12] == b"WEBP")
        || head.starts_with(b"II*\0")
        || head.starts_with(b"MM\0*")
    {
        return Ok(DocFormat::Image);
    }
    match ext.as_str() {
        "md" | "markdown" => return Ok(DocFormat::Markdown),
        "html" | "htm" | "xhtml" => return Ok(DocFormat::Html),
        "txt" | "text" | "log" | "csv" => return Ok(DocFormat::Txt),
        _ => {}
    }
    let lower: Vec<u8> = head.iter().take(512).map(|b| b.to_ascii_lowercase()).collect();
    if find(&lower, b"<!doctype html").is_some() || find(&lower, b"<html").is_some() {
        return Ok(DocFormat::Html);
    }
    if looks_like_text(head) {
        return Ok(DocFormat::Txt);
    }
    Err(ReaditError::UnsupportedFormat {
        detail: format!(
            "'.{}' files are not supported. Supported: PDF, Word, PowerPoint, EPUB, RTF, HTML, Markdown, text and images.",
            if ext.is_empty() { "unknown".to_string() } else { ext }
        ),
    })
}

fn detect_zip(path: &Path, ext: &str) -> Result<DocFormat> {
    let f = std::fs::File::open(path)?;
    let mut zip = zip::ZipArchive::new(f)?;
    if let Ok(mut m) = zip.by_name("mimetype") {
        let mut s = String::new();
        let _ = m.read_to_string(&mut s);
        if s.trim() == "application/epub+zip" {
            return Ok(DocFormat::Epub);
        }
    }
    if zip.by_name("word/document.xml").is_ok() {
        return Ok(DocFormat::Docx);
    }
    if zip.by_name("ppt/presentation.xml").is_ok() {
        return Ok(DocFormat::Pptx);
    }
    if zip.by_name("META-INF/container.xml").is_ok() {
        return Ok(DocFormat::Epub);
    }
    if zip.by_name("EncryptionInfo").is_ok() {
        return Err(ReaditError::Encrypted);
    }
    Err(ReaditError::UnsupportedFormat {
        detail: format!(
            "this archive ('.{ext}') is not a Word, PowerPoint or EPUB document"
        ),
    })
}

/// Extract a file. Legacy binary Office formats return a clear conversion message.
pub fn extract(path: &Path, opts: &ExtractOptions) -> Result<(DocFormat, Extracted)> {
    let mut head = vec![0u8; 4096];
    let n = {
        let mut f = std::fs::File::open(path)?;
        read_up_to(&mut f, &mut head)?
    };
    head.truncate(n);
    if n == 0 {
        return Err(ReaditError::NoText { detail: "The file is empty.".into() });
    }
    // OLE container with encryption marker: Office files saved with a password.
    if head.starts_with(&[0xD0, 0xCF, 0x11, 0xE0]) && is_encrypted_ooxml(path) {
        return Err(ReaditError::Encrypted);
    }
    let format = detect_format(path, &head)?;
    let out = match format {
        DocFormat::Pdf => pdf::extract(path, opts)?,
        DocFormat::Docx => docx::extract(path)?,
        DocFormat::Pptx => pptx::extract(path)?,
        DocFormat::Epub => epub::extract(path)?,
        DocFormat::Rtf => rtf::extract(&std::fs::read(path)?)?,
        DocFormat::Html => html::extract_document(&text::decode(&std::fs::read(path)?))?,
        DocFormat::Markdown => markdown::extract(&text::decode(&std::fs::read(path)?))?,
        DocFormat::Txt => text::extract(&std::fs::read(path)?)?,
        DocFormat::Doc | DocFormat::Ppt => {
            return Err(ReaditError::ConversionRequired {
                detail: format!(
                    "Legacy .{} files need converting first. Open the file in Word, PowerPoint, LibreOffice or Google Docs and save it as .{}x, then import that copy.",
                    format.as_str(),
                    format.as_str()
                ),
            })
        }
        DocFormat::Image => {
            let mut e = Extracted::default();
            e.pages_needing_ocr.push(1);
            e.location_count = 1;
            e.warnings.push("This is an image. Its text can only be read after OCR.".into());
            e
        }
    };
    Ok((format, out))
}

/// Encrypted Word/PowerPoint files are OLE containers with an EncryptionInfo stream.
fn is_encrypted_ooxml(path: &Path) -> bool {
    std::fs::read(path)
        .map(|b| {
            let needle: Vec<u8> = "EncryptionInfo".encode_utf16().flat_map(|u| u.to_le_bytes()).collect();
            find(&b, &needle).is_some()
        })
        .unwrap_or(false)
}

fn read_up_to(r: &mut impl Read, buf: &mut [u8]) -> Result<usize> {
    let mut total = 0;
    while total < buf.len() {
        let n = r.read(&mut buf[total..])?;
        if n == 0 {
            break;
        }
        total += n;
    }
    Ok(total)
}

pub(crate) fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

fn looks_like_text(head: &[u8]) -> bool {
    if head.starts_with(&[0xFF, 0xFE]) || head.starts_with(&[0xFE, 0xFF]) || head.starts_with(&[0xEF, 0xBB, 0xBF]) {
        return true;
    }
    let sample = &head[..head.len().min(2048)];
    let control = sample.iter().filter(|&&b| b < 9 || (b > 13 && b < 32)).count();
    control * 100 / sample.len().max(1) < 2
}

/// Read one entry of a zip archive as UTF-8, refusing oversized entries.
pub(crate) fn zip_text<R: Read + Seek>(zip: &mut zip::ZipArchive<R>, name: &str) -> Result<Option<String>> {
    let mut entry = match zip.by_name(name) {
        Ok(e) => e,
        Err(zip::result::ZipError::FileNotFound) => return Ok(None),
        Err(e) => return Err(e.into()),
    };
    if entry.size() > MAX_ENTRY_BYTES {
        return Err(ReaditError::Corrupt { detail: format!("{name} is unreasonably large") });
    }
    let mut buf = Vec::with_capacity(entry.size() as usize);
    entry.by_ref().take(MAX_ENTRY_BYTES).read_to_end(&mut buf)?;
    Ok(Some(String::from_utf8_lossy(&buf).into_owned()))
}

/// Collapse whitespace and strip control characters.
pub fn clean(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut space = false;
    for c in s.chars() {
        let c = match c {
            '\u{00A0}' | '\u{2007}' | '\u{202F}' | '\t' | '\n' | '\r' | '\u{000B}' | '\u{000C}' => ' ',
            '\u{00AD}' | '\u{200B}' | '\u{FEFF}' => continue,
            c if c.is_control() => continue,
            c => c,
        };
        if c == ' ' {
            if !space && !out.is_empty() {
                out.push(' ');
            }
            space = true;
        } else {
            out.push(c);
            space = false;
        }
    }
    if out.ends_with(' ') {
        out.pop();
    }
    out
}
