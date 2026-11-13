#!/usr/bin/env bash
# SPDX-License-Identifier: MPL-2.0
#
# Developer machine bootstrap for Linux and macOS (REPO-008).
#
#   tools/bootstrap/bootstrap.sh            install user-level toolchains, then check
#   tools/bootstrap/bootstrap.sh --check    only check, change nothing
#   tools/bootstrap/bootstrap.sh --skip android,flutter
#
# Never runs sudo. Missing OS packages are reported together with the command
# that installs them; run it yourself and re-run the bootstrap.
# Bash 3.2 compatible (macOS system bash).

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
# shellcheck source=versions.env
. "$ROOT/tools/bootstrap/versions.env"

MODE=install
SKIP=","
while [ $# -gt 0 ]; do
    case "$1" in
        --check) MODE=check ;;
        --skip) SKIP=",$2,"; shift ;;
        --skip=*) SKIP=",${1#--skip=},";;
        -h|--help) sed -n '4,12p' "$0"; exit 0 ;;
        *) echo "unknown argument: $1" >&2; exit 2 ;;
    esac
    shift
done

skipped() { case "$SKIP" in *",$1,"*) return 0 ;; *) return 1 ;; esac; }
have() { command -v "$1" >/dev/null 2>&1; }

if [ -t 1 ]; then G=$'\e[32m'; Y=$'\e[33m'; R=$'\e[31m'; B=$'\e[1m'; N=$'\e[0m'; else G= Y= R= B= N=; fi
FAILED=0
ACTIONS=""
ok()   { printf '  %sok%s    %s\n' "$G" "$N" "$1"; }
warn() { printf '  %swarn%s  %s\n' "$Y" "$N" "$1"; }
fail() { printf '  %sfail%s  %s\n' "$R" "$N" "$1"; FAILED=1; }
todo() { ACTIONS="${ACTIONS}  - $1"$'\n'; }
step() { printf '\n%s%s%s\n' "$B" "$1" "$N"; }

# version_ge 3.12.1 3.10 -> true
version_ge() {
    [ "$(printf '%s\n%s\n' "$2" "$1" | sort -t. -k1,1n -k2,2n -k3,3n | head -n1)" = "$2" ]
}

OS="$(uname -s)"
ARCH="$(uname -m)"
PKG=""
if [ "$OS" = Darwin ]; then
    PKG=brew
elif [ -r /etc/os-release ]; then
    # shellcheck disable=SC1091
    . /etc/os-release
    case " ${ID:-} ${ID_LIKE:-} " in
        *" debian "*|*" ubuntu "*) PKG=apt ;;
        *" fedora "*|*" rhel "*) PKG=dnf ;;
        *" arch "*) PKG=pacman ;;
        *" opensuse"*|*" suse "*) PKG=zypper ;;
    esac
fi

# --- 1. OS packages ---------------------------------------------------------
# Each entry: <check>|<apt>|<dnf>|<pacman>|<brew>. A check is a command name or
# pc:<pkg-config module>. Empty package name = not needed on that system.
REQUIRED="
git|git|git|git|git
git-lfs|git-lfs|git-lfs|git-lfs|git-lfs
curl|curl|curl|curl|curl
unzip|unzip|unzip|unzip|
xz|xz-utils|xz|xz|xz
python3|python3|python3|python|python
cc|build-essential|gcc|base-devel|
pkg-config|pkg-config|pkgconf-pkg-config|pkgconf|pkgconf
cmake|cmake|cmake|cmake|cmake
ninja|ninja-build|ninja-build|ninja|ninja
"
# Flutter Linux desktop (clients/flutter, PLAY-DSK): GTK 3 and clang.
LINUX_DESKTOP="
clang|clang|clang|clang|
pc:gtk+-3.0|libgtk-3-dev|gtk3-devel|gtk3|
pc:mpv|libmpv-dev|mpv-devel|mpv|
"
# Only for the NAS distro (distro/, DIST-*).
OPTIONAL="
podman|podman|podman|podman|podman
"

MISSING_PKGS=""
check_pkgs() { # $1 = list, $2 = fail|warn
    local line check pkg idx
    case "$PKG" in apt) idx=2 ;; dnf) idx=3 ;; pacman) idx=4 ;; brew) idx=5 ;; *) idx=2 ;; esac
    while IFS= read -r line; do
        [ -n "$line" ] || continue
        check="${line%%|*}"
        pkg="$(printf '%s' "$line" | cut -d'|' -f"$idx")"
        if [ "${check#pc:}" != "$check" ]; then
            if have pkg-config && pkg-config --exists "${check#pc:}"; then ok "$check"; continue; fi
        elif have "$check"; then
            ok "$check"; continue
        fi
        [ -n "$pkg" ] || { ok "$check (not needed here)"; continue; }
        if [ "$2" = fail ]; then fail "$check missing"; else warn "$check missing (optional)"; fi
        case " $MISSING_PKGS " in *" $pkg "*) ;; *) MISSING_PKGS="$MISSING_PKGS $pkg" ;; esac
    done <<EOF
