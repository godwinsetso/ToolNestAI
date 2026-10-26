//! Print what READIT extracts from a file and what the speech engine will be sent.
//! Usage: cargo run --example dump -- <file> [--spoken]

use readit_core::segment::ReadingOptions;
use readit_core::ReaditLibrary as Library;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let path = args.get(1).expect("usage: dump <file> [--spoken]");
    let spoken = args.iter().any(|a| a == "--spoken");
    let dir = tempfile_dir();
    let lib = Library::new(dir.clone(), std::env::var("READIT_PDFIUM_DIR").ok()).unwrap();
    match lib.import_file(path.clone(), None) {
        Err(e) => println!("ERROR: {e}"),
        Ok(r) => {
            let d = r.document;
            println!("TITLE: {}\nFORMAT: {:?}  {:?} x{}  sections: {}", d.title, d.format, d.location_kind, d.location_count, d.section_count);
            for w in &d.warnings {
                println!("WARNING: {w}");
            }
            let opts = ReadingOptions { read_speaker_notes: true, ..Default::default() };
            for s in lib.sections(d.id.clone(), 0, 2000).unwrap() {
                println!("[{:>3}] loc {:>2} {:<14} L{} | {}", s.ordinal, s.location, format!("{:?}", s.kind), s.level, s.text);
                if spoken {
                    for g in lib.segments(d.id.clone(), s.ordinal, opts.clone()).unwrap() {
                        println!("        -> {}", g.spoken_text);
                    }
                }
            }
        }
    }
    let _ = std::fs::remove_dir_all(dir);
}

fn tempfile_dir() -> String {
    let d = std::env::temp_dir().join(format!("readit-dump-{}", std::process::id()));
    d.to_string_lossy().into_owned()
}
