#!/usr/bin/env bash
# SPDX-License-Identifier: MPL-2.0
#
# Sign release APKs with the project keystore, if one is configured (R01-REL).
#
#   sign.sh <apk>...
#
# Reads the keystore from the environment (GitHub secrets, never files in git):
#   ANDROID_KEYSTORE_BASE64     base64 of the .jks/.p12 keystore
#   ANDROID_KEY_ALIAS           key alias
#   ANDROID_KEYSTORE_PASSWORD   keystore password
#   ANDROID_KEY_PASSWORD        key password (defaults to the keystore password)
#
# With a keystore every APK is re-signed in place (v1+v2+v3) and verified.
# Without one the APKs keep Flutter's debug signature. Prints "release" or
# "debug" on stdout; everything else goes to stderr.
#
# Needs ANDROID_HOME (build-tools with zipalign and apksigner).
set -euo pipefail

die() { printf 'sign.sh: %s\n' "$*" >&2; exit 1; }
[ $# -ge 1 ] || { sed -n '4,18p' "$0" >&2; exit 2; }

sdk="${ANDROID_HOME:-${ANDROID_SDK_ROOT:-}}"
[ -n "$sdk" ] || die "ANDROID_HOME is not set"
bt="$(find "$sdk/build-tools" -mindepth 1 -maxdepth 1 -type d | sort -V | tail -n1)"
[ -x "$bt/apksigner" ] || die "apksigner not found under $sdk/build-tools"

if [ -z "${ANDROID_KEYSTORE_BASE64:-}" ]; then
    printf 'sign.sh: no ANDROID_KEYSTORE_BASE64, APKs stay debug-signed\n' >&2
    for apk in "$@"; do "$bt/apksigner" verify --print-certs "$apk" | sed 's/^/  /' >&2; done
    echo debug
    exit 0
fi

[ -n "${ANDROID_KEY_ALIAS:-}" ] || die "ANDROID_KEY_ALIAS is not set"
[ -n "${ANDROID_KEYSTORE_PASSWORD:-}" ] || die "ANDROID_KEYSTORE_PASSWORD is not set"
export PA_KS_PASS="$ANDROID_KEYSTORE_PASSWORD"
export PA_KEY_PASS="${ANDROID_KEY_PASSWORD:-$ANDROID_KEYSTORE_PASSWORD}"

work="$(mktemp -d "${RUNNER_TEMP:-/tmp}/sign.XXXXXX")"
trap 'rm -rf "$work"' EXIT
printf '%s' "$ANDROID_KEYSTORE_BASE64" | base64 -d > "$work/release.keystore" \
    || die "ANDROID_KEYSTORE_BASE64 is not valid base64"

for apk in "$@"; do
    # Page-align native libraries (16 KB pages) before signing; apksigner keeps alignment.
    "$bt/zipalign" -f -P 16 4 "$apk" "$work/aligned.apk"
    # Passwords go through the environment, never the command line.
    "$bt/apksigner" sign \
        --ks "$work/release.keystore" --ks-key-alias "$ANDROID_KEY_ALIAS" \
        --ks-pass env:PA_KS_PASS --key-pass env:PA_KEY_PASS \
        --v1-signing-enabled true --v2-signing-enabled true --v3-signing-enabled true \
        --out "$apk" "$work/aligned.apk"
    rm -f "$apk.idsig"
    "$bt/apksigner" verify --verbose --print-certs "$apk" | sed 's/^/  /' >&2
done
echo release
