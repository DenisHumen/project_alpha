#!/usr/bin/env bash
# SPDX-License-Identifier: MPL-2.0
#
# Licence and linkage gate for the libsmb2 build (NET-002).
#
#   check-license.sh --config <cmake-build-dir>   check a configured tree (CMakeCache.txt)
#   check-license.sh --artifact <out-dir>         check the installed library
#   check-license.sh --run <out-dir>              link check_smb2.c against the artifact,
#                                                 verify the link is dynamic (ldd/otool)
#                                                 and run it (native target only)
#
# Fails (exit 1) unless libsmb2 is a shared library (ELF .so, Mach-O dylib or
# framework, PE DLL) that depends on system libraries only, exports every
# function the Rust wrapper loads, ships its LGPL texts, and no static archive
# of it is present.
set -euo pipefail

PA_SMB2_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=lib/common.sh
. "$PA_SMB2_ROOT/lib/common.sh"
# shellcheck source=lib/smb2.sh
. "$PA_SMB2_ROOT/lib/smb2.sh"

fail=0
bad() { printf 'licence check: FAIL: %s\n' "$*" >&2; fail=1; }
ok() { printf 'licence check: ok: %s\n' "$*" >&2; }

# Dependencies a shared libsmb2 may have: the C runtime and sockets of the OS.
ELF_DEPS_RE='^(libc\.so(\.6)?|libm\.so(\.6)?|libdl\.so(\.2)?|libpthread\.so(\.0)?|libresolv\.so(\.2)?|ld-linux-[a-z0-9_-]+\.so\.[0-9]+|liblog\.so)$'
MACHO_DEPS_RE='^/usr/lib/libSystem\.B\.dylib$'
PE_DEPS_RE='^(kernel32|ws2_32|msvcrt|advapi32|bcrypt|api-ms-win-crt-[a-z0-9-]+)\.dll$'

check_config() {
    local c="$1/CMakeCache.txt"
    [ -f "$c" ] || die "no CMakeCache.txt in $1 (run cmake first)"
    if grep -Eq '^BUILD_SHARED_LIBS:BOOL=(ON|1|TRUE|YES)$' "$c"; then ok "BUILD_SHARED_LIBS=ON"
    else bad "BUILD_SHARED_LIBS is not ON: $(grep '^BUILD_SHARED_LIBS' "$c" || echo missing)"; fi
    local v
    for v in ENABLE_LIBKRB5 ENABLE_GSSAPI; do
        if grep -Eq "^$v:BOOL=(OFF|0|FALSE|NO)$" "$c"; then ok "$v=OFF"
        else bad "$v is not OFF"; fi
    done
}

magic() { od -An -tx1 -N4 "$1" | tr -d ' \n'; }

# deps <lib>: one dependency per line (file names, case kept).
deps() {
    case "$(magic "$1")" in
        7f454c46) readelf -dW "$1" | sed -n 's/.*(NEEDED).*\[\(.*\)\]/\1/p' ;;
        cffaedfe|cefaedfe)
            # First line of `otool -L` on a dylib is its own install name.
            otool -L "$1" | tail -n +2 | awk '{print $1}' | grep -vxF "$(otool -D "$1" | tail -n1)" || true ;;
        4d5a*) "${OBJDUMP:-objdump}" -p "$1" | sed -n 's/^[[:space:]]*DLL Name: //p' ;;
        *) echo "?unknown-format" ;;
    esac
}

# exports <lib>: exported function names, one per line.
exports() {
    case "$(magic "$1")" in
        7f454c46) readelf --dyn-syms -W "$1" | awk '$7 != "UND" && $5 == "GLOBAL" {sub(/@.*/, "", $8); print $8}' ;;
        cffaedfe|cefaedfe) nm -gU "$1" | awk '{print $3}' | sed 's/^_//' ;;
        4d5a*) "${OBJDUMP:-objdump}" -p "$1" | awk '/\[Ordinal\/Name Pointer\] Table/ {t=1; next} t && NF==0 {t=0} t {print $NF}' ;;
    esac
}

# is_shared <lib>: ELF ET_DYN, Mach-O MH_DYLIB, PE with the DLL flag.
is_shared() {
    case "$(magic "$1")" in
        7f454c46) readelf -hW "$1" | grep -Eq 'Type:[[:space:]]+DYN' ;;
        cffaedfe|cefaedfe) otool -hv "$1" | grep -q DYLIB ;;
        4d5a*) "${OBJDUMP:-objdump}" -p "$1" | grep -Eq '^[[:space:]]+DLL$|^Characteristics.*\bDLL\b|^[[:space:]]*DLL$' ;;
        *) return 1 ;;
    esac
}

