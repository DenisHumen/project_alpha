#!/usr/bin/env bash
# SPDX-License-Identifier: MPL-2.0
#
# CORE-004: check the contents of pa-core-android.aar without a device:
#   - jni/<abi>/libpa_ffi.so for arm64-v8a, armeabi-v7a, x86_64, each an ELF
#     shared object of the right machine; on 64-bit ABIs LOAD segments are
#     aligned to 16 KB (Android 15+ devices with 16 KB pages);
#   - each .so exports the UniFFI scaffolding (core_version checksum);
#   - classes.jar carries the generated Kotlin API (dev.projectalpha.core);
#   - consumer R8 rules keep JNA and the bindings.
#
# Usage: tools/bindings/check-aar.sh [path/to/pa-core-android.aar]
#   Default: $CARGO_TARGET_DIR/android/pa-core-android.aar. Needs unzip and
#   readelf (binutils or llvm-readelf).

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
AAR="${1:-${CARGO_TARGET_DIR:-$ROOT/target}/android/pa-core-android.aar}"
[ -f "$AAR" ] || { echo "no AAR at $AAR; run tools/bindings/build-aar.sh" >&2; exit 1; }

readelf=$(command -v readelf || command -v llvm-readelf || true)
[ -n "$readelf" ] || { echo "readelf not found" >&2; exit 1; }

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
unzip -q "$AAR" -d "$work"

fail=0
err() { echo "FAIL: $*" >&2; fail=1; }

for spec in arm64-v8a:AArch64:64 armeabi-v7a:ARM:32 x86_64:X86-64:64; do
  IFS=: read -r abi machine bits <<<"$spec"
  so="$work/jni/$abi/libpa_ffi.so"
  before=$fail
  if [ ! -f "$so" ]; then err "missing jni/$abi/libpa_ffi.so"; continue; fi
  header="$("$readelf" -h "$so")"
  grep -q 'Type:.*DYN' <<<"$header" || err "$abi: not a shared object"
  grep -Eq "Machine:.*${machine}( |$)" <<<"$header" || err "$abi: wrong machine ($(grep Machine: <<<"$header"))"
  # 64-bit ABIs: every LOAD segment's alignment (last column) must be at
  # least 0x4000. 32-bit ARM never runs with 16 KB pages.
  [ "$bits" = 32 ] || for align in $("$readelf" -lW "$so" | awk '$1 == "LOAD" { print $NF }'); do
    [ $((align)) -ge 16384 ] || err "$abi: LOAD segment aligned to $align, below 16 KB"
  done
  # Not `readelf | grep -q`: grep exits early and pipefail sees SIGPIPE.
  syms="$("$readelf" --dyn-syms -W "$so")"
  grep -q 'uniffi_pa_ffi_checksum_func_core_version' <<<"$syms" ||
    err "$abi: UniFFI symbol uniffi_pa_ffi_checksum_func_core_version not exported"
  [ "$fail" != "$before" ] || echo "ok: jni/$abi/libpa_ffi.so ($machine, $(du -h "$so" | cut -f1))"
done

[ -f "$work/classes.jar" ] || err "missing classes.jar"
if [ -f "$work/classes.jar" ]; then
  classes="$(unzip -Z1 "$work/classes.jar")"
  for class in Pa_ffiKt CoreConfig CoreDirs CoreCallbacks CoreLifecycle PlatformSecretStore; do
    grep -qx "dev/projectalpha/core/$class.class" <<<"$classes" || err "classes.jar: no dev.projectalpha.core.$class"
  done
  echo "ok: classes.jar ($(grep -c '^dev/projectalpha/core/.*\.class$' <<<"$classes") classes in dev.projectalpha.core)"
fi

grep -q 'com.sun.jna' "$work/proguard.txt" 2>/dev/null || err "proguard.txt: no keep rules for JNA"
grep -q 'package="dev.projectalpha.core"' "$work/AndroidManifest.xml" ||
  err "AndroidManifest.xml: package is not dev.projectalpha.core"

[ "$fail" = 0 ] || exit 1
echo "AAR ok: $AAR"
