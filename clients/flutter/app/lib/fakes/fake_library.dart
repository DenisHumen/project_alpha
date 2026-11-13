// SPDX-License-Identifier: MPL-2.0
//
// In-memory implementation of the pa_library `Library` interface.
//
// * `FakeLibrary()` starts empty (onboarding is shown); a scan "finds" the
//   demo library so the whole flow can be tried before R01-LIB lands.
// * `FakeLibrary.demo()` starts with the demo library (screenshots, tests,
//   `--dart-define=KINOLUNE_DEMO=true`).

import 'dart:async';

import '../integration/library.dart';
import 'demo_data.dart';

class FakeLibrary implements Library {
  FakeLibrary({this.scanStepDelay = const Duration(milliseconds: 60)});

  FakeLibrary.demo({this.scanStepDelay = const Duration(milliseconds: 60)}) {
    _sources.add(MediaSource(id: 'src-demo', kind: SourceKind.localFolder, name: 'Demo library', path: '/media'));
    _load(buildDemoLibraryData());
  }

  final Duration scanStepDelay;
  final List<MediaSource> _sources = [];
  final _items = _Live<List<LibraryItem>>(const []);
  final _progress = _Live<Map<String, Progress>>(const {});
  String? tmdbApiKey;
  int _nextId = 1;

  void _load(DemoLibraryData d) {
    _items.value = [...d.movies, ...d.shows, ...d.episodes, ...d.tracks];
    _progress.value = {for (final p in d.progress) p.itemId: p};
  }

  @override
  Future<List<MediaSource>> sources() async => List.unmodifiable(_sources);

  @override
  Future<MediaSource> addSource(MediaSource s) async {
    if (s.id.isEmpty) s.id = 'src-${_nextId++}';
    _sources.add(s);
    return s;
  }

  @override
  Future<void> removeSource(String id) async {
    _sources.removeWhere((s) => s.id == id);
    if (_sources.isEmpty) {
      _items.value = const [];
      _progress.value = const {};
    }
  }

  @override
  Stream<ScanProgress> scan({String? sourceId, String? smbPassword}) async* {
    if (_sources.isEmpty) {
      yield ScanProgress(filesFound: 0, filesProcessed: 0, done: true, error: 'No sources');
      return;
    }
    final data = buildDemoLibraryData();
    final all = [...data.movies, ...data.episodes, ...data.tracks];
    final total = all.length;
    for (var i = 0; i < total; i += 7) {
      await Future<void>.delayed(scanStepDelay);
      yield ScanProgress(filesFound: total, filesProcessed: i, currentPath: all[i].filePath);
    }
    _load(data);
    yield ScanProgress(filesFound: total, filesProcessed: total, done: true);
  }

  @override
  Future<void> setTmdbApiKey(String? key) async => tmdbApiKey = key;

  Stream<List<LibraryItem>> _ofKind(ItemKind kind, [bool Function(LibraryItem)? where]) =>
      _items.stream.map((all) => all.where((i) => i.kind == kind && (where?.call(i) ?? true)).toList());

  @override
  Stream<List<LibraryItem>> movies() => _ofKind(ItemKind.movie);

  @override
  Stream<List<LibraryItem>> shows() => _ofKind(ItemKind.show);

  @override
  Stream<List<LibraryItem>> episodes(String showId) => _ofKind(ItemKind.episode, (i) => i.showId == showId)
      .map((l) => l..sort((a, b) => ((a.season ?? 0) * 1000 + (a.episode ?? 0)).compareTo((b.season ?? 0) * 1000 + (b.episode ?? 0))));

  @override
  Stream<List<LibraryItem>> tracks() => _ofKind(ItemKind.track);

  @override
  Stream<List<(LibraryItem, Progress)>> continueWatching() {
    final ctrl = StreamController<List<(LibraryItem, Progress)>>();
    List<LibraryItem> items = _items.value;
    Map<String, Progress> progress = _progress.value;
    void emit() {
      final byId = {for (final i in items) i.id: i};
      final list = <(LibraryItem, Progress)>[];
      for (final p in progress.values) {
        final item = byId[p.itemId];
        if (item == null || p.durationMs <= 0) continue;
        final f = p.positionMs / p.durationMs;
        if (f >= .02 && f <= .95 && !p.watched) list.add((item, p));
      }
      list.sort((a, b) => b.$2.updatedAt.compareTo(a.$2.updatedAt));
      ctrl.add(list);
    }

    late StreamSubscription<List<LibraryItem>> s1;
    late StreamSubscription<Map<String, Progress>> s2;
    ctrl.onListen = () {
      s1 = _items.stream.listen((v) {
        items = v;
        emit();
      });
      s2 = _progress.stream.listen((v) {
        progress = v;
        emit();
      });
    };
    ctrl.onCancel = () async {
      await s1.cancel();
      await s2.cancel();
    };
    return ctrl.stream;
  }

  /// Not part of the contract: progress lookup used by fakes/tests.
  Progress? progressOf(String itemId) => _progress.value[itemId];

  @override
  Future<void> saveProgress(Progress p) async {
    if (p.durationMs > 0 && p.positionMs / p.durationMs > .95) p.watched = true;
    _progress.value = {..._progress.value, p.itemId: p};
  }

  @override
  Future<void> fixMatch(String itemId, {int? tmdbId, String? title, int? year}) async {
    _items.value = [
      for (final i in _items.value)
        if (i.id == itemId)
          (i
            ..title = (title != null && title.isNotEmpty) ? title : i.title
            ..year = year ?? i.year
            ..matchConfidence = 1.0)
        else
          i,
    ];
  }
}

/// A tiny "behaviour subject": new listeners get the current value first.
class _Live<T> {
  _Live(this._value);

  T _value;
  final _ctrl = StreamController<T>.broadcast();

  T get value => _value;
  set value(T v) {
    _value = v;
    _ctrl.add(v);
  }

  Stream<T> get stream async* {
    yield _value;
    yield* _ctrl.stream;
  }
}
