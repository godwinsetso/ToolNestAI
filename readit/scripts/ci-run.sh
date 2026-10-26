#!/usr/bin/env bash
# Run a build step; if it fails, post the end of its output as GitHub error
# annotations so the cause is readable from the API without downloading logs.
log="$(mktemp)"
set -o pipefail
"$@" 2>&1 | tee "$log"
code=${PIPESTATUS[0]}
if [ "$code" -ne 0 ]; then
  tail -n 150 "$log" | grep -v '^\s*$' | tail -n 120 > "$log.tail"
  # Up to 8 annotations of about 3.5 KB each, newest output last.
  split -C 3500 "$log.tail" "$log.part."
  for p in $(ls "$log.part."* | tail -n 8); do
    body=$(sed -e 's/%/%25/g' "$p" | awk '{printf "%s%%0A", $0}')
    echo "::error title=Step failed ($*)::$body"
  done
fi
exit "$code"
