#!/usr/bin/env bash
# SPDX-License-Identifier: MPL-2.0
# Host entry point: builds the spike image and runs a command inside it with
# the GPU (/dev/dri) passed through. No host packages or sudo required beyond
# membership in the docker group.
#   tools/run-in-docker.sh                      # full headless benchmark
#   tools/run-in-docker.sh flutter build linux  # any other command
set -euo pipefail
here="$(cd "$(dirname "$0")/.." && pwd)"
image=pa-spike-dsk001

nice docker build -q -t "$image" "$here/docker" >/dev/null

gpu_args=()
if [[ -d /dev/dri ]]; then
  gpu_args+=(--device /dev/dri)
  for node in /dev/dri/renderD* /dev/dri/card*; do
    [[ -e $node ]] && gpu_args+=(--group-add "$(stat -c %g "$node")")
  done
fi

cmd=("$@")
[[ ${#cmd[@]} -eq 0 ]] && cmd=(bash tools/bench-headless.sh)

exec docker run --rm --cpus="${SPIKE_CPUS:-4}" -u "$(id -u):$(id -g)" \
  "${gpu_args[@]}" -v "$here:/src" -w /src \
  -e SECONDS_PER_RUN -e WLR_RENDERER -e BACKENDS -e MODES -e CLIPS \
  "$image" "${cmd[@]}"
