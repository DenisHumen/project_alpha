#!/usr/bin/env bash
# SPDX-License-Identifier: MPL-2.0
#
# PLAY-APL-001: media for the PlayerEngine contract suite (CASES needs two
# items of at least 10 s, two audio tracks, one subtitle track that is off,
# an external SRT). Generated, never committed.
#
# Usage: scripts/make-fixtures.sh <out-dir>     (needs ffmpeg)
#   contract-1.mkv  12 s, contract-2.mkv  10 s: H.264 320x240,
#   AAC stereo (eng, default) + AC-3 stereo (rus), SRT (eng, not default)
#   external.srt

set -euo pipefail

out="${1:?usage: $0 <out-dir>}"
mkdir -p "$out"
command -v ffmpeg >/dev/null || { echo "ffmpeg not found" >&2; exit 1; }

# libx264 if this FFmpeg has it (test media only, never shipped); else the
# built-in MPEG-4 Part 2 encoder.
if ffmpeg -hide_banner -encoders 2>/dev/null | grep -q ' libx264 '; then
  vcodec=(-c:v libx264 -preset veryfast -crf 32 -g 24 -pix_fmt yuv420p)
else
  vcodec=(-c:v mpeg4 -q:v 8 -g 24)
fi

cat >"$out/embedded.srt" <<'EOF'
1
00:00:01,000 --> 00:00:03,000
Embedded subtitle

2
00:00:05,000 --> 00:00:07,000
Second line
EOF

cat >"$out/external.srt" <<'EOF'
1
00:00:00,500 --> 00:00:04,000
External subtitle
EOF

make() {
  local name="$1" seconds="$2"
  ffmpeg -hide_banner -nostdin -loglevel error -y \
    -f lavfi -i "testsrc2=size=320x240:rate=24:duration=$seconds" \
    -f lavfi -i "sine=frequency=440:sample_rate=48000:duration=$seconds" \
    -f lavfi -i "sine=frequency=660:sample_rate=48000:duration=$seconds" \
    -i "$out/embedded.srt" \
    -map 0:v -map 1:a -map 2:a -map 3:s \
    "${vcodec[@]}" \
    -c:a:0 aac -b:a:0 96k -ac:a:0 2 -c:a:1 ac3 -b:a:1 128k -ac:a:1 2 -c:s srt \
    -metadata:s:a:0 language=eng -metadata:s:a:0 title=English \
    -metadata:s:a:1 language=rus -metadata:s:a:1 title=Russian \
    -metadata:s:s:0 language=eng \
    -disposition:a:0 default -disposition:a:1 0 -disposition:s:0 0 \
    -t "$seconds" "$out/$name"
}

make contract-1.mkv 12
make contract-2.mkv 10
rm -f "$out/embedded.srt"
ls -l "$out"
