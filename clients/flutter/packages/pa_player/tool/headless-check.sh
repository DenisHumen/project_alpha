#!/usr/bin/env bash
# SPDX-License-Identifier: MPL-2.0
# Headless smoke test of the Linux example inside the PLAY-DSK-001 spike image
# (spikes/desktop-player/docker: Flutter, libmpv, sway, grim).
#   docker run --rm -v <pkg-parent>:/src -w /src/pa_player pa-spike-dsk001 \
#     bash tool/headless-check.sh /media/clip.mp4 [/media/a.flac ...]
# Builds example/ (unless already built), runs it on a headless sway for
# PA_EXIT_AFTER seconds per case, prints PA_RESULT lines and saves screenshots
# to example/build/headless/.
set -euo pipefail
cd "$(dirname "$0")/../example"
APP=build/linux/x64/release/bundle/pa_player_example
[[ -x $APP ]] || { flutter pub get >/dev/null; flutter build linux --release | tail -3; }
OUT=build/headless
mkdir -p "$OUT"

export XDG_RUNTIME_DIR=/tmp/xdg-runtime
mkdir -p "$XDG_RUNTIME_DIR" && chmod 700 "$XDG_RUNTIME_DIR"
cat >/tmp/sway.conf <<'EOF'
output HEADLESS-1 resolution 1280x720 position 0 0
default_border none
for_window [app_id=".*"] floating enable, move position 0 0, resize set 1280 720
EOF
WLR_BACKENDS=headless WLR_RENDERER="${WLR_RENDERER:-pixman}" WLR_LIBINPUT_NO_DEVICES=1 \
  sway -c /tmp/sway.conf >"$OUT/sway.log" 2>&1 &
SWAY_PID=$!
trap 'kill $SWAY_PID 2>/dev/null || true' EXIT
for _ in $(seq 50); do [[ -S $XDG_RUNTIME_DIR/wayland-1 ]] && break; sleep 0.2; done
export WAYLAND_DISPLAY=wayland-1 GDK_BACKEND=wayland

run_case() { # name seconds files...
  local name=$1 secs=$2; shift 2
  PA_EXIT_AFTER=$secs "$APP" "$@" >"$OUT/$name.log" 2>&1 &
  local pid=$!
  sleep $((secs - 2))
  grim "$OUT/$name.png" 2>/dev/null || echo "grim failed"
  wait $pid || echo "$name: exit $?"
  grep -h '^PA_RESULT' "$OUT/$name.log" | sed "s/^/$name /" || echo "$name: no result"
}

video=$1; shift
PA_START_MS=5000 PA_SUBTITLE="${video%.*}.srt" run_case video 10 "$video"
# No sound card in a container: decode to the null audio output.
[[ $# -gt 0 ]] && PA_MPV_OPTS="ao=null" run_case music 10 "$@"
