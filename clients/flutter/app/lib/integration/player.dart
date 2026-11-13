// SPDX-License-Identifier: MPL-2.0
//
// Single switch point between the in-app fake and the real `pa_player`.
// When R01-PLAYER is on main: add `pa_player: {path: ../packages/pa_player}`
// to pubspec.yaml and replace the export below with
//   export 'package:pa_player/pa_player.dart';

export '../fakes/fake_pa_player.dart';
