// SPDX-License-Identifier: MPL-2.0

import 'platform/form_factor.dart';

/// Compile-time switches (`--dart-define=NAME=value`).
abstract final class AppConfig {
  /// Start with the built-in demo library (fictional titles, art drawn in
  /// code). Useful for screenshots and for trying the UI without media.
  static const demo = bool.fromEnvironment('KINOLUNE_DEMO');

  /// Run the library on the Rust core (`pa_core`, UI-024; the default). With
  /// `false` the app uses the Dart v0 implementation, which is also the
  /// fallback when the native core is missing (`openAppLibrary`).
  static const rustCore = bool.fromEnvironment(
    'KINOLUNE_RUST_CORE',
    defaultValue: true,
  );

  /// `tv`, `phone` or `desktop`; empty = auto-detect.
  static const _formFactor = String.fromEnvironment('KINOLUNE_FORM_FACTOR');

  static FormFactor? get forcedFormFactor => switch (_formFactor) {
    'tv' => FormFactor.tv,
    'phone' => FormFactor.phone,
    'desktop' => FormFactor.desktop,
    _ => null,
  };
}
