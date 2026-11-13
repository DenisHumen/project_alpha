// SPDX-License-Identifier: MPL-2.0

import 'dart:ffi';
import 'dart:io';

/// On Linux the player uses the system libmpv (LGPL). Returns a user-facing
/// hint when it cannot be loaded, null when it is fine or not applicable.
String? checkLibmpv() {
  if (!Platform.isLinux) return null;
  for (final name in ['libmpv.so.2', 'libmpv.so.1', 'libmpv.so']) {
    try {
      DynamicLibrary.open(name);
      return null;
    } on ArgumentError {
      // try the next soname
    }
  }
  return 'Video playback needs the mpv library, which was not found. '
      'Install it with your package manager: '
      '“sudo apt install libmpv2” (Debian, Ubuntu), '
      '“sudo dnf install mpv-libs” (Fedora) or '
      '“sudo pacman -S mpv” (Arch), then restart Kinolune.';
}
