#!/usr/bin/env bash
# SPDX-License-Identifier: MPL-2.0
# Runs inside the spike container: builds the app, generates test clips,
# starts a headless sway (Wayland + Xwayland) on the host GPU and runs every
# embedding mode on every clip. Results: out/results.jsonl + out/*.png.
set -euo pipefail
cd "$(dirname "$0")/.."

SECONDS_PER_RUN="${SECONDS_PER_RUN:-15}"
MEDIA=out/media
mkdir -p out "$MEDIA"

# ---- 1. Test clips (generated, never committed) ---------------------------
if [[ ! -f $MEDIA/h264_1080p30_sdr.mp4 ]]; then
  nice ffmpeg -loglevel error -y \
    -f lavfi -i "testsrc2=size=1920x1080:rate=30" \
    -f lavfi -i "sine=frequency=440:sample_rate=48000" -t 20 \
    -c:v libx264 -preset veryfast -pix_fmt yuv420p -c:a aac \
    "$MEDIA/h264_1080p30_sdr.mp4"
fi
if [[ ! -f $MEDIA/hevc10_2160p24_hdr10.mkv ]]; then
  nice ffmpeg -loglevel error -y \
    -f lavfi -i "testsrc2=size=3840x2160:rate=24" -t 10 \
    -vf "format=yuv420p10le" -c:v libx265 -preset ultrafast \
    -x265-params "log-level=error:pools=2:colorprim=bt2020:transfer=smpte2084:colormatrix=bt2020nc:hdr10=1:master-display=G(13250,34500)B(7500,3000)R(34000,16000)WP(15635,16450)L(10000000,1):max-cll=1000,400" \
    -color_primaries bt2020 -color_trc smpte2084 -colorspace bt2020nc \
    "$MEDIA/hevc10_2160p24_hdr10.mkv"
fi

# ---- 2. Build ---------------------------------------------------------------
flutter pub get >/dev/null
nice flutter build linux --release 2>&1 | tail -3
APP=build/linux/x64/release/bundle/desktop_player

# ---- 3. Headless compositor -------------------------------------------------
export XDG_RUNTIME_DIR=/tmp/xdg-runtime
mkdir -p "$XDG_RUNTIME_DIR" && chmod 700 "$XDG_RUNTIME_DIR"
cat >/tmp/sway.conf <<'EOF'
output HEADLESS-1 resolution 1920x1080 position 0 0
xwayland enable
default_border none
for_window [app_id=".*"] floating enable, move position 0 0
for_window [class=".*"] floating enable, move position 0 0
EOF
WLR_RENDER_DRM_DEVICE="${WLR_RENDER_DRM_DEVICE:-$(ls /dev/dri/renderD* 2>/dev/null | head -1)}" \
  WLR_BACKENDS=headless WLR_RENDERER="${WLR_RENDERER:-gles2}" WLR_LIBINPUT_NO_DEVICES=1 \
  sway -c /tmp/sway.conf >out/sway.log 2>&1 &
SWAY_PID=$!
trap 'kill $SWAY_PID 2>/dev/null || true' EXIT
for _ in $(seq 50); do
  [[ -S $XDG_RUNTIME_DIR/wayland-1 ]] && break
  sleep 0.2
done
export WAYLAND_DISPLAY=wayland-1
export SWAYSOCK
SWAYSOCK=$(ls "$XDG_RUNTIME_DIR"/sway-ipc.*.sock)
export DISPLAY=:0   # sway spawns Xwayland lazily on the first X11 client

vainfo 2>&1 | grep -E "Driver version|VAProfileHEVCMain10" | head -2 || true

# Plain X server (no XWayland) to check X11 child-window composition semantics.
# Xvfb has no DRI3, so GL there is llvmpipe: use it for "what is visible",
# not for performance.
if [[ " ${BACKENDS:-wayland x11} " == *" xvfb "* ]]; then
  Xvfb :99 -screen 0 1920x1080x24 >out/xvfb.log 2>&1 &
  sleep 1
fi

# ---- 4. Runs ----------------------------------------------------------------
: >out/results.jsonl
for clip in "$MEDIA"/${CLIPS:-*}; do
  for backend in ${BACKENDS:-wayland x11}; do
    for mode in ${MODES:-texture native}; do
      name="$(basename "${clip%.*}")-$backend-$mode"
      echo "== $name"
      env_args=(GDK_BACKEND=x11)
      case $backend in
        wayland) env_args=(GDK_BACKEND=wayland) ;;
        # gpu-next refuses llvmpipe unless allowed; Xvfb has no DRI3/Vulkan WSI.
        xvfb) env_args=(GDK_BACKEND=x11 DISPLAY=:99 WAYLAND_DISPLAY=
                        "SPIKE_MPV_OPTS=gpu-sw=yes;gpu-api=opengl;gpu-context=x11egl") ;;
      esac
      env "${env_args[@]}" SPIKE_MODE=$mode SPIKE_FILE="$clip" \
        SPIKE_SECONDS="$SECONDS_PER_RUN" SPIKE_AO=null \
        "$APP" >"out/$name.log" 2>&1 &
      pid=$!
      sleep $((SECONDS_PER_RUN - 5))
      # Focus test: focus the Flutter window, send Space; the OSD reports the
      # last key Flutter saw (native mode: does the mpv surface steal input?).
      # Synthetic input in a headless session is best-effort only.
      case $backend in
        wayland)
          swaymsg "[title=\"^desktop_player\$\"] focus" >/dev/null 2>&1 || true
          wtype -k space >/dev/null 2>&1 || echo "wtype failed" ;;
        x11)
          swaymsg "[title=\"^desktop_player\$\"] focus" >/dev/null 2>&1 || true
          xdotool key space >/dev/null 2>&1 || echo "xdotool failed" ;;
        xvfb)
          DISPLAY=:99 xdotool search --name '^desktop_player$' windowfocus key space \
            >/dev/null 2>&1 || echo "xdotool failed" ;;
      esac
      sleep 1
      case $backend in
        x11) xwininfo -root -tree >"out/$name.xtree" 2>&1 ;;
        xvfb) DISPLAY=:99 xwininfo -root -tree >"out/$name.xtree" 2>&1 ;;
      esac
      if [[ $backend == xvfb ]]; then
        DISPLAY=:99 import -window root "out/$name.png" 2>/dev/null || echo "import failed"
      else
        grim "out/$name.png" 2>/dev/null || echo "grim failed"
      fi
      wait $pid || echo "exit code $?"
      grep -h '^SPIKE_RESULT ' "out/$name.log" | sed "s/^SPIKE_RESULT /{\"run\":\"$name\",\"r\":/; s/\$/}/" >>out/results.jsonl \
        || echo "no result for $name"
    done
  done
done
echo "done: $(wc -l <out/results.jsonl) results"
