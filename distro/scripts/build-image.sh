#!/usr/bin/env bash
# SPDX-License-Identifier: MPL-2.0
# Build the NAS OS container image.
#   distro/scripts/build-image.sh [TAG] [VERSION]
# Defaults: TAG=localhost/alpha-os:dev, VERSION=0.0.0-dev.<timestamp>
set -euo pipefail
source "$(dirname "$0")/lib.sh"

TAG="${1:-localhost/alpha-os:dev}"
VERSION="${2:-0.0.0-dev.$(date -u +%Y%m%d%H%M%S)}"

log "Building ${TAG} (version ${VERSION}) with ${ENGINE}"
nice -n 10 "${ENGINE}" build \
    --build-arg "ALPHA_OS_VERSION=${VERSION}" \
    -f "${DISTRO_DIR}/Containerfile" \
    -t "${TAG}" \
    "${DISTRO_DIR}"