check_artifact() {
    local out="$1" libs=() f
    [ -d "$out" ] || die "no artifact directory $out"
    while IFS= read -r -d '' f; do libs+=("$f"); done < <(find "$out" -type f \
        \( -name 'libsmb2.so*' -o -name 'libsmb2*.dylib' -o -path '*/smb2.framework/smb2' \
           -o -name 'libsmb2*.dll' -o -name 'smb2.dll' \) -print0)
    [ "${#libs[@]}" -gt 0 ] || { bad "no libsmb2 shared library under $out"; return; }

    # A static archive next to the shared library could be linked by mistake.
    local static
    static="$(find "$out" -type f \( -name 'libsmb2.a' -o -name 'smb2.a' -o -name 'smb2.lib' -o -name 'smb2_static*' \) | head -n1)"
    if [ -n "$static" ]; then bad "static archive present: $static"; else ok "no static archive"; fi

    local re
    for f in "${libs[@]}"; do
        local name; name="${f#"$out"/}"
        if is_shared "$f"; then ok "$name is a shared library"; else bad "$name is not a shared library"; fi
        case "$(magic "$f")" in
            7f454c46) re="$ELF_DEPS_RE" ;;
            cffaedfe|cefaedfe) re="$MACHO_DEPS_RE" ;;
            4d5a*) re="$PE_DEPS_RE" ;;
        esac
        local d any=0
        while IFS= read -r d; do
            [ -n "$d" ] || continue
            any=1
            if printf '%s\n' "$d" | tr '[:upper:]' '[:lower:]' | grep -Eq -- "$re" \
                || printf '%s\n' "$d" | grep -Eq -- "$re"; then
                ok "$name -> $d (system)"
            else
                bad "$name depends on non-system library $d"
            fi
        done < <(deps "$f")
        [ "$any" = 1 ] || bad "$name: could not read its dependencies"

        case "$(magic "$f")" in
            cffaedfe|cefaedfe)
                local id; id="$(otool -D "$f" | tail -n1)"
                case "$id" in
                    @rpath/*) ok "$name install name $id" ;;
                    *) bad "$name install name $id is not @rpath/..." ;;
                esac ;;
        esac

        local have missing="" s
        have="$(exports "$f" | sort -u)"
        for s in $SMB2_REQUIRED_SYMBOLS; do
            printf '%s\n' "$have" | grep -qxF "$s" || missing="$missing $s"
        done
        if [ -z "$missing" ]; then ok "$name exports all $(echo "$SMB2_REQUIRED_SYMBOLS" | wc -w | tr -d ' ') functions used by pa-smb2"
        else bad "$name does not export:$missing"; fi
    done

    for f in libsmb2-COPYING libsmb2-LICENCE-LGPL-2.1.txt; do
        if [ -s "$out/share/licenses/$f" ]; then ok "ships share/licenses/$f"
        else bad "missing share/licenses/$f"; fi
    done
    if grep -q 'GNU LESSER GENERAL PUBLIC LICENSE' "$out/share/licenses/libsmb2-LICENCE-LGPL-2.1.txt" 2>/dev/null \
        && grep -q 'Version 2.1' "$out/share/licenses/libsmb2-LICENCE-LGPL-2.1.txt"; then
        ok "licence text is LGPL 2.1"
    else bad "share/licenses/libsmb2-LICENCE-LGPL-2.1.txt is not the LGPL 2.1"; fi
}

# The "links dynamically" check from NET-002: a program linked against the
# artifact must load libsmb2 from it at run time, not carry a copy.
run_check() {
    local out="$1" tmp bin
    tmp="$(mktemp -d)"
    # shellcheck disable=SC2064
    trap "rm -rf '$tmp'" RETURN
    bin="$tmp/check_smb2"
    # shellcheck disable=SC2086 # CFLAGS/LDFLAGS are word lists
    ${CC:-cc} ${CFLAGS:-} "$PA_SMB2_ROOT/check_smb2.c" -o "$bin" \
        -I"$out/include" ${LDFLAGS:-} -L"$out/lib" -Wl,-rpath,"$out/lib" -lsmb2 \
        || die "failed to compile check_smb2.c"
    if [ "$(uname -s)" = Darwin ]; then
        if otool -L "$bin" | grep -q '@rpath/libsmb2'; then ok "check_smb2 links @rpath/libsmb2 dynamically (otool -L)"
        else bad "check_smb2 does not link libsmb2 dynamically: $(otool -L "$bin" | tr '\n' ' ')"; fi
    else
        local line
        line="$(ldd "$bin" | grep 'libsmb2' || true)"
        if printf '%s\n' "$line" | grep -qF "$out/lib/"; then ok "check_smb2 loads libsmb2 from the artifact (ldd: $(echo "$line" | tr -s ' \t' ' '))"
        else bad "check_smb2 does not load libsmb2 dynamically from $out/lib: ${line:-not in ldd}"; fi
    fi
    if nm "$bin" 2>/dev/null | grep -Eq '[[:space:]]T _?smb2_init_context$'; then
        bad "check_smb2 contains smb2_init_context (static copy)"
    else ok "check_smb2 has no static copy of libsmb2"; fi
    if "$bin"; then ok "libsmb2 runs (context, options, connection error path)"
    else bad "runtime check failed"; fi
}

[ $# -eq 2 ] || { sed -n '4,16p' "$0"; exit 2; }
case "$1" in
    --config) check_config "$2" ;;
    --artifact) check_artifact "$2" ;;
    --run) run_check "$2" ;;
    *) sed -n '4,16p' "$0"; exit 2 ;;
esac
exit "$fail"
