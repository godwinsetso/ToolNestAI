#!/usr/bin/env bash
# Compile the platform-neutral Android code (bindings, reading engine) with
# kotlinc and run it on a desktop JVM against the real Rust core.
# Needs: kotlinc on PATH or KOTLINC, jna.jar at JNA_JAR, PDFium at READIT_PDFIUM_DIR.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
KOTLINC="${KOTLINC:-kotlinc}"
JNA_JAR="${JNA_JAR:?set JNA_JAR to jna.jar}"
OUT="$(mktemp -d)"
(cd "$ROOT/core" && cargo build -q --lib)
SRC="$ROOT/android/app/src/main/java/app/readit"
"$KOTLINC" -nowarn -cp "$JNA_JAR" \
  "$SRC/core/readit_core.kt" "$SRC/playback/Contracts.kt" "$SRC/playback/ReaderController.kt" \
  "$SRC/data/CoreDocumentSource.kt" "$SRC/data/Defaults.kt" \
  "$ROOT/android/jvmtest/ReaderControllerTest.kt" -d "$OUT/test.jar" 2>&1 | grep -v JAVA_TOOL_OPTIONS || true
STDLIB="$(dirname "$(readlink -f "$(command -v "$KOTLINC")")")/../lib/kotlin-stdlib.jar"
java -Djna.library.path="$ROOT/core/target/debug" -cp "$OUT/test.jar:$JNA_JAR:$STDLIB" ReaderControllerTestKt "$ROOT/samples" 2>&1 | grep -v JAVA_TOOL_OPTIONS
