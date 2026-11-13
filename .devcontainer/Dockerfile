# SPDX-License-Identifier: MPL-2.0
#
# Dev Container for the Linux part of the monorepo (REPO-008): Rust core and
# server, Android (SDK/NDK, cargo-ndk), Flutter Linux desktop, Go (distro),
# podman. Apple targets build only on macOS.
#
# Versions come from rust-toolchain.toml, mise.toml and
# tools/bootstrap/versions.env, so the container and `just bootstrap` agree.
# Build context is the repository root (see devcontainer.json).

FROM mcr.microsoft.com/devcontainers/base:ubuntu-24.04@sha256:d94c97dd9cacf183d0a6fd12a8e87b526e9e928307674ae9c94139139c0c6eae

# Set to true only if you have read and accept the Android SDK licences; then
# the SDK packages are baked into the image instead of installed on first start.
ARG ACCEPT_ANDROID_SDK_LICENSES=false

ARG DEBIAN_FRONTEND=noninteractive
RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        build-essential clang cmake ninja-build pkg-config \
        curl ca-certificates git git-lfs unzip xz-utils zip file \
        python3 \
        libgtk-3-dev liblzma-dev libstdc++-12-dev libmpv-dev \
        podman fuse-overlayfs uidmap \
    && rm -rf /var/lib/apt/lists/*

USER vscode
ENV HOME=/home/vscode \
    ANDROID_HOME=/home/vscode/android-sdk \
    CARGO_BUILD_JOBS=4 \
    MISE_YES=1
ENV PATH=/home/vscode/.local/share/mise/shims:/home/vscode/.local/bin:/home/vscode/.cargo/bin:/home/vscode/android-sdk/cmdline-tools/latest/bin:/home/vscode/android-sdk/platform-tools:$PATH

COPY --chown=vscode:vscode tools/bootstrap/versions.env rust-toolchain.toml mise.toml /tmp/pa/

# Rust: rustup without a default toolchain, then the pinned one.
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
        | sh -s -- -y --no-modify-path --profile minimal --default-toolchain none \
    && cd /tmp/pa \
    && rustup toolchain install "$(sed -n 's/^channel *= *"\(.*\)"/\1/p' rust-toolchain.toml)" \
        --profile minimal --component clippy,rustfmt

# mise (checksum-pinned) and every tool from mise.toml.
RUN . /tmp/pa/versions.env \
    && case "$(uname -m)" in \
        x86_64) arch=x64; sum="$MISE_SHA256_LINUX_X64" ;; \
        aarch64) arch=arm64; sum="$MISE_SHA256_LINUX_ARM64" ;; \
       esac \
    && curl -fsSL "https://github.com/jdx/mise/releases/download/v${MISE_VERSION}/mise-v${MISE_VERSION}-linux-${arch}.tar.gz" -o /tmp/mise.tar.gz \
    && echo "$sum  /tmp/mise.tar.gz" | sha256sum -c - \
    && tar -xzf /tmp/mise.tar.gz -C /tmp \
    && install -D -m 0755 /tmp/mise/bin/mise "$HOME/.local/bin/mise" \
    && rm -rf /tmp/mise /tmp/mise.tar.gz \
    && cd /tmp/pa && mise trust --quiet mise.toml && mise install \
    && echo 'eval "$(mise activate bash)"' >> "$HOME/.bashrc"

# Android command-line tools (checksum-pinned). SDK packages need accepted
# licences: see ACCEPT_ANDROID_SDK_LICENSES above or run `just bootstrap`.
RUN . /tmp/pa/versions.env \
    && curl -fsSL "https://dl.google.com/android/repository/commandlinetools-linux-${ANDROID_CMDLINE_TOOLS_BUILD}_latest.zip" -o /tmp/clt.zip \
    && echo "$ANDROID_CMDLINE_TOOLS_SHA256_LINUX  /tmp/clt.zip" | sha256sum -c - \
    && unzip -q /tmp/clt.zip -d /tmp \
    && mkdir -p "$ANDROID_HOME/cmdline-tools" \
    && mv /tmp/cmdline-tools "$ANDROID_HOME/cmdline-tools/latest" \
    && rm /tmp/clt.zip \
    && if [ "$ACCEPT_ANDROID_SDK_LICENSES" = true ]; then \
        yes | sdkmanager --licenses >/dev/null \
        && sdkmanager --install platform-tools "platforms;$ANDROID_PLATFORM" \
            "build-tools;$ANDROID_BUILD_TOOLS" "ndk;$ANDROID_NDK" >/dev/null; \
       fi

RUN flutter config --no-analytics >/dev/null 2>&1 || true
