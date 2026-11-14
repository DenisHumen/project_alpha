// SPDX-License-Identifier: MPL-2.0

import 'dart:async';

import 'package:flutter/foundation.dart';

import '../integration/library.dart';
import 'library_queries.dart';

/// The items of one query, loaded page by page as the grid asks for them
/// (UI-008: lazy builders over 10 000+ titles). The first page is watched,
/// so a library change reloads it and drops the other pages.
class PagedItems extends ChangeNotifier {
  PagedItems(this.queries, this.query, {this.pageSize = 120}) {
    _sub = queries
        .watchQuery(query, limit: pageSize)
        .listen(_onFirstPage, onError: _onError);
  }

  final LibraryQueries queries;
  final LibraryQuery query;
  final int pageSize;

  StreamSubscription<LibraryPage>? _sub;
  final _pages = <int, List<LibraryItem>>{};
  final _loading = <int>{};
  int _generation = 0;
  bool _disposed = false;

  int? _total;

  /// Bumped on every new first page (a query result or a library change).
  int get generation => _generation;

  /// Matching items; null until the first page is in.
  int? get total => _total;

  Object? error;

  void _onFirstPage(LibraryPage p) {
    _generation++;
    _pages
      ..clear()
      ..[0] = p.items;
    _loading.clear();
    _total = p.total;
    error = null;
    notifyListeners();
  }

  void _onError(Object e) {
    error = e;
    _total ??= 0;
    notifyListeners();
  }

  /// Item [index], or null while its page loads (the load starts here).
  LibraryItem? itemAt(int index) {
    final page = index ~/ pageSize;
    final items = _pages[page];
    if (items == null) {
      _load(page);
      return null;
    }
    final i = index - page * pageSize;
    return i < items.length ? items[i] : null;
  }

  void _load(int page) {
    if (!_loading.add(page)) return;
    final gen = _generation;
    queries
        .query(query, offset: page * pageSize, limit: pageSize)
        .then(
          (p) {
            if (_disposed || gen != _generation) return;
            _loading.remove(page);
            _pages[page] = p.items;
            notifyListeners();
          },
          onError: (Object e) {
            if (_disposed || gen != _generation) return;
            _loading.remove(page);
            error = e;
            notifyListeners();
          },
        );
  }

  @override
  void dispose() {
    _disposed = true;
    _sub?.cancel();
    super.dispose();
  }
}
