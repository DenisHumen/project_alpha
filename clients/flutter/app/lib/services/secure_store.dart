// SPDX-License-Identifier: MPL-2.0

import 'package:flutter/foundation.dart';
import 'package:flutter_secure_storage/flutter_secure_storage.dart';

/// Secrets (SMB passwords, TMDB key) in the platform keystore: Android
/// Keystore-backed storage, libsecret on Linux. If the keystore is not
/// available (e.g. no Secret Service on a headless desktop) values are kept in
/// memory for the session only.
abstract class SecureStore {
  Future<String?> read(String key);
  Future<void> write(String key, String? value);

  static const tmdbKey = 'tmdb_api_key';
  static String smbPassword(String sourceId) => 'smb_password_$sourceId';
}

class PlatformSecureStore implements SecureStore {
  PlatformSecureStore();

  final _storage = const FlutterSecureStorage();
  final _memory = <String, String>{};
  bool _broken = false;

  @override
  Future<String?> read(String key) async {
    if (!_broken) {
      try {
        return await _storage.read(key: key);
      } catch (e) {
        _broken = true;
        debugPrint('SecureStore unavailable, using memory: $e');
      }
    }
    return _memory[key];
  }

  @override
  Future<void> write(String key, String? value) async {
    if (!_broken) {
      try {
        if (value == null) {
          await _storage.delete(key: key);
        } else {
          await _storage.write(key: key, value: value);
        }
        return;
      } catch (e) {
        _broken = true;
        debugPrint('SecureStore unavailable, using memory: $e');
      }
    }
    if (value == null) {
      _memory.remove(key);
    } else {
      _memory[key] = value;
    }
  }
}

class MemorySecureStore implements SecureStore {
  final values = <String, String>{};

  @override
  Future<String?> read(String key) async => values[key];

  @override
  Future<void> write(String key, String? value) async {
    if (value == null) {
      values.remove(key);
    } else {
      values[key] = value;
    }
  }
}
