# SPDX-License-Identifier: MPL-2.0
# Reproducible build + headless test environment for the PLAY-DSK-001 spike.
# Everything runs inside the container: no sudo / system packages on the host.
FROM ubuntu:26.04

ARG FLUTTER_VERSION=3.50.0
ARG FLUTTER_SHA256=6ea2d806fd57269ae783e6788e2a81fc26eaad782f1e0d0ff38623bb765ff9f5

ENV DEBIAN_FRONTEND=noninteractive
RUN apt-get update && apt-get install -y --no-install-recommends \
      ca-certificates curl git unzip xz-utils file \
      clang cmake ninja-build pkg-config libgtk-3-dev liblzma-dev libstdc++-14-dev \
      libmpv-dev libegl-dev libgl-dev libx11-dev libwayland-dev wayland-protocols \
      libgl1-mesa-dri mesa-vulkan-drivers libvulkan1 \
      sway grim xwayland xvfb x11-utils x11-apps imagemagick ffmpeg procps jq \
      fonts-dejavu-core dbus xdotool wtype vainfo intel-media-va-driver mesa-va-drivers \
    && rm -rf /var/lib/apt/lists/*

# Build user with the same uid as a typical desktop user so bind mounts stay writable.
RUN (getent passwd 1000 >/dev/null && usermod -l dev -d /home/dev -m "$(getent passwd 1000 | cut -d: -f1)") \
      || useradd -m -u 1000 -s /bin/bash dev

RUN curl -fsSL -o /tmp/flutter.tar.xz \
      "https://storage.googleapis.com/flutter_infra_release/releases/stable/linux/flutter_linux_${FLUTTER_VERSION}-stable.tar.xz" \
    && echo "${FLUTTER_SHA256}  /tmp/flutter.tar.xz" | sha256sum -c - \
    && mkdir -p /opt && tar -xJf /tmp/flutter.tar.xz -C /opt && rm /tmp/flutter.tar.xz \
    && chown -R 1000:1000 /opt/flutter

USER 1000
ENV HOME=/home/dev PATH=/opt/flutter/bin:$PATH
RUN git config --global --add safe.directory /opt/flutter \
    && flutter config --no-analytics --enable-linux-desktop >/dev/null \
    && flutter precache --linux && flutter --version
WORKDIR /src