$1
EOF
}

step "OS packages (${PKG:-unknown package manager})"
check_pkgs "$REQUIRED" fail
if [ "$OS" = Linux ] && ! skipped flutter; then check_pkgs "$LINUX_DESKTOP" fail; fi
check_pkgs "$OPTIONAL" warn
if have python3; then
    PYV="$(python3 -c 'import sys; print("%d.%d" % sys.version_info[:2])')"
    if version_ge "$PYV" "$MIN_PYTHON"; then ok "python $PYV"; else fail "python $PYV < $MIN_PYTHON"; fi
fi
if have git; then
    GITV="$(git --version | awk '{print $3}')"
    if version_ge "$GITV" "$MIN_GIT"; then ok "git $GITV"; else fail "git $GITV < $MIN_GIT"; fi
fi
if [ -n "$MISSING_PKGS" ]; then
    case "$PKG" in
        apt) todo "sudo apt-get install -y$MISSING_PKGS" ;;
        dnf) todo "sudo dnf install -y$MISSING_PKGS" ;;
        pacman) todo "sudo pacman -S --needed$MISSING_PKGS" ;;
        brew) todo "brew install$MISSING_PKGS" ;;
        *) todo "install with your package manager:$MISSING_PKGS" ;;
    esac
fi
if [ "$OS" = Darwin ] && ! xcode-select -p >/dev/null 2>&1; then
    fail "Xcode command line tools missing"
    todo "xcode-select --install"
fi

# --- 2. Rust (rustup + rust-toolchain.toml) ---------------------------------
step "Rust"
[ -d "$HOME/.cargo/bin" ] && PATH="$HOME/.cargo/bin:$PATH"
RUST_CHANNEL="$(sed -n 's/^channel *= *"\(.*\)"/\1/p' "$ROOT/rust-toolchain.toml")"
if ! have rustup; then
    fail "rustup missing"
    todo "curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain none"
else
    if [ "$MODE" = install ]; then
        (cd "$ROOT" && rustup toolchain install "$RUST_CHANNEL" --profile minimal \
            --component clippy,rustfmt >/dev/null)
    fi
    if (cd "$ROOT" && rustc --version 2>/dev/null | grep -q " $RUST_CHANNEL "); then
        ok "rustc $RUST_CHANNEL (rust-toolchain.toml)"
    else
        fail "rust $RUST_CHANNEL not installed"
        todo "just bootstrap   # installs the toolchain from rust-toolchain.toml"
    fi
fi

# --- 3. mise-managed tools (mise.toml) --------------------------------------
step "mise.toml (Flutter, JDK, Go, Node, just, cargo-ndk, nextest)"
[ -d "$HOME/.local/bin" ] && PATH="$HOME/.local/bin:$PATH"
install_mise() {
    local arch sum url tmp
    case "$ARCH" in
        x86_64|amd64) arch=x64; sum="$MISE_SHA256_LINUX_X64" ;;
        aarch64|arm64) arch=arm64; sum="$MISE_SHA256_LINUX_ARM64" ;;
        *) return 1 ;;
    esac
    url="https://github.com/jdx/mise/releases/download/v${MISE_VERSION}/mise-v${MISE_VERSION}-linux-${arch}.tar.gz"
    tmp="$(mktemp -d)"
    curl -fsSL "$url" -o "$tmp/mise.tar.gz"
    echo "$sum  $tmp/mise.tar.gz" | sha256sum -c - >/dev/null
    tar -xzf "$tmp/mise.tar.gz" -C "$tmp"
    mkdir -p "$HOME/.local/bin"
    install -m 0755 "$tmp/mise/bin/mise" "$HOME/.local/bin/mise"
    rm -rf "$tmp"
}
if ! have mise && [ "$MODE" = install ] && [ "$OS" = Linux ] && have curl && have sha256sum; then
    echo "  installing mise $MISE_VERSION to ~/.local/bin"
    install_mise || warn "could not install mise automatically"
fi
if ! have mise; then
    fail "mise missing"
    if [ "$OS" = Darwin ]; then todo "brew install mise"; else
        todo "curl https://mise.run | MISE_VERSION=v$MISE_VERSION sh"; fi
    todo "activate mise in your shell: https://mise.jdx.dev/getting-started.html"
