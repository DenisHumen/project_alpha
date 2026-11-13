// SPDX-License-Identifier: MPL-2.0
//
// Single switch point between the in-app fake and the real `pa_library`.
// When R01-LIB is on main: add `pa_library: {path: ../packages/pa_library}`
// to pubspec.yaml and replace the export below with
//   export 'package:pa_library/pa_library.dart';
// The demo mode (lib/fakes/fake_library.dart) keeps working because it
// implements the same interface.

export '../fakes/pa_library_contract.dart';
