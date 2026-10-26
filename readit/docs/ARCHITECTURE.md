# READIT ARCHITECTURE

## LAYERS

```
Android app (Kotlin)                         Desktop app (Tauri 2)
  ui/          Compose screens                  ui/          HTML, CSS, JavaScript (no framework)
                                                src-tauri/   player.rs (Rust twin of ReaderController)
                                                             voices.rs (catalogue, downloads, sherpa-rs)
                                                             audio.rs (cpal output with resampling)
                                                             main.rs (commands and events)
  playback/    ReaderSession, PlaybackService (Media3), AudioTrackSink
               ReaderController  <- platform-free, tested on the JVM
  tts/         SherpaEngine (Kokoro, Piper), SystemTtsEngine, VoiceManager
  data/        DocumentImporter, CoreDocumentSource
        |
        |  UniFFI-generated Kotlin bindings over JNA (app/readit/core/readit_core.kt)
        v
readit-core (Rust)
  extract/     pdf (PDFium), docx, pptx, epub, html, markdown, rtf, text
  normalize    written text -> what a careful reader would say
  segment      sentence segments with UTF-8 and UTF-16 offsets, reading options
  db           SQLite schema, migrations, all library data
```

**Why a Rust core:** one extraction and normalisation pipeline for Android and desktop, so a bookmark means the same sentence on both; and it can be tested without an Android device.

## READING PIPELINE

1. **Import:** the file is hashed (SHA-256, first 10 bytes as the document id), identified by content, extracted, and copied into private storage. Same file, same id.
2. **Sections:** paragraphs, headings, list items, table rows, speaker notes, code and page furniture, each with its page, slide or chapter. Section id is `<document id>:<ordinal>`.
3. **Segments:** each readable section is split into sentences (abbreviation- and decimal-aware, long sentences split at clauses). Each keeps its exact position in the original text.
4. **Spoken text:** normalisation and the pronunciation dictionary produce the text sent to the voice. The displayed document is never altered.
5. **Playback:** ReaderController renders up to three segments ahead on one thread and plays on another. Every jump increments a generation number so stale audio is dropped. Positions are saved as each sentence starts.

## DATA MODEL (SQLite, migrations by `PRAGMA user_version`)

| Table | Holds |
|---|---|
| documents | title, file name, format, private copy path, size, page/slide/chapter count, warnings, favourite, dates |
| sections, sections_fts | extracted text by ordinal, kind, heading level, location; full-text index |
| reading_positions | last section and sentence per document |
| bookmarks | position, label, note, highlighted text |
| listening_history, queue, collections, collection_items | history and organisation |
| pronunciations | term, replacement, case rule, optional document scope |
| installed_voices | engine, language, accent, licence, size, offline flag, path |
| consent_records, voice_profiles | personal voice consent (Phase 3), revocation |
| reading_profiles | speed, pitch, pauses, notes/slides/furniture choices |
| audio_cache, settings | generated audio index, key-value settings |

## DEPENDENCIES

| Dependency | Use | Licence |
|---|---|---|
| rusqlite (bundled SQLite) | database | MIT / public domain |
| pdfium-render + PDFium binaries (bblanchon, chromium/7881) | PDF text | MIT/Apache-2.0; PDFium BSD-3 |
| zip, quick-xml | DOCX, PPTX, EPUB | MIT |
| scraper, ego-tree | HTML, EPUB chapters | MIT/ISC |
| pulldown-cmark | Markdown | MIT |
| regex, unicode-segmentation, sha2, serde | text and hashing | MIT/Apache-2.0 |
| uniffi | Kotlin bindings | MPL-2.0 |
| JNA 5.17 | native calls from Kotlin | Apache-2.0 / LGPL-2.1 |
| sherpa-onnx 1.13.8 | offline speech | Apache-2.0 |
| AndroidX, Compose, Media3, WorkManager, Kotlin coroutines | app | Apache-2.0 |
| Apache Commons Compress | unpacking voice packs | Apache-2.0 |
| Tauri 2, tauri-plugin-dialog | desktop shell | MIT/Apache-2.0 |
| sherpa-rs 0.6.8 | desktop speech | MIT |
| cpal | desktop audio output | Apache-2.0 |
| ureq (with the system certificate store), tar, bzip2 | desktop voice downloads | MIT/Apache-2.0 |
| Literata, Atkinson Hyperlegible | desktop fonts, bundled | SIL OFL 1.1 |
