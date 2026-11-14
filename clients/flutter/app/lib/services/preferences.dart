// SPDX-License-Identifier: MPL-2.0

import 'dart:convert';
import 'dart:io';

import 'package:flutter/foundation.dart';

/// Small non-secret UI preferences (interface language…). Secrets go to
/// [SecureStore](secure_store.dart), library data to pa_library.
abstract class Preferences {
  Future<String?> read(String key);
  Future<void> write(String key, String? value);

  /// BCP 47 tag of the chosen UI language; absent = follow the system.
  static const locale = 'ui_locale';

  /// 'false' keeps playing here when another device takes the title over
  /// (SYNC-003); absent = pause.
  static const takeoverPause = 'sync_takeover_pause';

  /// 'true' shows the Twitch section in the navigation (UI-002).
  static const showTwitch = 'nav_show_twitch';

  /// Recent search queries (UI-011), a JSON list, newest first.
  static const searchHistory = 'search_history';

  /// 'list' shows a library view (`movies`, `shows`) as a list (UI-008).
  static String libraryLayout(String view) => 'library_layout_$view';

  /// Subtitle style (PLAY-007), JSON of pa_player `SubtitleSettings`.
  static const subtitleSettings = 'subtitle_settings';

  /// Subtitle track and delay remembered for a series (PLAY-007), JSON of
  /// pa_player `SubtitleMemory`; [series] is the show id.
  static String subtitleSeries(String series) => 'subtitle_series_$series';
}

/// JSON file in the application support directory.
class FilePreferences implements Preferences {
  FilePreferences(this.path);

  final String path;
  Map<String, String>? _cache;

  Future<Map<String, String>> _load() async {
    final cached = _cache;
    if (cached != null) return cached;
    try {
      final f = File(path);
      if (await f.exists()) {
        final data = jsonDecode(await f.readAsString());
        if (data is Map) {
          return _cache = {
            for (final e in data.entries)
              if (e.value is String) '${e.key}': e.value as String,
          };
        }
      }
    } catch (e) {
      debugPrint('Preferences: cannot read $path: $e');
    }
    return _cache = {};
  }

  @override
  Future<String?> read(String key) async => (await _load())[key];

  @override
  Future<void> write(String key, String? value) async {
    final m = await _load();
    if (value == null) {
      m.remove(key);
    } else {
      m[key] = value;
    }
    try {
      final f = File(path);
      await f.parent.create(recursive: true);
      await f.writeAsString(jsonEncode(m));
    } catch (e) {
      debugPrint('Preferences: cannot write $path: $e');
    }
  }
}

class MemoryPreferences implements Preferences {
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
