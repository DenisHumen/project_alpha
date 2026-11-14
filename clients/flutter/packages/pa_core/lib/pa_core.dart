// SPDX-License-Identifier: MPL-2.0
/// Dart bindings to the project_alpha Rust core (ADR-0001).
///
/// Call `await RustLib.init()` once before any other function, for example at
/// the top of `main()`. The native library `libpa_frb` is built from
/// `core/crates/pa-frb` by the package's Native Assets hook and bundled with
/// the app automatically.
library;

export 'src/rust/api/events.dart';
export 'src/rust/api/images.dart';
export 'src/rust/api/library.dart';
export 'src/rust/api/recognition.dart';
export 'src/rust/api/sources.dart';
export 'src/rust/api/version.dart';
export 'src/rust/frb_generated.dart' show RustLib;
