// SPDX-License-Identifier: MPL-2.0

import 'dart:async';

import 'package:material_ui/material_ui.dart';

import '../integration/library.dart';
import '../integration/player.dart';
import 'secure_store.dart';

/// App-wide services, provided to the tree by [Services].
class AppServices extends ChangeNotifier {
  AppServices({
    required this.library,
    required this.secureStore,
    MusicPlayerController? music,
    this.playerEngineFactory,
  }) : music =
           music ?? MusicPlayerController(engineFactory: playerEngineFactory);

  final Library library;
  final SecureStore secureStore;
  final MusicPlayerController music;

  /// Overrides the media_kit engine (tests use pa_player's FakePlayerEngine).
  final PlayerEngineFactory? playerEngineFactory;

  List<MediaSource> _sources = const [];
  List<MediaSource> get sources => _sources;
  bool _loaded = false;
  bool get loaded => _loaded;

  ScanProgress? _scan;
  bool _scanRunning = false;
  ScanProgress? get scanProgress => _scan;
  bool get scanning => _scanRunning;

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
    if (_scanRunning) return;
    final targets = sourceId == null
        ? _sources
        : _sources.where((s) => s.id == sourceId).toList();
    _scanRunning = true;
    _scan = const ScanProgress(phase: ScanPhase.connecting);
    notifyListeners();
    final errors = <String>[];
    var found = 0;
    try {
      for (final s in targets) {
        final pwd = s.kind == SourceKind.smb
            ? await secureStore.read(SecureStore.smbPassword(s.id))
            : null;
        await for (final p in library.scan(sourceId: s.id, smbPassword: pwd)) {
          _scan = p;
          if (p.isDone) {
            found += p.filesFound;
            if (p.error != null) errors.add('${s.name}: ${p.error}');
          }
          notifyListeners();
        }
      }
    } catch (e) {
      errors.add('$e');
    }
    _scanRunning = false;
    _scan = ScanProgress(
      phase: ScanPhase.done,
      filesFound: found,
      processed: found,
      total: found,
      error: errors.isEmpty ? null : errors.join('\n'),
    );
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
  const Services({
    super.key,
    required AppServices services,
    required super.child,
  }) : super(notifier: services);

  static AppServices of(BuildContext context) =>
      context.dependOnInheritedWidgetOfExactType<Services>()!.notifier!;

  /// Read without subscribing to rebuilds.
  static AppServices read(BuildContext context) =>
      context.getInheritedWidgetOfExactType<Services>()!.notifier!;
}
