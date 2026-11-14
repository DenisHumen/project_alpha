// SPDX-License-Identifier: MPL-2.0
// CORE-013: build flavor constant. Runs in both flavors:
//   flutter test test/flavor_test.dart
//   flutter test test/flavor_test.dart --dart-define=KINOLUNE_FLAVOR=full

import 'package:flutter_test/flutter_test.dart';
import 'package:kinolune/flavor.dart';

void main() {
  test('flavor constant matches KINOLUNE_FLAVOR (default store)', () {
    const raw = String.fromEnvironment('KINOLUNE_FLAVOR');
    expect(AppFlavor.isValid, isTrue);
    expect(AppFlavor.current.name, raw.isEmpty ? 'store' : raw);
  });

  test('store has no full-only capability, full has all', () {
    if (AppFlavor.current == BuildFlavor.store) {
      expect(AppFlavor.capabilities, isEmpty);
      expect(AppFlavor.donationLinks, isFalse);
      expect(AppFlavor.twitchNative, isFalse);
      expect(AppFlavor.twitchArchive, isFalse);
    } else {
      expect(AppFlavor.capabilities, [
        'twitch-native',
        'twitch-archive',
        'donation-links',
      ]);
    }
  });

  test('core flavor check', () {
    expect(checkCoreFlavor(AppFlavor.name), isNull);
    final other = AppFlavor.isFull ? 'store' : 'full';
    expect(checkCoreFlavor(other), contains('core flavor "$other"'));
  });
}
