// SPDX-License-Identifier: MPL-2.0

import 'dart:async';

import 'package:material_ui/material_ui.dart';

import '../integration/library.dart';
import '../integration/player.dart';
import 'secure_store.dart';

/// App-wide services, provided to the tree by [Services].
class AppServices extends ChangeNotifier {
  AppServices({required this.library, required this.secureStore, MusicPlayerController? music})
      : music = music ?? MusicPlayerController();

  final Library library;
  final SecureStore secureStore;
  final MusicPlayerController music;

  /// Subtitle (artist · album) for the current music queue; the contract's
  /// PlayRequest carries only a title.
  final Map<String, String> trackSubtitles = {};

  List<MediaSource> _sources = const [];
  List<MediaSource> get sources => _sources;
  bool _loaded = false;
  bool get loaded => _loaded;

  ScanProgress? _scan;
  ScanProgress? get scanProgress => _scan;
  bool get scanning => _scan != null && !_scan!.done;

  Future<void> init() async {
    _sources = await library.sources();
    final key = await secureStore.read(SecureStore.tmdbKey);
    if (key != null && key.isNotEmpty) await library.setTmdbApiKey(key);
    _loaded = true;
    notifyListeners();
  }

  Future<void> refreshSources() async {
    _sources = await library.sources();
    notifyListeners();
  }

  Future<MediaSource> addSource(MediaSource s, {String? smbPassword}) async {
    final added = await library.addSource(s);
    if (smbPassword != null && smbPassword.isNotEmpty) {
      await secureStore.write(SecureStore.smbPassword(added.id), smbPassword);
    }
    await refreshSources();
    return added;
  }

  Future<void> removeSource(String id) async {
    await library.removeSource(id);
    await secureStore.write(SecureStore.smbPassword(id), null);
    await refreshSources();
  }

  /// Scans one source or all of them; progress is published via [scanProgress].
  Future<void> scan({String? sourceId}) async {
    if (scanning) return;
    final targets = sourceId == null ? _sources : _sources.where((s) => s.id == sourceId).toList();
    _scan = ScanProgress(filesFound: 0, filesProcessed: 0);
    notifyListeners();
    try {
      for (final s in targets) {
        final pwd = s.kind == SourceKind.smb ? await secureStore.read(SecureStore.smbPassword(s.id)) : null;
        await for (final p in library.scan(sourceId: s.id, smbPassword: pwd)) {
          _scan = ScanProgress(
            filesFound: p.filesFound,
            filesProcessed: p.filesProcessed,
            currentPath: p.currentPath,
            error: p.error,
          );
          notifyListeners();
        }
      }
      _scan = ScanProgress(
        filesFound: _scan?.filesFound ?? 0,
        filesProcessed: _scan?.filesFound ?? 0,
        done: true,
        error: _scan?.error,
      );
    } catch (e) {
      _scan = ScanProgress(filesFound: 0, filesProcessed: 0, done: true, error: '$e');
    }
    notifyListeners();
  }

  Future<String?> tmdbKey() => secureStore.read(SecureStore.tmdbKey);

  Future<void> setTmdbKey(String? key) async {
    final k = (key == null || key.trim().isEmpty) ? null : key.trim();
    await secureStore.write(SecureStore.tmdbKey, k);
    await library.setTmdbApiKey(k);
    notifyListeners();
  }

  @override
  void dispose() {
    music.dispose();
    super.dispose();
  }
}

class Services extends InheritedNotifier<AppServices> {
  const Services({super.key, required AppServices services, required super.child}) : super(notifier: services);

  static AppServices of(BuildContext context) =>
      context.dependOnInheritedWidgetOfExactType<Services>()!.notifier!;

  /// Read without subscribing to rebuilds.
  static AppServices read(BuildContext context) =>
      context.getInheritedWidgetOfExactType<Services>()!.notifier!;
}
