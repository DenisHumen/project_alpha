// SPDX-License-Identifier: MPL-2.0
//
// State of the Search screen (UI-011): debounced as-you-type search and the
// local search history.

import 'dart:async';
import 'dart:convert';

import 'package:flutter/foundation.dart';

import '../integration/library.dart';
import '../services/preferences.dart';

/// Pause after the last keystroke before searching. With the core query
/// under 50 ms on 50 000 items (pa-catalog `tests/search.rs`), results
/// appear within 100 ms of the input.
const searchDebounce = Duration(milliseconds: 50);

/// Grouped search function, e.g. [Library.searchGrouped].
typedef GroupedSearch = Future<List<SearchGroup>> Function(String query);

/// Debounced as-you-type search. [update] on every edit; listeners are told
/// when [groups] (or [busy], [error]) change. A result of an older query is
/// dropped when a newer one was asked meanwhile.
class LibrarySearch extends ChangeNotifier {
  LibrarySearch(this._search, {this.debounce = searchDebounce});

  final GroupedSearch _search;
  final Duration debounce;

  Timer? _timer;
  int _generation = 0;
  bool _disposed = false;

  /// The trimmed query the shown [groups] belong to.
  String get query => _query;
  String _query = '';

  /// Results of [query]; empty when it is empty or nothing matched.
  List<SearchGroup> get groups => _groups;
  List<SearchGroup> _groups = const [];

  /// A search is pending or running.
  bool get busy => _busy;
  bool _busy = false;

  /// Why the last search failed, if it did.
  Object? get error => _error;
  Object? _error;

  /// Whether [query] was searched and nothing matched.
  bool get isEmptyResult =>
      _query.isNotEmpty && !_busy && _error == null && _groups.isEmpty;

  /// The text of the field changed: search after [debounce]. An empty
  /// text clears the results at once.
  void update(String text) {
    final q = text.trim();
    _timer?.cancel();
    if (q.isEmpty) {
      _generation++;
      _set(q, const [], busy: false, error: null);
      return;
    }
    if (q == _query && !_busy && _error == null) return;
    // A search still running for an older text is dropped.
    _generation++;
    _busy = true;
    _timer = Timer(debounce, () => _run(q));
    notifyListeners();
  }

  /// Searches [text] now (Enter, a history entry).
  Future<void> submit(String text) {
    _timer?.cancel();
    final q = text.trim();
    if (q.isEmpty) {
      update(q);
      return Future.value();
    }
    return _run(q);
  }

  Future<void> _run(String q) async {
    final gen = ++_generation;
    _busy = true;
    try {
      final groups = await _search(q);
      if (gen != _generation || _disposed) return;
      _set(q, groups, busy: false, error: null);
    } catch (e) {
      if (gen != _generation || _disposed) return;
      _set(q, const [], busy: false, error: e);
    }
  }

  void _set(
    String q,
    List<SearchGroup> groups, {
    required bool busy,
    required Object? error,
  }) {
    _query = q;
    _groups = groups;
    _busy = busy;
    _error = error;
    if (!_disposed) notifyListeners();
  }

  @override
  void dispose() {
    _disposed = true;
    _timer?.cancel();
    super.dispose();
  }
}

/// Recent search queries, newest first, kept in [Preferences] on this
/// device only.
class SearchHistory extends ValueNotifier<List<String>> {
  SearchHistory(this._prefs, {this.max = 10}) : super(const []);

  final Preferences _prefs;

  /// Most entries kept.
  final int max;

  /// Reads the saved history.
  Future<void> load() async {
    try {
      final raw = await _prefs.read(Preferences.searchHistory);
      if (raw == null) return;
      final data = jsonDecode(raw);
      if (data is List) {
        value = data.whereType<String>().take(max).toList(growable: false);
      }
    } on FormatException {
      value = const [];
    }
  }

  /// Puts [query] first; an entry differing only in case is replaced.
  Future<void> add(String query) async {
    final q = query.trim();
    if (q.isEmpty) return;
    final key = q.toLowerCase();
    await _save([
      q,
      ...value.where((e) => e.toLowerCase() != key),
    ].take(max).toList(growable: false));
  }

  Future<void> remove(String query) =>
      _save(value.where((e) => e != query).toList(growable: false));

  Future<void> clear() => _save(const []);

  Future<void> _save(List<String> list) async {
    value = list;
    await _prefs.write(
      Preferences.searchHistory,
      list.isEmpty ? null : jsonEncode(list),
    );
  }
}
