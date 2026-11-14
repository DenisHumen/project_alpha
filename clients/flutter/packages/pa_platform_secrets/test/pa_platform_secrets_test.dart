// SPDX-License-Identifier: MPL-2.0

import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:pa_core/pa_core.dart';
import 'package:pa_platform_secrets/pa_platform_secrets.dart';

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();
  final messenger =
      TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger;
  final stored = <String, String>{};
  var failWith = '';

  setUp(() {
    stored.clear();
    failWith = '';
    messenger.setMockMethodCallHandler(platformSecretsChannel, (call) async {
      if (failWith.isNotEmpty) {
        throw PlatformException(code: failWith, message: 'Keystore said no');
      }
      final key = call.arguments['key'] as String;
      switch (call.method) {
        case 'load':
          return stored[key];
        case 'save':
          stored[key] = call.arguments['value'] as String;
          return null;
        case 'remove':
          return stored.remove(key) != null;
      }
      throw MissingPluginException();
    });
  });

  tearDown(
    () => messenger.setMockMethodCallHandler(platformSecretsChannel, null),
  );

  Future<PlatformSecretReply> call(String op, String key, [String? value]) =>
      handlePlatformSecretCall(
        PlatformSecretCall(op: op, key: key, value: value),
      );

  test('load, save, overwrite and remove go over the channel', () async {
    expect((await call('load', 'k')).value, isNull);
    expect((await call('save', 'k', 'v1')).errorCode, isNull);
    expect(stored['k'], 'v1');
    expect((await call('load', 'k')).value, 'v1');
    await call('save', 'k', 'v2');
    expect((await call('load', 'k')).value, 'v2');
    final removed = await call('remove', 'k');
    expect(removed.existed, isTrue);
    expect(removed.errorCode, isNull);
    expect((await call('remove', 'k')).existed, isFalse);
  });

  test('platform errors become reply codes, never exceptions', () async {
    failWith = 'locked';
    final locked = await call('load', 'k');
    expect(locked.errorCode, 'locked');
    expect(locked.errorMessage, contains('Keystore said no'));
    failWith = 'weird';
    expect((await call('save', 'k', 'v')).errorCode, 'failed');
    failWith = '';
    expect((await call('nope', 'k')).errorCode, 'failed');
  });

  test('a missing plugin means no platform store', () async {
    messenger.setMockMethodCallHandler(platformSecretsChannel, null);
    expect((await call('load', 'k')).errorCode, 'unavailable');
  });

  test('only Android has a store in this package', () {
    debugDefaultTargetPlatformOverride = TargetPlatform.linux;
    expect(platformSecretsSupported, isFalse);
    debugDefaultTargetPlatformOverride = TargetPlatform.android;
    expect(platformSecretsSupported, isTrue);
    debugDefaultTargetPlatformOverride = null;
  });
}
