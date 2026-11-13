#!/usr/bin/env bash
# SPDX-License-Identifier: MPL-2.0
#
# Generates synthetic test clips for the PLAY-AND-001 manual test plan (no third-party content).
# Needs ffmpeg with libx264 and libx265. These GPL encoders run only on the tester's machine; nothing
# produced here is shipped with the app.
#
# Usage: tools/make-test-media.sh [out-dir] [seconds]
# Serve:  python3 -m http.server 8000 --directory <out-dir>   (then use http://<pc-ip>:8000/<file> in the app)
set -euo pipefail

out=${1:-test-media}
secs=${2:-30}
mkdir -p "$out"

# name:rate pairs. NTSC rates must be exact rationals, otherwise the container stores a rounded value.
rates="23.976:24000/1001 24:24 25:25 29.97:30000/1001 50:50 59.94:60000/1001"

hdr10_params="hdr10=1:repeat-headers=1:colorprim=bt2020:transfer=smpte2084:colormatrix=bt2020nc"
hdr10_params+=":master-display=G(13250,34500)B(7500,3000)R(34000,16000)WP(15635,16450)L(10000000,1):max-cll=1000,400"

for pair in $rates; do
  name=${pair%%:*}
  rate=${pair#*:}
  src="testsrc2=size=1920x1080:rate=$rate:duration=$secs"

  # SDR H.264, MP4 — AFR only.
  ffmpeg -hide_banner -loglevel error -y -f lavfi -i "$src" \
    -c:v libx264 -preset veryfast -pix_fmt yuv420p -g 48 \
    "$out/sdr-h264-1080p-$name.mp4"

  # HDR10 HEVC Main10, MKV — HDR mode + AFR. The pattern is SDR content tagged as PQ: it looks dim on purpose;
  # it only proves that the HDMI output switches to HDR10. Use real HDR samples for picture quality checks.
  ffmpeg -hide_banner -loglevel error -y -f lavfi -i "$src" \
    -c:v libx265 -preset fast -pix_fmt yuv420p10le -profile:v main10 -x265-params "$hdr10_params:log-level=error" \
    -color_primaries bt2020 -color_trc smpte2084 -colorspace bt2020nc \
    "$out/hdr10-hevc-1080p-$name.mkv"
done

# HLG HEVC Main10 (50 fps, broadcast-like).
ffmpeg -hide_banner -loglevel error -y -f lavfi -i "testsrc2=size=1920x1080:rate=50:duration=$secs" \
  -c:v libx265 -preset fast -pix_fmt yuv420p10le -profile:v main10 \
  -x265-params "colorprim=bt2020:transfer=arib-std-b67:colormatrix=bt2020nc:repeat-headers=1:log-level=error" \
  -color_primaries bt2020 -color_trc arib-std-b67 -colorspace bt2020nc \
  "$out/hlg-hevc-1080p-50.mkv"

ls -la "$out"
