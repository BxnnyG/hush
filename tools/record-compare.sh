#!/usr/bin/env bash
# Record the raw microphone and the processed "Sordino Mic" at the same time, for quality analysis.
#
#   tools/record-compare.sh [seconds]            (default 45)
#
# Works with the installed daemon (sordino, or the old "hush" build). Files stay on your machine
# in ~/sordino-recordings/<timestamp>/ : raw.wav, processed.wav, state.json.
set -euo pipefail
secs=${1:-45}

ctl=$(command -v sordinoctl || command -v hushctl || true)
[ -n "$ctl" ] || { echo "sordinoctl not found. Is Sordino installed?"; exit 1; }
state=$("$ctl" state)
mic=$(printf '%s' "$state" | python3 -c 'import json,sys; print(json.load(sys.stdin).get("active_mic") or "")')
[ -n "$mic" ] || { echo "Sordino has no active microphone (status: $("$ctl" status | head -1))"; exit 1; }
if pw-cli ls Node 2>/dev/null | grep -q 'node.name = "sordino_mic"'; then out=sordino_mic; else out=hush_mic; fi

dir=$HOME/sordino-recordings/$(date +%Y%m%d-%H%M%S)
mkdir -p "$dir"
printf '%s\n' "$state" > "$dir/state.json"
echo "Microphone: $mic"
echo "Recording to: $dir"
echo
echo "Do this, in a normal speaking voice and your normal position:"
echo "   0-15 s  talk normally (read something aloud)"
echo "  15-25 s  silence (do not move)"
echo "  25-35 s  type on the keyboard / click the mouse"
echo "  35-$secs s  talk again, a bit quieter, and leave a gap between sentences"
echo
echo "Starting in 3 seconds. Use headphones if speakers are on."
sleep 3

pw-record --target "$mic" --rate 48000 --channels 1 "$dir/raw.wav" &
p1=$!
pw-record --target "$out" --rate 48000 --channels 1 "$dir/processed.wav" &
p2=$!
for ((i = secs; i > 0; i--)); do printf '\r%3d s left ' "$i"; sleep 1; done
echo
kill -INT "$p1" "$p2" 2>/dev/null || true
wait "$p1" "$p2" 2>/dev/null || true
echo "Done: $dir"
ls -la "$dir"
