#!/usr/bin/env bash
# The film on the site's home (site/public/velo.mp4, and its poster): Velo used by its own
# script, scripts/reel.txt, off-screen, the camera's moves made afterwards. Its sound is what
# the app hears: the test loop, or a song of yours (the film is then yours to publish only if
# the song is). Needs ffmpeg.
#   scripts/reel.sh                                 the test loop, into site/public
#   OUT=film.mp4 scripts/reel.sh song.mp3 0 798     a song from 0:00; 798: the Output chip's x
#                                                   with that song's name in the top bar
set -euo pipefail
cd "$(dirname "$0")/.."
song="${1:-}"; from="${2:-0}"; output="${3:-670}"
out="${OUT:-site/public/velo.mp4}"
seconds=80
cargo build -p velo
tmp=$(mktemp -d)
sed "s/{output}/$output/; s/{window}/$((output + 8))/" scripts/reel.txt > "$tmp/reel.txt"
VELO_DATA="$tmp/data" VELO_AUDIO="${song:-synth}" VELO_AUDIO_FROM="$from" VELO_CAPTURE_SIZE=1440,810 VELO_CAPTURE_TO="$tmp/end.png" \
  VELO_CAPTURE_AT=$seconds VELO_REEL="$tmp/take.mp4" VELO_REEL_SCRIPT="$tmp/reel.txt" target/debug/velo
if [ -z "$song" ]; then
  sound=(-stream_loop -1 -f f32le -ar 48000 -ac 1 -i "$tmp/take.f32")
else
  # where the song was at the film's first frame: from there, or (under 0) that much later
  lead=$(cat "$tmp/take.lead")
  case "$lead" in -*) sound=(-itsoffset "${lead#-}" -i "$song") ;; *) sound=(-ss "$lead" -i "$song") ;; esac
fi
ffmpeg -y -loglevel error -i "$tmp/take.mp4" "${sound[@]}" -map 0:v -map 1:a -t $seconds \
  -vf "$(cat "$tmp/take.vf"),fade=in:st=0:d=0.5,fade=out:st=$((seconds - 1)):d=1" -af "afade=in:st=0:d=0.4,afade=out:st=$((seconds - 2)):d=2,volume=0.85" \
  -c:v libx264 -preset slow -crf 27 -pix_fmt yuv420p -c:a aac -b:a 160k -movflags +faststart "$out"
ffmpeg -y -loglevel error -ss $((seconds - 4)) -i "$out" -frames:v 1 -q:v 3 "${out%.mp4}.jpg"
echo "$out: $(du -h "$out" | cut -f1)"
