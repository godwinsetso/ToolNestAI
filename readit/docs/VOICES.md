# VOICES, ENGINES AND LICENCES

## ENGINE

| Component | Version | Licence | Notes |
|---|---|---|---|
| sherpa-onnx on Android (runs Kokoro and Piper on device) | 1.13.8, pinned | Apache-2.0 | Version 2.0.0 silently skips words missing from Kokoro's dictionary ("institutions", "Mensah", "cedis" disappeared in testing). Do not upgrade until fixed. |
| sherpa-onnx on desktop, through the sherpa-rs 0.6.8 crate | 1.12.9 (set by the crate; archive checksum verified) | Apache-2.0 | Statically linked on Windows and macOS. With Kokoro v1.0 the `dict` folder must be passed or the engine exits the process; READIT always passes it. |
| ONNX Runtime (inside sherpa-onnx) | bundled | MIT | |
| espeak-ng data (pronunciation, inside each voice pack) | bundled | GPL-3.0 data files | Shipped as downloaded data, not linked into READIT's code. |
| Android TextToSpeech | system | n/a | Fallback; network voices are labelled. |

## VOICE PACKS IN THE CATALOGUE

Downloaded on demand from the sherpa-onnx GitHub releases, checked against the SHA-256 below before installation.

| Pack | Voices | Download | Licence | SHA-256 |
|---|---|---|---|---|
| Kokoro v1.0 int8 | 16 English: Emma, Isabella, Lily, Alice, George, Lewis, Daniel, Fable (British); Heart, Bella, Sarah, Nicole, Michael, Adam, Liam, Echo (American) | 132.3 MB | Apache-2.0 | `4c3052abaa60943a341f193888cf6abd68787dae6ab8ae5c925a706caa247e4e` |
| Piper en_GB alba (medium) | Alba, female, Scottish English | 67.2 MB | CC BY 4.0 (attribution required) | `fcd45962906933eec4431d3688f7d74aaac8713c87c6717f91fd3b23463aa1a1` |
| Piper en_GB northern_english_male (medium) | Northern, male | 67.2 MB | CC BY-SA 4.0 (attribution, share-alike for the model) | `2bb2c1e709f58c11f17c693b3b38f500e110e7f54f2651774ec48b8d41f12c55` |
| Piper en_GB cori (medium) | Cori, female, British | 67.3 MB | Public domain | `49c9a5361bbdd95d7ca9687c4de11e5908481f65e7c7c368960df79949fdac2b` |

**Excluded after licence review:** Piper en_US ryan (CC BY-NC-SA: non-commercial), en_US lessac (Blizzard licence: non-commercial), en_GB alan, en_GB jenny_dioco and en_US amy (model cards only say "See URL"; add them once the licence is confirmed).

**Ghanaian English:** no openly licensed Ghanaian English neural voice was found. The closest options are the British voices plus READIT's pronunciation dictionary (seeded with cedi, pesewa, Accra, Kumasi, Tamale, Takoradi). A proper Ghanaian voice would mean training a Piper voice from a consenting Ghanaian speaker's recordings (a few hours of clean audio).

## MEASURED QUALITY AND SPEED

Test text: READIT's normalised output for the sample PDF (8 sentences, about 31 seconds of speech), synthesised sentence by sentence as the app does, then transcribed by an offline Whisper tiny.en model. Recall counts content words found in the transcript, excluding numerals (Whisper writes "2.5 million" as digits).

Hardware: 2-core cloud Xeon at 2.1 GHz without AVX-512, 2 threads. Phones will differ; READIT measures on the device itself.

| Voice | Content-word recall | Time to first sentence | Real-time factor |
|---|---|---|---|
| Kokoro Emma (British female) | 0.95 | 2.2 s | 1.22 |
| Kokoro George (British male) | 0.95 | 2.8 s | 1.20 |
| Kokoro Lily (British female) | 0.94 | 2.5 s | 1.14 |
| Kokoro Heart (American female) | 0.94 | 2.3 s | 1.25 |
| Piper Alba | 0.89 | 0.12 s | 0.07 |
| Piper Northern male | 0.92 | 0.12 s | 0.07 |

Words every voice "missed" were Ghanaian terms the recogniser itself does not know (Mensah, cedis, pesewas). A real-time factor above 1 means rendering is slower than speaking; READIT renders ahead while playing, so a voice keeps up on a device only if its factor there is below 1. Hence the in-app **Test** button and the Piper fallback.

Desktop, end to end through the desktop player (whole 3-page sample PDF, 32 sentences, about 80 seconds of speech): Kokoro Emma recall 0.96, first sentence after 2.1 s; Piper Alba recall 0.94, first sentence after 0.12 s.

Reproduce: `python3 tools/voice_check.py --models <dir> --text samples/spoken-sample.txt --out voice-samples --asr <whisper dir>`

## HARDWARE GUIDANCE

| Voice type | RAM in use | Suitable devices |
|---|---|---|
| Phone's built-in | minimal | All |
| Piper medium | about 100 MB | All Android 8+ phones |
| Kokoro int8 | about 350 MB | Recent mid-range and better; confirm with Test |
