#!/usr/bin/env bash
# SPDX-License-Identifier: MPL-2.0
# Sign a pushed image with cosign (key-based, signature stored as <repo>:sha256-<digest>.sig,
# the format containers/image and therefore bootc verify).
#   distro/scripts/sign-image.sh REPO@sha256:DIGEST [extra cosign args...]
#
# Env: COSIGN_PRIVATE_KEY  PEM of the encrypted private key (never a file in the repo)
#      COSIGN_PASSWORD     its password (may be empty for the throwaway test key)
#      COSIGN_TLOG=false   do not upload to the public Rekor log (tests only)
# Registry credentials must already be in place (`cosign login`).
set -euo pipefail
source "$(dirname "$0")/lib.sh"

REF="${1:?image reference with digest required}"
shift
[[ "${REF}" == *@sha256:* ]] || die "sign by digest, not by tag: ${REF}"
[[ -n "${COSIGN_PRIVATE_KEY:-}" ]] || die "COSIGN_PRIVATE_KEY is not set"
command -v cosign >/dev/null || die "cosign not found"
export COSIGN_PASSWORD="${COSIGN_PASSWORD:-}"

log "Signing ${REF}"
cosign sign --yes --key env://COSIGN_PRIVATE_KEY \
    --tlog-upload="${COSIGN_TLOG:-true}" "$@" "${REF}"
