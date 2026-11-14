// SPDX-License-Identifier: MPL-2.0

import 'dart:async';

import 'package:material_ui/material_ui.dart';

import '../integration/library.dart';
import '../integration/player.dart';
import '../l10n/l10n.dart';
import '../sources/discovery.dart';
import '../sources/library_source_manager.dart';
import '../sources/source_manager.dart';
import 'preferences.dart';
import 'secure_store.dart';

/// App-wide services, provided to the tree by [Services].
class AppServices extends ChangeNotifier {
  AppServices({
    required this.library,
    required this.secureStore,
    Preferences? preferences,
    MusicPlayerController? music,
    this.playerEngineFactory,
    Locale Function()? systemLocale,
    SourceManager? sourceManager,
    this.discovery = const NoSourceDiscovery(),
  }) : preferences = preferences ?? MemoryPreferences(),
       sourceManager = sourceManager ?? LibrarySourceManager(library),
       systemLocale = systemLocale ?? _platformLocale,
       music =
           music ?? MusicPlayerController(engineFactory: playerEngineFactory);

  final Library library;
  final SecureStore secureStore;
  final Preferences preferences;
  final MusicPlayerController music;

  /// Settings and status of sources (UI-005): the Rust source manager, or
  /// the reduced v0 one ([LibrarySourceManager]).
  final SourceManager sourceManager;

  /// LAN discovery for the add flow (hook; off until pa-discovery is in
  /// pa_core).
  final SourceDiscovery discovery;

  /// UI language chosen in Settings; null = follow the system locale.
  final ValueNotifier<Locale?> locale = ValueNotifier(null);

  /// The system UI language, for [locale] == null (tests override it).
  final Locale Function() systemLocale;

  /// The first preferred system language the app has strings for (as the
  /// framework resolves it), else English.
  static Locale _platformLocale() {
    final supported = {
      for (final l in AppLocalizations.supportedLocales) l.languageCode,
    };
    for (final l in WidgetsBinding.instance.platformDispatcher.locales) {
      if (supported.contains(l.languageCode)) return l;
    }
    return const Locale('en');
  }

  /// Whether the Twitch section is in the navigation. Off by default until
  /// the Twitch feature ships (after release 0.1); toggled in Settings.
  final ValueNotifier<bool> showTwitch = ValueNotifier(false);

  /// Overrides the media_kit engine (tests use pa_player's FakePlayerEngine).
  final PlayerEngineFactory? playerEngineFactory;

  List<MediaSource> _sources = const [];
  List<MediaSource> get sources => _sources;

  List<ManagedSource> _managed = const [];

  /// [sources] with settings and status, from [sourceManager].
  List<ManagedSource> get managedSources => _managed;

  /// Why [managedSources] could not be read, if it failed.
  Object? get managedSourcesError => _managedError;
  Object? _managedError;

  /// End time and error of each source's last scan in this session (the
  /// reduced manager has no stored status).
  final Map<String, ({DateTime at, String? error})> sessionScans = {};

  /// Sources a scan is running for (all of them for a full scan).
  final Set<String> scanningSources = {};
  bool _loaded = false;
  bool get loaded => _loaded;

  ScanProgress? _scan;
  bool _scanRunning = false;
  ScanProgress? get scanProgress => _scan;
  bool get scanning => _scanRunning;

  Future<void> init() async {
    locale.value = parseLocaleTag(await preferences.read(Preferences.locale));
    await _syncMetadataLanguage();
    showTwitch.value = await preferences.read(Preferences.showTwitch) == 'true';
    _sources = await library.sources();
    await _loadManaged();
    final key = await secureStore.read(SecureStore.tmdbKey);
    if (key != null && key.isNotEmpty) await library.setTmdbApiKey(key);
    _loaded = true;
    notifyListeners();
    unawaited(startWatching());
  }

  /// Follows the auto-scan sources (LIB-005): new and removed files show up
  /// in the library on their own, network shares are rescanned on a timer.
  /// With [scanOnStart] every source is scanned once now (changes made
  /// while the app was closed). Restarted when the sources change.
  Future<void> startWatching({bool scanOnStart = true}) async {
    final passwords = <String, String>{};
    for (final s in _sources) {
      if (s.kind != SourceKind.smb) continue;
      final pwd = await secureStore.read(SecureStore.smbPassword(s.id));
      if (pwd != null && pwd.isNotEmpty) passwords[s.id] = pwd;
    }
    try {
      await library.startWatching(
        smbPasswords: passwords,
        scanOnStart: scanOnStart,
      );
    } catch (e) {
      // The library still works; manual scans stay available.
      debugPrint('Library watcher not started: $e');
    }
  }

  Future<void> refreshSources() async {
    _sources = await library.sources();
    await _loadManaged();
    notifyListeners();
  }

  Future<void> _loadManaged() async {
    try {
      _managed = await sourceManager.list();
      _managedError = null;
    } catch (e) {
      _managedError = e;
    }
  }

