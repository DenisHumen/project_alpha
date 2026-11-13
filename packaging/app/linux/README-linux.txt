SPDX-License-Identifier: MPL-2.0

Kinolune for Linux (x86_64)
===========================

Requirements: glibc 2.39 or newer (Ubuntu 24.04+), GTK 3, PulseAudio or
PipeWire, libva and EGL. libmpv and FFmpeg (our LGPL build) are included in lib/.
  Ubuntu/Debian:  sudo apt install libgtk-3-0 libva2 libva-drm2 libegl1 libpulse0
  Fedora:         sudo dnf install gtk3 libva mesa-libEGL pulseaudio-libs
  Arch:           sudo pacman -S gtk3 libva libglvnd libpulse

Run the executable in this directory (for example ./kinolune).

Licences: LICENSE (MPL-2.0) covers Kinolune itself; THIRD-PARTY-NOTICES.md and
licenses/ cover third-party code (licenses/FLUTTER-NOTICES.txt lists the
Flutter engine and every Dart package). licenses/libmpv/ covers the bundled
libmpv and FFmpeg (LGPL-2.1-or-later); their corresponding source is attached
to the same GitHub Release (kinolune-<version>-libmpv-sources.tar.xz).

Source code: https://github.com/DenisHumen/project_alpha
