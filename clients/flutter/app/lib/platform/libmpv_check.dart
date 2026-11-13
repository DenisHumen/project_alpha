// SPDX-License-Identifier: MPL-2.0

import 'dart:ffi';
import 'dart:io';

/// On Linux the player uses the system libmpv (LGPL). Returns true when it
/// cannot be loaded (the app then shows `appLibmpvMissing`).
bool libmpvMissing() {
  if (!Platform.isLinux) return false;
  for (final name in ['libmpv.so.2', 'libmpv.so.1', 'libmpv.so']) {
    try {
      DynamicLibrary.open(name);
      return false;
    } on ArgumentError {
      // try the next soname
    }
  }
  return true;
}
