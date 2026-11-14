// SPDX-License-Identifier: MPL-2.0

import 'dart:async';
import 'dart:convert';

import 'package:material_ui/material_ui.dart';

import '../integration/library.dart';
import '../integration/player.dart';
import '../l10n/l10n.dart';
import '../library/library_queries.dart';
import '../sources/discovery.dart';
import '../sources/library_source_manager.dart';
import '../sources/source_manager.dart';
import '../sync/device_sync.dart';
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
    this.sync = const NoDeviceSync(),
    this.onRemoteLibraryChange,
    LibraryQueries? queries,
    DateTime Function()? clock,
  }) : preferences = preferences ?? MemoryPreferences(),
       queries = queries ?? LibraryQueries.forLibrary(library),
       clock = clock ?? DateTime.now,
       sourceManager = sourceManager ?? LibrarySourceManager(library),
       systemLocale = systemLocale ?? _platformLocale,
       music =
           music ?? MusicPlayerController(engineFactory: playerEngineFactory);

  final Library library;

  /// Queries of the library grid (UI-008): LIB-009 on the Rust core, in
  /// memory without filters on the v0 fallback.
  final LibraryQueries queries;

  /// Current time ("new" badges); tests pin it.
  final DateTime Function() clock;

  final SecureStore secureStore;
  final Preferences preferences;
  final MusicPlayerController music;

  /// Settings and status of sources (UI-005): the Rust source manager, or
  /// the reduced v0 one ([LibrarySourceManager]).
  final SourceManager sourceManager;

  /// LAN discovery for the add flow: `CoreSourceDiscovery` on the Rust
  /// core (NET-020), off on the v0 fallback.
  final SourceDiscovery discovery;

  /// Pairing and LAN sync with the user's other devices (UI-019).
  final DeviceSync sync;

  /// Refreshes the library views when another device changed progress;
  /// default: [RustLibrary.notifyExternalChange].
  final void Function()? onRemoteLibraryChange;
  StreamSubscription<SyncEvent>? _syncSub;

  /// What other devices play now (`pb.update` / `pb.takeover`): the hook
  /// for the "continue here" banner of SYNC-003. Nothing listens yet.
  Stream<SyncEvent> get playbackElsewhere => sync.events.where(
    (e) => e.kind == SyncEventKind.playback || e.kind == SyncEventKind.takeover,
  );

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

  /// Subtitle style (PLAY-007): the system caption style by default.
  final ValueNotifier<SubtitleSettings> subtitleSettings = ValueNotifier(
    const SubtitleSettings(),
  );

  /// Languages the user reads subtitles in, best first: the UI language.
  /// The player puts the system caption language before them.
  List<String> get subtitleLanguages => [
    (locale.value ?? systemLocale()).languageCode,
  ];

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
    subtitleSettings.value = SubtitleSettings.fromJson(
      _json(await preferences.read(Preferences.subtitleSettings)),
    );
    _sources = await library.sources();
    await _loadManaged();
    await migrateLegacySmbPasswords();
    final key = await secureStore.read(SecureStore.tmdbKey);
    if (key != null && key.isNotEmpty) await library.setTmdbApiKey(key);
    _loaded = true;
    notifyListeners();
    unawaited(startWatching());
    unawaited(startSync());
  }

  /// Listens to the sync service and starts it if devices are paired.
  Future<void> startSync() async {
    _syncSub ??= sync.events.listen(_onSyncEvent);
    try {
      await sync.start();
    } catch (e) {
      // Pairing still works from the Devices screen; sync retries there.
      debugPrint('Sync not started: $e');
    }
  }

  void _onSyncEvent(SyncEvent e) {
    switch (e.kind) {
      case SyncEventKind.libraryChanged:
        final refresh = onRemoteLibraryChange;
        if (refresh != null) {
          refresh();
        } else if (library is RustLibrary) {
          (library as RustLibrary).notifyExternalChange();
        }
      case SyncEventKind.devicesChanged ||
          SyncEventKind.paired ||
          SyncEventKind.inviteClosed:
        notifyListeners();
      case SyncEventKind.pairingFailed ||
          SyncEventKind.playback ||
          SyncEventKind.takeover:
        break;
    }
  }

  /// Follows the auto-scan sources (LIB-005): new and removed files show up
  /// in the library on their own, network shares are rescanned on a timer.
  /// With [scanOnStart] every source is scanned once now (changes made
  /// while the app was closed). Restarted when the sources change.
  Future<void> startWatching({bool scanOnStart = true}) async {
    try {
      await library.startWatching(scanOnStart: scanOnStart);
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

  /// Adds a source through [sourceManager]. Its password stays in the
  /// core's secret store only (LIB-018): the core reads the share itself.
  Future<ManagedSource> createSource(SourceDraft draft) async {
    final added = await sourceManager.create(draft);
    await refreshSources();
    // The add dialog scans the new source itself.
    unawaited(startWatching(scanOnStart: false));
    return added;
  }

  /// Saves edited settings; [SourceDraft.password] null keeps the password,
  /// empty removes it. The watcher follows the new auto-scan setting.
  Future<ManagedSource> updateSource(String id, SourceDraft draft) async {
    final saved = await sourceManager.update(id, draft);
    await refreshSources();
    unawaited(startWatching(scanOnStart: false));
    return saved;
  }

  /// Moves SMB passwords that app versions before LIB-018 kept in
  /// [secureStore] (`smb_password_<id>`) into the core's secret store, once,
  /// then deletes the old copy. A source that already has a password in
  /// the core keeps it; the old copy is only deleted. On the reduced
  /// manager (no core secret store) nothing is moved or deleted.
  Future<void> migrateLegacySmbPasswords() async {
    if (sourceManager.reduced || !sourceManager.canEdit) return;
    for (final m in _managed) {
      if (m.type != SourceType.smb) continue;
      final key = SecureStore.smbPassword(m.id);
      final String? old;
      try {
        old = await secureStore.read(key);
      } catch (e) {
        debugPrint('Old SMB password of source ${m.id} not readable: $e');
        continue;
      }
      if (old == null) continue;
      try {
        if (!m.hasPassword && old.isNotEmpty) {
          await sourceManager.update(m.id, draftOf(m, password: old));
        }
        await secureStore.write(key, null);
      } catch (e) {
        // Kept for the next start.
        debugPrint('SMB password of source ${m.id} not migrated: $e');
      }
    }
    await _loadManaged();
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

  Future<MediaSource> addSource(MediaSource s) async {
    final added = await library.addSource(s);
    await refreshSources();
    // The add dialog scans the new source itself.
    unawaited(startWatching(scanOnStart: false));
    return added;
  }

  Future<void> removeSource(String id) async {
    await library.removeSource(id);
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
        String? sourceError;
        await for (final p in library.scan(sourceId: s.id)) {
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

  Future<void> setSubtitleSettings(SubtitleSettings value) async {
    subtitleSettings.value = value;
    await preferences.write(
      Preferences.subtitleSettings,
      jsonEncode(value.toJson()),
    );
  }

  /// Subtitle track and delay remembered for a series (the show id).
  Future<SubtitleMemory?> subtitleMemory(String series) async {
    final raw = await preferences.read(Preferences.subtitleSeries(series));
    return raw == null ? null : SubtitleMemory.fromJson(_json(raw));
  }

  Future<void> saveSubtitleMemory(String series, SubtitleMemory memory) =>
      preferences.write(
        Preferences.subtitleSeries(series),
        jsonEncode(memory.toJson()),
      );

  static Object? _json(String? raw) {
    if (raw == null) return null;
    try {
      return jsonDecode(raw);
    } on FormatException {
      return null;
    }
  }

  Future<void> setShowTwitch(bool value) async {
    showTwitch.value = value;
    await preferences.write(Preferences.showTwitch, value ? 'true' : null);
  }

  @override
  void dispose() {
    unawaited(_syncSub?.cancel());
    unawaited(library.stopWatching());
    locale.dispose();
    showTwitch.dispose();
    subtitleSettings.dispose();
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
