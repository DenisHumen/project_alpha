#!/usr/bin/env bash
# SPDX-License-Identifier: MPL-2.0
#
# PLAY-AND-002: test media for the instrumented tests of clients/android/player.
#
#   h264.mkv, hevc.mkv  12 s, 320x240 @ 25 fps, keyframe every second;
#                       audio: AAC "eng" (default) and AAC "rus";
#                       subtitles: SRT "eng", not default
#   external.srt        side-loaded subtitle for add_external_subtitle
#
# Usage: tools/make-fixtures.sh OUT_DIR
# Needs an ffmpeg with libx264 and libx265 (FFMPEG overrides the command).
# The files are generated, never committed: the encoders are only used here,
# on the build machine, and are not part of any client.

set -euo pipefail

OUT="${1:?usage: make-fixtures.sh OUT_DIR}"
read -r -a FF <<<"${FFMPEG:-ffmpeg}"
mkdir -p "$OUT"
OUT="$(cd "$OUT" && pwd)"

srt() {
  printf '1\n00:00:00,500 --> 00:00:05,000\n%s 1\n\n2\n00:00:05,500 --> 00:00:11,500\n%s 2\n' "$1" "$1"
}
srt "Embedded subtitle" >"$OUT/embedded.srt"
srt "External subtitle" >"$OUT/external.srt"

for codec in h264 hevc; do
  case "$codec" in
    h264) video=(-c:v libx264 -preset ultrafast -profile:v main) ;;
    hevc) video=(-c:v libx265 -preset ultrafast -x265-params log-level=error) ;;
  esac
  "${FF[@]}" -y -v error \
    -f lavfi -i "testsrc2=size=320x240:rate=25:duration=12" \
    -f lavfi -i "sine=frequency=440:duration=12" \
    -f lavfi -i "sine=frequency=660:duration=12" \
    -i "$OUT/embedded.srt" \
    -map 0:v -map 1:a -map 2:a -map 3:s \
    "${video[@]}" -pix_fmt yuv420p -g 25 \
    -c:a aac -b:a 64k -ac 2 -c:s srt \
    -metadata:s:a:0 language=eng -metadata:s:a:0 title=English \
    -metadata:s:a:1 language=rus -metadata:s:a:1 title=Russian \
    -metadata:s:s:0 language=eng \
    -disposition:a:0 default -disposition:a:1 0 -disposition:s:0 0 \
    "$OUT/$codec.mkv"
done
echo "fixtures: $OUT"
