// SPDX-License-Identifier: MPL-2.0
// Native Assets build hook: Dart and Flutter run it on `dart test`,
// `flutter test`, `flutter build` and `flutter run`. It builds the Rust crate
// core/crates/pa-frb for the target platform (rustup toolchain from the
// crate's rust-toolchain.toml) and bundles libpa_frb with the app.
//
// Build flavor (ADR-0007, CORE-013): the cargo feature `store` or `full` of
// pa-frb comes from the environment variable CARGO_KINOLUNE_FLAVOR (default
// `store`). The CARGO_ prefix is what lets it through the hook runner's
// environment filter, and the runner re-runs the hook when it changes. Set
// it together with `--dart-define=KINOLUNE_FLAVOR`; docs/dev/flavors.md.
import 'dart:io';

import 'package:flutter_rust_bridge_hooks/flutter_rust_bridge_hooks.dart';

const _flavorVariable = 'CARGO_KINOLUNE_FLAVOR';

void main(List<String> args) async {
  await build(args, (input, output) async {
    final flavor = Platform.environment[_flavorVariable] ?? 'store';
    if (flavor != 'store' && flavor != 'full') {
      throw ArgumentError.value(
        flavor,
        _flavorVariable,
        'must be "store" or "full"',
      );
    }
    await FlutterRustBridgeNativeAssetsBuilder(
      cratePath: '../../../../core/crates/pa-frb',
      features: [flavor],
    ).run(input: input, output: output);
  });
}
