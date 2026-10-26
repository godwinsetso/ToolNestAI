# READIT FEATURE STATUS

Status as of Phase 1. Android and desktop share the same core, so document, reading-intelligence and library features apply to both unless a row says otherwise. "Implemented" means the code exists; the last column says how it was checked. Nothing here is listed as done on the strength of a screen alone.

Legend for the mode column: **Offline** works with no connection. **Download once** needs internet only to fetch a voice. **Hardware** depends on the phone's speed.

## DOCUMENTS

| Feature | Status | Mode | Verified by |
|---|---|---|---|
| PDF text, page numbers, headings by font size | Implemented | Offline | Rust tests on sample PDF |
| PDF running headers, footers, page numbers tagged and skippable | Implemented | Offline | Rust and JVM tests |
| Word .docx: headings, lists, tables, text boxes, reading order | Implemented | Offline | Rust tests |
| PowerPoint .pptx: slide order, title first, tables, speaker notes (content, notes or both) | Implemented | Offline | Rust and JVM tests |
| EPUB chapters | Implemented | Offline | Rust tests |
| TXT (UTF-8, UTF-16, Windows-1252), RTF, HTML (scripts/nav ignored), Markdown | Implemented | Offline | Rust tests |
| Password-protected, damaged, unsupported files: clear message, nothing half-imported | Implemented | Offline | Rust tests |
| Legacy .doc and .ppt | Detected; user told how to convert | Offline | Rust tests |
| Scanned PDFs and images: detected, pages listed | Implemented | Offline | Rust tests |
| OCR for scanned PDFs and images | **Planned (Phase 2)** | Offline | |
| Import: file picker, multiple files, share menu, "Open with" | Implemented | Offline | Not yet compiled |
| Large documents: paged section loading, nothing loaded whole | Implemented | Offline | Design; needs device test |
| Header/footer filtering for Word | Not needed: Word headers live outside the body and are never read | Offline | Rust test |

## LISTENING

| Feature | Status | Mode | Verified by |
|---|---|---|---|
| Kokoro voices (16 English, British and American) | Implemented | Download once, hardware | Real audio + speech-recognition check |
| Piper voices (Alba, Northern English male, Cori) | Implemented | Download once | Real audio + speech-recognition check |
| Phone's built-in voices as fallback, network voices labelled | Implemented | Offline or online (labelled) | Compiles against Android SDK |
| On-device speed test per voice with plain verdict | Implemented | Offline | Not yet on a device |
| Downloads: size shown, storage check, pause/resume, SHA-256 check, removal | Implemented | Download once | Not yet compiled |
| Play, pause, stop, skip back/forward (5 to 60 s), previous/next paragraph, page/slide/chapter, go to number, position slider | Implemented | Offline | JVM tests |
| Speed 0.5x to 3x, voice change without restarting the document | Implemented | Offline | JVM tests |
| Pitch | Phone voices only (Kokoro and Piper cannot change pitch) | Offline | |
| Sentence highlighting synced to audio, auto-follow | Implemented | Offline | JVM tests (events); UI not yet run |
| Sleep timer: minutes or end of page/slide/chapter | Implemented | Offline | JVM tests |
| Background playback, notification, lock screen, Bluetooth buttons | Implemented (Media3) | Offline | Not yet on a device |
| Calls and other apps: pause and resume; ducking; headphones unplugged pauses | Implemented | Offline | Not yet on a device |
| Resume from last sentence after restart | Implemented | Offline | Rust and JVM tests |
| Reading queue across documents | Data model only; **no UI yet** | | |
| Audio cache of generated speech | Data model and paths only; **not used yet** | | |
| Highlight on the original PDF page image | **Planned** | | |

## READING INTELLIGENCE

| Feature | Status | Verified by |
|---|---|---|
| Numbers, money (cedis and pesewas, dollars, pounds, naira), percentages, dates (day first), times, ranges, ordinals, years | Implemented | Rust tests |
| Abbreviations, acronyms spelled (KYC, FATF) or spoken (ECOWAS), IDs read digit by digit | Implemented | Rust tests |
| Pronunciation dictionary, starter Ghanaian entries, live "READIT will say" preview | Implemented | Rust tests |
| Spelling mode for serials | In the core options; **no UI switch yet** | Rust tests |
| Pauses after headings and paragraphs, per profile | Implemented | Rust and JVM tests |
| Code blocks: read, announce or skip | Implemented (default: announce) | Rust tests |
| Footnote and equation reading modes | **Planned** | |
| Mixed-language documents | **Planned** (English voices only in Phase 1) | |
| Original text is never rewritten; spoken text is separate | Implemented by design | Rust tests |

## LIBRARY, PROFILES, PRIVACY

| Feature | Status | Verified by |
|---|---|---|
| Library: recent, favourites, search, collections, rename, remove (original untouched) | Implemented | Rust tests (data); UI not compiled |
| Bookmarks with notes | Implemented | Rust tests |
| Reading profiles: Study, Work, Relaxed, Accessibility, My Reader | Implemented | Rust tests |
| Theme light/dark/system, text size, high contrast, large controls, TalkBack labels | Implemented | Not yet on a device |
| No account, no analytics, no uploads, cloud backup disabled | Implemented | Manifest and code review |
| Erase all data | Implemented | Rust tests |
| Local database encryption | **Planned**: relies on Android's app sandbox and device encryption for now | |

## DESKTOP APP

| Feature | Status | Verified by |
|---|---|---|
| Windows installer (per-user, WebView2 bootstrapper, file associations for PDF, DOCX, PPTX, EPUB) | Configured | **Built only by CI on Windows; not yet run** |
| Linux .deb and AppImage | Implemented | Built here; .deb installed and used in a clean environment |
| macOS .dmg | Configured, experimental | Not yet built |
| Offline voice bundled with the installer (Piper Cori) | Implemented | Installed .deb test |
| Import: Open dialog, several files, drag and drop, command-line files ("Open with") | Implemented | Command line tested; dialog and drop not yet driven in tests |
| Reader: sentence highlight, follows speech, click any paragraph to read from it, document spine with headings | Implemented | Screenshots of the running app |
| Player: play/pause, skip, paragraph, page, speed, voice, sleep timer, bookmark, volume | Implemented | Desktop player tests and running app |
| Reopens the last document at its saved sentence | Implemented | Running app |
| Voice downloads with progress, pause/resume, checksum, removal; Test speed check; Preview | Implemented | Download and Test done in the installed app |
| Keyboard control of every playback action | Implemented | Running app (Space, Alt+number) |
| Dark mode, high contrast, text size | Implemented | Dark mode screenshot |
| Media keys and Windows media overlay | **Planned** | |
| Phone voices fallback | Not applicable on desktop: a bundled Piper voice is the fallback | |

## NOT IN PHASE 1

| Feature | Phase |
|---|---|
| OCR | 2 |
| My Voice Studio and consent-based voice cloning | 3: consent records and deletion rules already in the core and tested |
| AI reading assistant, summaries, flashcards | 3 |
| Optional cloud voices and sync | 3 |
