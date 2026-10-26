#!/usr/bin/env bash
# Stage what the desktop installer bundles:
#   resources/pdfium/   PDF engine (PDFium, chromium/7881)
#   resources/voices/   Piper Cori (public domain), so READIT speaks offline from first launch
#   libs/linux/         speech engine shared libraries (Linux packages only)
# Usage: scripts/fetch-desktop-deps.sh [windows|linux|macos]   (default: this machine)
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
RES="$ROOT/desktop/src-tauri/resources"
PDFIUM_BUILD=7881
CORI_ARCHIVE=vits-piper-en_GB-cori-medium
CORI_SHA=49c9a5361bbdd95d7ca9687c4de11e5908481f65e7c7c368960df79949fdac2b

os="${1:-}"
if [ -z "$os" ]; then
  case "$(uname -s)" in
    Linux*) os=linux ;; Darwin*) os=macos ;; MINGW*|MSYS*|CYGWIN*) os=windows ;; *) echo "Unknown OS"; exit 1 ;;
  esac
fi
sha() { if command -v sha256sum >/dev/null; then sha256sum "$1" | cut -d' ' -f1; else shasum -a 256 "$1" | cut -d' ' -f1; fi; }

mkdir -p "$RES/pdfium" "$RES/voices"
tmp="$(mktemp -d)"
case "$os" in
  windows) asset=pdfium-win-x64.tgz; lib=bin/pdfium.dll ;;
  linux)   asset=pdfium-linux-x64.tgz; lib=lib/libpdfium.so ;;
  macos)   asset=pdfium-mac-univ.tgz; lib=lib/libpdfium.dylib ;;
esac
echo "PDFium ($asset)"
curl -fsSL "https://github.com/bblanchon/pdfium-binaries/releases/download/chromium%2F$PDFIUM_BUILD/$asset" | tar xz -C "$tmp"
cp "$tmp/$lib" "$RES/pdfium/"
cp "$tmp/LICENSE" "$RES/pdfium/LICENSE-PDFium.txt" 2>/dev/null || true

if [ ! -f "$RES/voices/$CORI_ARCHIVE/.installed" ]; then
  echo "Bundled voice: Piper Cori"
  curl -fsSL -o "$tmp/cori.tar.bz2" "https://github.com/k2-fsa/sherpa-onnx/releases/download/tts-models/$CORI_ARCHIVE.tar.bz2"
  [ "$(sha "$tmp/cori.tar.bz2")" = "$CORI_SHA" ] || { echo "Checksum mismatch for the bundled voice"; exit 1; }
  tar xjf "$tmp/cori.tar.bz2" -C "$RES/voices"
  echo "$CORI_SHA" > "$RES/voices/$CORI_ARCHIVE/.installed"
fi

if [ "$os" = linux ]; then
  # Speech engine shared libraries for the .deb/.AppImage (Windows and macOS link it statically).
  mkdir -p "$ROOT/desktop/src-tauri/libs/linux"
  src=$(ls -d ~/.cache/sherpa-rs/x86_64-unknown-linux-gnu/*/sherpa-onnx-v1.12.9-linux-x64-shared/lib 2>/dev/null | head -1 || true)
  if [ -z "$src" ]; then
    curl -fsSL "https://github.com/k2-fsa/sherpa-onnx/releases/download/v1.12.9/sherpa-onnx-v1.12.9-linux-x64-shared.tar.bz2" | tar xj -C "$tmp"
    src="$tmp/sherpa-onnx-v1.12.9-linux-x64-shared/lib"
  fi
  cp "$src/libsherpa-onnx-c-api.so" "$src/libonnxruntime.so" "$ROOT/desktop/src-tauri/libs/linux/"
fi
rm -rf "$tmp"
echo "Desktop resources ready for $os."
