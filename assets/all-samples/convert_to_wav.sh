#!/usr/bin/env bash
# Convert every .mp3 to a .wav alongside it (mono, 44.1kHz, 16-bit PCM).
# Keeps the original .mp3 files. Skips files already converted, so it's
# safe to re-run if interrupted.
set -euo pipefail

convert_one() {
    src="$1"
    dst="${src%.mp3}.wav"
    # Skip if a non-empty wav already exists
    if [[ -s "$dst" ]]; then
        return 0
    fi
    ffmpeg -nostdin -loglevel error -y -i "$src" \
        -ac 1 -ar 44100 -sample_fmt s16 "$dst" \
        && echo "ok  $dst" \
        || echo "FAIL $src"
}
export -f convert_one

find . -type f -iname '*.mp3' -print0 \
    | xargs -0 -P "$(nproc)" -I {} bash -c 'convert_one "$@"' _ {}

echo "=== DONE ==="
