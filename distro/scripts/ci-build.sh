#!/usr/bin/env bash
# SPDX-License-Identifier: MPL-2.0
# CI build of the NAS OS image: build -> rechunk -> bootc container lint.
#   sudo distro/scripts/ci-build.sh TAG VERSION
#
# Needs rootful podman: rechunk runs in the pinned base image against the host
# containers-storage (/var/lib/containers). Env: ALPHA_SIGNATURE_POLICY, ALPHA_IMAGE_REPO
# (passed to the build, see Containerfile).
set -euo pipefail
export ENGINE=podman
source "$(dirname "$0")/lib.sh"

TAG="${1:?tag required}"
VERSION="${2:?version required}"
RAW="${TAG}-raw"

[[ "$(id -u)" == 0 ]] || die "run as root (rootful podman)"

"${DISTRO_DIR}/scripts/build-image.sh" "${RAW}" "${VERSION}"

# Rechunk: split the image into stable, content-addressed layers so that an update
# downloads only the changed packages (05 §4.2 p.7). The tool ships in the base image.
log "Rechunking ${RAW} -> ${TAG}"
podman run --rm --privileged \
    -v /var/lib/containers:/var/lib/containers \
    "${BASE_IMAGE}" \
    /usr/libexec/bootc-base-imagectl rechunk "${RAW}" "${TAG}"
podman rmi "${RAW}" >/dev/null

log "bootc container lint on the rechunked image"
podman run --rm "${TAG}" bootc container lint

label="$(podman image inspect --format '{{ index .Labels "containers.bootc" }}' "${TAG}")"
[[ "${label}" == 1 ]] || die "rechunked image lost the containers.bootc label"
ver="$(podman run --rm "${TAG}" sed -n 's/^ALPHA_OS_VERSION=//p' /usr/share/alpha/version)"
[[ "${ver}" == "${VERSION}" ]] || die "version in the image is '${ver}', expected '${VERSION}'"
layers="$(podman image inspect --format '{{ len .RootFS.Layers }}' "${TAG}")"
log "Built ${TAG}: version ${VERSION}, ${layers} layers"
