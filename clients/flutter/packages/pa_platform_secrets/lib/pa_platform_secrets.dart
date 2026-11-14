// SPDX-License-Identifier: MPL-2.0

/// The platform secret store of the Rust core on mobile (CORE-015).
///
/// On Android the core keeps passwords and tokens in an Android Keystore
/// AES-256 key with AES-GCM over app-private storage
/// (`clients/android/secrets`, `KeystoreSecretStore`). The core is Rust and
/// the store is Kotlin, so the core calls a Dart callback
/// ([handlePlatformSecretCall]) that forwards each call over a platform
/// channel. Open it once at app start, after `RustLib.init()`:
///
/// ```dart
/// final info = await openPlatformSecrets(dataDir: dir.path);
/// ```
///
/// Desktop does not need this: the core uses the OS keychain there.
/// iOS will use the Keychain the same way once the app has an iOS runner;
/// the Swift store already exists in `PACore` (`KeychainSecretStore`).
library;

import 'package:flutter/foundation.dart';
import 'package:flutter/services.dart';
import 'package:pa_core/pa_core.dart';

/// Channel of the Kotlin side (`PaPlatformSecretsPlugin`).
const platformSecretsChannel = MethodChannel('pa_platform_secrets');

/// Whether this platform has a store in this package.
bool get platformSecretsSupported =>
    !kIsWeb && defaultTargetPlatform == TargetPlatform.android;

/// Opens the platform store as the secret store of the core
/// (`secretsOpenPlatform`). Returns null where the platform has none
/// ([platformSecretsSupported] is false): the core then uses the OS keychain.
///
/// [dataDir] is the core data directory: secrets in an old encrypted
/// fallback file there are moved into the platform store.
Future<CoreSecretStoreInfo?> openPlatformSecrets({
  String? dataDir,
  MethodChannel channel = platformSecretsChannel,
}) async {
  if (!platformSecretsSupported) return null;
  return secretsOpenPlatform(
    dataDir: dataDir,
    callback: (call) => handlePlatformSecretCall(call, channel: channel),
  );
}

/// Answers one call of the core over [channel]. Never throws: errors go to
/// the core as [PlatformSecretReply.errorCode], since an exception would not
/// reach the waiting Rust thread.
Future<PlatformSecretReply> handlePlatformSecretCall(
  PlatformSecretCall call, {
  MethodChannel channel = platformSecretsChannel,
}) async {
  try {
    switch (call.op) {
      case 'load':
        final value = await channel.invokeMethod<String>('load', {
          'key': call.key,
        });
        return _ok(value: value);
      case 'save':
        await channel.invokeMethod<void>('save', {
          'key': call.key,
          'value': call.value ?? '',
        });
        return _ok();
      case 'remove':
        final existed = await channel.invokeMethod<bool>('remove', {
          'key': call.key,
        });
        return _ok(existed: existed ?? false);
      default:
        return _error('failed', 'unknown operation ${call.op}');
    }
  } on PlatformException catch (e) {
    final code = switch (e.code) {
      'unavailable' || 'locked' => e.code,
      _ => 'failed',
    };
    return _error(code, '${e.code}: ${e.message}');
  } on MissingPluginException catch (e) {
    return _error('unavailable', 'no platform secret store: ${e.message}');
  } catch (e) {
    return _error('failed', '$e');
  }
}

PlatformSecretReply _ok({String? value, bool existed = false}) =>
    PlatformSecretReply(
      value: value,
      existed: existed,
      errorCode: null,
      errorMessage: null,
    );

PlatformSecretReply _error(String code, String message) => PlatformSecretReply(
  value: null,
  existed: false,
  errorCode: code,
  errorMessage: message,
);