else
    ok "mise $(mise --version 2>/dev/null | awk '{print $1}')"
    if [ "$MODE" = install ]; then
        mise trust --quiet "$ROOT/mise.toml"
        if skipped flutter; then
            (cd "$ROOT" && mise install java go node just cargo:cargo-ndk cargo:cargo-nextest)
        else
            (cd "$ROOT" && mise install)
        fi
    fi
    MISSING_TOOLS="$(cd "$ROOT" && mise ls --current --missing --no-header 2>/dev/null | awk '{print $1}' | tr '\n' ' ')"
    for t in $MISSING_TOOLS; do
        if [ "$t" = flutter ] && skipped flutter; then continue; fi
        fail "$t (mise.toml) not installed"
    done
    [ -z "$MISSING_TOOLS" ] && ok "all mise.toml tools installed"
    [ -n "$MISSING_TOOLS" ] && todo "cd $(basename "$ROOT") && mise install"
fi

# --- 4. Android SDK ---------------------------------------------------------
if ! skipped android; then
    step "Android SDK ($ANDROID_PLATFORM, build-tools $ANDROID_BUILD_TOOLS, NDK $ANDROID_NDK)"
    if [ "$OS" = Darwin ]; then DEFAULT_SDK="$HOME/Library/Android/sdk"; else DEFAULT_SDK="$HOME/Android/Sdk"; fi
    SDK="${ANDROID_HOME:-${ANDROID_SDK_ROOT:-$DEFAULT_SDK}}"
    SDKMANAGER="$SDK/cmdline-tools/latest/bin/sdkmanager"
    if [ ! -x "$SDKMANAGER" ] && [ "$MODE" = install ] && [ "$OS" = Linux ] && have unzip; then
        echo "  installing Android cmdline-tools to $SDK"
        tmp="$(mktemp -d)"
        curl -fsSL "https://dl.google.com/android/repository/commandlinetools-linux-${ANDROID_CMDLINE_TOOLS_BUILD}_latest.zip" -o "$tmp/clt.zip"
        echo "$ANDROID_CMDLINE_TOOLS_SHA256_LINUX  $tmp/clt.zip" | sha256sum -c - >/dev/null
        unzip -q "$tmp/clt.zip" -d "$tmp"
        mkdir -p "$SDK/cmdline-tools"
        rm -rf "$SDK/cmdline-tools/latest"
        mv "$tmp/cmdline-tools" "$SDK/cmdline-tools/latest"
        rm -rf "$tmp"
    fi
    PKGS="platform-tools platforms;$ANDROID_PLATFORM build-tools;$ANDROID_BUILD_TOOLS ndk;$ANDROID_NDK"
    if [ ! -x "$SDKMANAGER" ]; then
        fail "Android cmdline-tools missing in $SDK"
        todo "install Android Studio or cmdline-tools, set ANDROID_HOME (now: $SDK)"
    elif [ ! -f "$SDK/licenses/android-sdk-license" ]; then
        fail "Android SDK licences not accepted"
        todo "read and accept the licences yourself: $SDKMANAGER --licenses"
    else
        if [ "$MODE" = install ]; then
            # shellcheck disable=SC2086
            "$SDKMANAGER" --sdk_root="$SDK" --install $PKGS >/dev/null
        fi
        for p in $PKGS; do
            if [ -d "$SDK/$(printf '%s' "$p" | tr ';' '/')" ]; then ok "$p"; else
                fail "$p missing"; todo "$SDKMANAGER --sdk_root=$SDK --install '$p'"; fi
        done
    fi
    [ -n "${ANDROID_HOME:-}" ] || warn "ANDROID_HOME is not set (expected $SDK)"
fi

# --- 5. Xcode (macOS only) --------------------------------------------------
if [ "$OS" = Darwin ]; then
    step "Xcode"
    WANT="$(tr -d '[:space:]' < "$ROOT/.xcode-version")"
    if have xcodebuild && HAVE_X="$(xcodebuild -version 2>/dev/null | awk 'NR==1{print $2}')"; then
        case "$HAVE_X" in
            "$WANT"*) ok "Xcode $HAVE_X" ;;
            *) warn "Xcode $HAVE_X, project pins $WANT (.xcode-version); use xcodes or xcode-select" ;;
        esac
    else
        warn "Xcode not installed (needed only for Apple targets; pinned $WANT)"
    fi
fi

# --- Summary ----------------------------------------------------------------
echo
if [ "$FAILED" -eq 0 ]; then
    printf '%sbootstrap: environment is ready%s\n' "$G" "$N"
    exit 0
fi
printf '%sbootstrap: some checks failed.%s Do this, then re-run `just bootstrap`:\n' "$R" "$N"
printf '%s' "$ACTIONS"
exit 1