  ManagedSource? managedSource(String id) =>
      _managed.where((s) => s.id == id).firstOrNull;

  /// Adds a source through [sourceManager]. An SMB password is also kept in
  /// [secureStore]: the scan mounts the share with it (the core never hands
  /// a stored password back).
  Future<ManagedSource> createSource(SourceDraft draft) async {
    final added = await sourceManager.create(draft);
    final pwd = draft.password;
    if (draft.type == SourceType.smb && pwd != null && pwd.isNotEmpty) {
      await secureStore.write(SecureStore.smbPassword(added.id), pwd);
    }
    await refreshSources();
    return added;
  }

  /// Saves edited settings; [SourceDraft.password] null keeps the password,
  /// empty removes it.
  Future<ManagedSource> updateSource(String id, SourceDraft draft) async {
    final saved = await sourceManager.update(id, draft);
    final pwd = draft.password;
    if (pwd != null) {
      await secureStore.write(
        SecureStore.smbPassword(id),
        pwd.isEmpty || draft.type != SourceType.smb ? null : pwd,
      );
    }
    await refreshSources();
    return saved;
  }

  /// Removes a source: its settings and password, then its library items.
  Future<void> deleteSource(String id) async {
    await sourceManager.delete(id);
    await removeSource(id);
  }

  /// Checks a source; the new status is in [managedSources].
  Future<SourceStatusInfo> checkSource(String id) async {
    try {
      return await sourceManager.check(id);
    } finally {
      await refreshSources();
    }
  }

  Future<SpeedResult> speedTest(String id, {int maxSeconds = 10}) async {
    try {
      return await sourceManager.speedTest(id, maxSeconds: maxSeconds);
    } finally {
      await refreshSources();
    }
  }

  /// Whether a full scan includes [s]: not for browse-only sources nor for
  /// kinds the scan cannot read yet (WebDAV).
  bool _scannable(MediaSource s, {required bool explicit}) {
    final m = managedSource(s.id);
    if (m == null) return true;
    if (!m.type.indexable) return false;
    return explicit || m.scanIntoLibrary;
  }

  Future<MediaSource> addSource(MediaSource s, {String? smbPassword}) async {
    final added = await library.addSource(s);
    if (smbPassword != null && smbPassword.isNotEmpty) {
      await secureStore.write(SecureStore.smbPassword(added.id), smbPassword);
    }
    await refreshSources();
    // The add dialog scans the new source itself.
    unawaited(startWatching(scanOnStart: false));
    return added;
  }

  Future<void> removeSource(String id) async {
    await library.removeSource(id);
    await secureStore.write(SecureStore.smbPassword(id), null);
    await refreshSources();
    unawaited(startWatching(scanOnStart: false));
  }

  /// Scans one source or all of them; progress is published via [scanProgress].
  Future<void> scan({String? sourceId}) async {
    if (_scanRunning) return;
    final targets = sourceId == null
        ? _sources.where((s) => _scannable(s, explicit: false)).toList()
        : _sources
              .where((s) => s.id == sourceId && _scannable(s, explicit: true))
              .toList();
    if (sourceId != null && targets.isEmpty) return;
    _scanRunning = true;
    scanningSources.addAll(targets.map((s) => s.id));
    _scan = const ScanProgress(phase: ScanPhase.connecting);
    notifyListeners();
    final errors = <String>[];
    var found = 0;
    try {
      for (final s in targets) {
        final pwd = s.kind == SourceKind.smb
            ? await secureStore.read(SecureStore.smbPassword(s.id))
            : null;
        String? sourceError;
        await for (final p in library.scan(sourceId: s.id, smbPassword: pwd)) {
          _scan = p;
          sourceError ??= p.error;
          if (p.isDone) {
            found += p.filesFound;
            if (p.error != null) errors.add('${s.name}: ${p.error}');
          }
          notifyListeners();
        }
        sessionScans[s.id] = (at: DateTime.now(), error: sourceError);
        scanningSources.remove(s.id);
      }
    } catch (e) {
      errors.add('$e');
    }
    _scanRunning = false;
    scanningSources.clear();
    await _loadManaged();
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

  /// Provider metadata follows the UI language (UI-024); stored metadata in
  /// another language is fetched again on the next scan.
  Future<void> _syncMetadataLanguage() => library.setMetadataLanguage(
    metadataLanguageFor(locale.value ?? systemLocale()),
  );

  Future<void> setLocale(Locale? value) async {
    locale.value = value;
    await _syncMetadataLanguage();
    await preferences.write(
      Preferences.locale,
      value == null ? null : localeTag(value),
    );
  }

  Future<void> setShowTwitch(bool value) async {
    showTwitch.value = value;
    await preferences.write(Preferences.showTwitch, value ? 'true' : null);
  }

  @override
  void dispose() {
    unawaited(library.stopWatching());
    locale.dispose();
    showTwitch.dispose();
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
