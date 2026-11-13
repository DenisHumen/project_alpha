#!/usr/bin/env bash
# SPDX-License-Identifier: MPL-2.0
#
# Download the third-party licence texts shipped with the app (R01-REL).
#
#   fetch-licences.sh <dir>
#
# Texts come from SPDX license-list-data at a pinned tag and are verified by
# SHA-256, so the repository does not carry copies of foreign licence texts.
# Which component uses which licence: THIRD-PARTY-NOTICES.md.
set -euo pipefail

SPDX_TAG="v3.29.0"
LICENCES="
5749785c8bdefafcb5d798270ed0a967036fe2ca63dcedade1627565dfef81d2 LGPL-2.1-or-later
996af0513df21f7496288951c41428a03c174e9e4a9d63665c57d670f845ccb1 LGPL-3.0-or-later
fb981668c18a279e285fc4d83fba1e836cc84dd4daa73c9697d3cfd2d8aca6e0 GPL-3.0-or-later
074e6e32c86a4c0ef8b3ed25b721ca23aca83df277cd88106ef7177c354615ff Apache-2.0
f32fb3b417a194167cfad068223fc975ba96c5960513a10f66a3c28720aec1df BSD-2-Clause
521c6f0ed8e64736684f89843d16a4df6b4b7449acbcfc6ca6630a3037ca8c53 ISC
b05785f9f18e6716bab63424b11454513b9943a222595b70411009202fc592b5 MIT
ced6622122ce451cb1ea0c3c3f507a640e2a44c075c04900ddd9fae8acb5369f FTL
"

[ $# -eq 1 ] || { sed -n '4,10p' "$0" >&2; exit 2; }
dest="$1"
mkdir -p "$dest"

while read -r sum id; do
    [ -n "$id" ] || continue
    f="$dest/$id.txt"
    if [ -f "$f" ] && printf '%s  %s\n' "$sum" "$f" | sha256sum -c --quiet - 2>/dev/null; then
        continue
    fi
    curl -fsSL --retry 3 -o "$f.part" \
        "https://raw.githubusercontent.com/spdx/license-list-data/$SPDX_TAG/text/$id.txt"
    printf '%s  %s\n' "$sum" "$f.part" | sha256sum -c --quiet - \
        || { echo "fetch-licences.sh: checksum mismatch for $id" >&2; exit 1; }
    mv "$f.part" "$f"
done <<< "$LICENCES"
