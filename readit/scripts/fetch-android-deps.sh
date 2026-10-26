#!/usr/bin/env bash
# Prepare everything the Android build needs that is not in Maven:
#   - sherpa-onnx speech engine AAR (pinned to 1.13.8)
#   - PDFium shared libraries for each ABI
#   - READIT's Rust core compiled for each ABI, plus fresh Kotlin bindings
# Requirements: Rust (rustup), cargo-ndk, Android NDK (ANDROID_NDK_HOME), curl, tar.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
APP="$ROOT/android/app"
SHERPA_VERSION=1.13.8
PDFIUM_BUILD=7881
ABIS=(arm64-v8a armeabi-v7a x86_64)

mkdir -p "$APP/libs"
AAR="$APP/libs/sherpa-onnx-$SHERPA_VERSION.aar"
if [ ! -f "$AAR" ]; then
  echo "Downloading sherpa-onnx $SHERPA_VERSION"
  curl -fL -o "$AAR" "https://github.com/k2-fsa/sherpa-onnx/releases/download/v$SHERPA_VERSION/sherpa-onnx-$SHERPA_VERSION.aar"
fi

pdfium_name() { case "$1" in arm64-v8a) echo android-arm64;; armeabi-v7a) echo android-arm;; x86_64) echo android-x64;; esac; }
for abi in "${ABIS[@]}"; do
  dest="$APP/src/main/jniLibs/$abi"
  mkdir -p "$dest"
  if [ ! -f "$dest/libpdfium.so" ]; then
    echo "Downloading PDFium for $abi"
    tmp="$(mktemp -d)"
    curl -fsSL "https://github.com/bblanchon/pdfium-binaries/releases/download/chromium%2F$PDFIUM_BUILD/pdfium-$(pdfium_name "$abi").tgz" | tar xz -C "$tmp"
    cp "$tmp/lib/libpdfium.so" "$dest/"
    rm -rf "$tmp"
  fi
done

echo "Building READIT core for Android"
rustup target add aarch64-linux-android armv7-linux-androideabi x86_64-linux-android >/dev/null
command -v cargo-ndk >/dev/null || cargo install cargo-ndk --locked
(cd "$ROOT/core" && cargo ndk -t arm64-v8a -t armeabi-v7a -t x86_64 -P 26 -o "$APP/src/main/jniLibs" build --release --lib)

echo "Generating Kotlin bindings"
(cd "$ROOT/core" && cargo build --lib -q
  LIB=$(ls target/debug/libreadit_core.so target/debug/libreadit_core.dylib target/debug/readit_core.dll 2>/dev/null | head -1)
  cargo run -q --bin uniffi-bindgen -- generate --library "$LIB" \
    --language kotlin --out-dir "$APP/src/main/java" --no-format)

echo "Android native dependencies ready."
