#!/usr/bin/env bash
# SPDX-License-Identifier: MPL-2.0
#
# Reproducibly build the bundled anime mapping database (META-008) from the
# pinned anime-offline-database release (ODbL-1.0). The output is a derived
# database under ODbL-1.0 and is NOT committed to git (see README.md).
#
# Usage: data/anime-mappings/build.sh [out-dir]   (default: data/anime-mappings/dist)
#
# Anime-Lists and Fribb are deliberately absent: they have no licence and are
# downloaded by the user's device at runtime (pa-anime-mappings::update).

set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=upstream.env
. "$here/upstream.env"

out="${1:-$here/dist}"
mkdir -p "$out"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

sha256() {
    if command -v sha256sum >/dev/null; then sha256sum "$1"; else shasum -a 256 "$1"; fi | cut -d' ' -f1
}

echo "Downloading anime-offline-database ${AOD_TAG}..."
curl -fsSL --retry 3 -o "$work/upstream.jsonl" "$AOD_URL"
actual="$(sha256 "$work/upstream.jsonl")"
if [ "$actual" != "$AOD_SHA256" ]; then
    echo "SHA-256 mismatch: expected $AOD_SHA256, got $actual" >&2
    exit 1
fi

curl -fsSL --retry 3 -o "$out/LICENSE.anime-offline-database" "$AOD_LICENSE_URL"

cargo run --release --quiet -p pa-anime-mappings --bin anime-mappings-build -- \
    "$work/upstream.jsonl" "$out/anime-offline-ids.jsonl" "$AOD_TAG"

echo "SHA-256 of the bundle: $(sha256 "$out/anime-offline-ids.jsonl")"
