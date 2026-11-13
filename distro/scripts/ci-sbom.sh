#!/usr/bin/env bash
# SPDX-License-Identifier: MPL-2.0
# SPDX SBOM of a built image in the rootful podman storage (syft over the mounted rootfs:
# no multi-GB export to an archive).
#   SYFT=/path/to/syft distro/scripts/ci-sbom.sh TAG OUTPUT.spdx.json
set -euo pipefail
source "$(dirname "$0")/lib.sh"

TAG="${1:?tag required}"
OUT="${2:?output file required}"
SYFT="${SYFT:-syft}"

version="$(sudo podman image inspect --format '{{ index .Labels "org.opencontainers.image.version" }}' "${TAG}")"
mnt="$(sudo podman image mount "${TAG}")"
trap 'sudo podman image unmount "${TAG}" >/dev/null' EXIT

log "SBOM of ${TAG} (${version}) -> ${OUT}"
sudo "${SYFT}" scan "dir:${mnt}" --quiet \
    --source-name alpha-os --source-version "${version:-unknown}" \
    -o "spdx-json=${OUT}"
sudo chown "$(id -u):$(id -g)" "${OUT}"
packages="$(python3 -c 'import json,sys; print(len(json.load(open(sys.argv[1]))["packages"]))' "${OUT}")"
log "SBOM: ${packages} packages"
[[ "${packages}" -gt 100 ]] || die "SBOM looks empty (${packages} packages)"
