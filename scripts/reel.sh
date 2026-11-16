#!/usr/bin/env bash
# The film on the site's home (site/public/velo.mp4, and its poster): Velo used by its own
# script, scripts/reel.txt, off-screen, with the test loop it hears as its sound. Needs ffmpeg.
#   scripts/reel.sh
set -euo pipefail
cd "$(dirname "$0")/.."
cargo build -p velo
tmp=$(mktemp -d)
seconds=38.5
VELO_DATA="$tmp/data" VELO_AUDIO=synth VELO_CAPTURE_SIZE=1440,810 VELO_CAPTURE_TO="$tmp/end.png" VELO_CAPTURE_AT=$seconds VELO_REEL="$tmp/take.mp4" target/debug/velo
ffmpeg -y -loglevel error -i "$tmp/take.mp4" -stream_loop -1 -f f32le -ar 48000 -ac 1 -i "$tmp/take.f32" -shortest \
  -vf "fade=in:st=0:d=0.6,fade=out:st=37.8:d=0.7" -af "afade=in:st=0:d=0.6,afade=out:st=37.2:d=1.3,volume=0.8" \
  -c:v libx264 -preset slow -crf 23 -pix_fmt yuv420p -c:a aac -b:a 128k -movflags +faststart site/public/velo.mp4
ffmpeg -y -loglevel error -ss 32 -i site/public/velo.mp4 -frames:v 1 -q:v 3 site/public/velo.jpg
echo "site/public/velo.mp4: $(du -h site/public/velo.mp4 | cut -f1)"
