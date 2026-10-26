#!/usr/bin/env python3
"""Synthesise READIT's normalised sample text with each offline voice, measure
speed, then transcribe the audio with an offline Whisper model to confirm no
words were dropped.

  python3 tools/voice_check.py --models DIR --text spoken.txt --out voice-samples [--asr DIR] [--label TAG]
"""
import argparse
import json
import os
import re
import time

import numpy as np
import sherpa_onnx
import soundfile as sf

VOICES = [
    # (file label, folder, engine, speaker id)
    ("kokoro_v1_bf_emma", "kokoro-int8-multi-lang-v1_0", "kokoro", 21),
    ("kokoro_v1_bm_george", "kokoro-int8-multi-lang-v1_0", "kokoro", 26),
    ("kokoro_v1_bf_lily", "kokoro-int8-multi-lang-v1_0", "kokoro", 23),
    ("kokoro_v1_af_heart", "kokoro-int8-multi-lang-v1_0", "kokoro", 3),
    ("piper_en_GB_alba", "vits-piper-en_GB-alba-medium", "vits", 0),
    ("piper_en_GB_northern_english_male", "vits-piper-en_GB-northern_english_male-medium", "vits", 0),
]


def config(models, folder, engine, threads):
    d = os.path.join(models, folder)
    if engine == "kokoro":
        lex = os.path.join(d, "lexicon-gb-en.txt")
        k = sherpa_onnx.OfflineTtsKokoroModelConfig(
            model=os.path.join(d, "model.int8.onnx"), voices=os.path.join(d, "voices.bin"),
            tokens=os.path.join(d, "tokens.txt"), data_dir=os.path.join(d, "espeak-ng-data"),
            lexicon=lex if os.path.exists(lex) else "")
        m = sherpa_onnx.OfflineTtsModelConfig(kokoro=k, num_threads=threads)
    else:
        onnx = [f for f in os.listdir(d) if f.endswith(".onnx")][0]
        v = sherpa_onnx.OfflineTtsVitsModelConfig(
            model=os.path.join(d, onnx), tokens=os.path.join(d, "tokens.txt"), data_dir=os.path.join(d, "espeak-ng-data"))
        m = sherpa_onnx.OfflineTtsModelConfig(vits=v, num_threads=threads)
    return sherpa_onnx.OfflineTtsConfig(model=m)


def words(s):
    return re.findall(r"[a-z]+", s.lower().replace("-", " "))


def recall(ref, hyp):
    """Share of reference words found, in order, in the transcript (LCS-based)."""
    a, b = words(ref), words(hyp)
    dp = [[0] * (len(b) + 1) for _ in range(len(a) + 1)]
    for i in range(len(a) - 1, -1, -1):
        for j in range(len(b) - 1, -1, -1):
            dp[i][j] = dp[i + 1][j + 1] + 1 if a[i] == b[j] else max(dp[i + 1][j], dp[i][j + 1])
    return dp[0][0] / max(1, len(a))


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--models", required=True)
    ap.add_argument("--text", required=True)
    ap.add_argument("--out", required=True)
    ap.add_argument("--asr")
    ap.add_argument("--label", default=sherpa_onnx.__version__)
    ap.add_argument("--threads", type=int, default=2)
    a = ap.parse_args()
    os.makedirs(a.out, exist_ok=True)
    lines = [l.strip() for l in open(a.text, encoding="utf-8") if l.strip()]
    text = " ".join(l if l.endswith((".", "?", "!")) else l + "." for l in lines)

    recognizer = None
    if a.asr:
        recognizer = sherpa_onnx.OfflineRecognizer.from_whisper(
            encoder=os.path.join(a.asr, "tiny.en-encoder.int8.onnx"),
            decoder=os.path.join(a.asr, "tiny.en-decoder.int8.onnx"),
            tokens=os.path.join(a.asr, "tiny.en-tokens.txt"), num_threads=a.threads)

    results = []
    for label, folder, engine, sid in VOICES:
        if not os.path.isdir(os.path.join(a.models, folder)):
            continue
        t0 = time.time()
        tts = sherpa_onnx.OfflineTts(config(a.models, folder, engine, a.threads))
        load = time.time() - t0
        # READIT speaks one segment at a time, so synthesise and verify per line.
        all_samples, hyp, first, gen, sr = [], [], None, 0.0, 22050
        for line in lines:
            t0 = time.time()
            audio = tts.generate(line, sid=sid, speed=1.0)
            dt = time.time() - t0
            gen += dt
            first = dt if first is None else first
            sr = audio.sample_rate
            seg = np.array(audio.samples, dtype=np.float32)
            all_samples.append(seg)
            all_samples.append(np.zeros(int(sr * 0.25), dtype=np.float32))
            if recognizer is not None and len(seg):
                st = recognizer.create_stream()
                st.accept_waveform(sr, seg)
                recognizer.decode_stream(st)
                hyp.append(st.result.text.strip())
        samples = np.concatenate(all_samples) if all_samples else np.zeros(0, dtype=np.float32)
        dur = len(samples) / sr
        path = os.path.join(a.out, f"{label}.wav")
        if dur > 0:
            sf.write(path, samples, sr, subtype="PCM_16")
        row = {"voice": label, "engine": engine, "engine_version": a.label, "load_s": round(load, 1),
               "first_segment_s": round(first or 0, 2), "audio_s": round(dur, 1), "synth_s": round(gen, 1),
               "rtf": round(gen / dur, 2) if dur else None}
        if recognizer is not None:
            row["word_recall"] = round(recall(text, " ".join(hyp)), 3)
            row["transcript"] = " | ".join(hyp)
        results.append(row)
        print(json.dumps({k: v for k, v in row.items() if k != "transcript"}), flush=True)
    with open(os.path.join(a.out, f"results-{a.label}.json"), "w") as f:
        json.dump({"text": text, "results": results}, f, indent=2)


if __name__ == "__main__":
    main()
