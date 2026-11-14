// SPDX-License-Identifier: MPL-2.0

/// Build flavor (ADR-0007, CORE-013).
///
/// `store`: App Store, Google Play, Microsoft Store — no native Twitch
/// playback, no Twitch downloads, no external donation links. `full`:
/// GitHub Releases, F-Droid, Flathub and the rest — everything.
enum BuildFlavor { store, full }

/// The flavor of this build, a compile-time constant:
/// `--dart-define=KINOLUNE_FLAVOR=store|full` (default `store`).
///
/// Full-only Dart code goes behind `if (AppFlavor.donationLinks)` and the
/// like: the AOT compiler drops it from store builds, which the artifact
/// check (tools/flavor/check-artifacts.sh) verifies on `libapp.so`. The
/// Rust core is flavored separately (CARGO_KINOLUNE_FLAVOR, pa_core build
/// hook); [checkCoreFlavor] compares the two on startup.
abstract final class AppFlavor {
  /// Raw `KINOLUNE_FLAVOR` value.
  static const name = String.fromEnvironment(
    'KINOLUNE_FLAVOR',
    defaultValue: 'store',
  );

  /// Whether `KINOLUNE_FLAVOR` names a known flavor.
  static const isValid = name == 'store' || name == 'full';

  /// The flavor; an unknown name counts as `store`.
  static const current = name == 'full' ? BuildFlavor.full : BuildFlavor.store;

  static const isFull = current == BuildFlavor.full;

  /// Full-only capabilities, the same ids as `pa-flavor` in the core.
  static const twitchNative = isFull;
  static const twitchArchive = isFull;
  static const donationLinks = isFull;

  /// Capability ids compiled into this Dart build, as the core reports its
  /// own in `buildCapabilities()`.
  static const capabilities = <String>[
    if (twitchNative) 'twitch-native',
    if (twitchArchive) 'twitch-archive',
    if (donationLinks) 'donation-links',
  ];
}

/// Compares the Dart flavor with the core's (`buildFlavor()` from pa_core).
/// Returns `null` when they agree, or what is wrong: a mismatch is a
/// packaging error (one of `--dart-define=KINOLUNE_FLAVOR` and
/// `CARGO_KINOLUNE_FLAVOR` was not set).
String? checkCoreFlavor(String coreFlavor) {
  if (!AppFlavor.isValid) {
    return 'KINOLUNE_FLAVOR="${AppFlavor.name}" is not "store" or "full"';
  }
  if (coreFlavor != AppFlavor.name) {
    return 'app flavor "${AppFlavor.name}" but core flavor "$coreFlavor": '
        'build with the same --dart-define=KINOLUNE_FLAVOR and '
        'CARGO_KINOLUNE_FLAVOR';
  }
  return null;
}
