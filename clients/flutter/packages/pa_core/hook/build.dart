// SPDX-License-Identifier: MPL-2.0
// Native Assets build hook: Dart and Flutter run it on `dart test`,
// `flutter test`, `flutter build` and `flutter run`. It builds the Rust crate
// core/crates/pa-frb for the target platform (rustup toolchain from the
// crate's rust-toolchain.toml) and bundles libpa_frb with the app.
import 'package:flutter_rust_bridge_hooks/flutter_rust_bridge_hooks.dart';

void main(List<String> args) async {
  await build(args, (input, output) async {
    await const FlutterRustBridgeNativeAssetsBuilder(
      cratePath: '../../../../core/crates/pa-frb',
    ).run(input: input, output: output);
  });
}
