// SPDX-License-Identifier: MPL-2.0
//
// Search (UI-011, 01 §1.5): as-you-type search over the library, results
// grouped as movies, shows, episodes and music (people, collections and
// streamers join when the core can search them), recent searches kept on
// the device. Keyboard and remote: ↓ leaves the field for the results, the
// arrows move between them, OK / Enter opens one, Esc clears the field
// first and then goes back.

import 'package:flutter/services.dart';
import 'package:material_ui/material_ui.dart';

import '../integration/library.dart';
import '../l10n/l10n.dart';
import '../services/app_services.dart';
import '../theme/kinolune_theme.dart';
import '../widgets/cards.dart';
import '../widgets/focusable.dart';
import '../widgets/format.dart';
import 'music_screen.dart' show TrackTile;
import 'playback.dart';
import 'routes.dart';
import 'search_model.dart';

class SearchScreen extends StatefulWidget {
  const SearchScreen({super.key, this.initialQuery = ''});

  final String initialQuery;

  @override
  State<SearchScreen> createState() => _SearchScreenState();
}

class _SearchScreenState extends State<SearchScreen> {
  late final _text = TextEditingController(text: widget.initialQuery);
  final _field = FocusNode(debugLabel: 'search-field');
  LibrarySearch? _search;
  SearchHistory? _history;

  @override
  void didChangeDependencies() {
    super.didChangeDependencies();
    if (_search != null) return;
    final services = Services.of(context);
    final library = services.library;
    _search = LibrarySearch((q) => library.searchGrouped(q));
    _history = SearchHistory(services.preferences)..load();
    if (widget.initialQuery.trim().isNotEmpty) {
      _search!.submit(widget.initialQuery);
    }
  }

  @override
  void dispose() {
    _search?.dispose();
    _history?.dispose();
    _text.dispose();
    _field.dispose();
    super.dispose();
  }

  void _setText(String q) {
    _text.value = TextEditingValue(
      text: q,
      selection: TextSelection.collapsed(offset: q.length),
    );
  }

  void _clear() {
    _setText('');
    _search!.update('');
    _field.requestFocus();
  }

  void _submit(String q) {
    _search!.submit(q);
    _history!.add(q);
  }

  void _fromHistory(String q) {
    _setText(q);
    _submit(q);
  }

  /// Opens a result and remembers the query that found it.
  void _open(VoidCallback open) {
    _history!.add(_text.text);
    open();
  }

  Future<void> _openEpisode(LibraryItem e) async {
    final library = Services.of(context).library;
    final showId = e.showId;
    final show = showId == null ? null : await library.item(showId);
    if (!mounted) return;
    openItem(context, e, show: show);
  }

  KeyEventResult _fieldKey(FocusNode _, KeyEvent event) {
    if (event is! KeyDownEvent) return KeyEventResult.ignored;
    final key = event.logicalKey;
    if (key == LogicalKeyboardKey.escape && _text.text.isNotEmpty) {
      _clear();
      return KeyEventResult.handled;
    }
    if (key == LogicalKeyboardKey.arrowDown) {
      // The field would move the caret; leave it for the first result.
      _field.focusInDirection(TraversalDirection.down);
      return KeyEventResult.handled;
    }
    return KeyEventResult.ignored;
  }

  @override
  Widget build(BuildContext context) {
    final d = KDensity.of(context);
    final l = context.l10n;
    final search = _search!;

    return ListView(
      clipBehavior: Clip.none,
      padding: EdgeInsets.only(bottom: d.pageInsets.bottom + 90),
      children: [
        PageTitle(l.navSearch),
        Padding(
          padding: EdgeInsets.fromLTRB(
            d.pageInsets.left,
            0,
            d.pageInsets.right,
            24,
          ),
          child: ConstrainedBox(
            constraints: const BoxConstraints(maxWidth: 640),
            child: Focus(
              canRequestFocus: false,
              skipTraversal: true,
              onKeyEvent: _fieldKey,
              child: ListenableBuilder(
                listenable: _text,
                builder: (context, _) => TextField(
                  controller: _text,
                  focusNode: _field,
                  autofocus: d.isDesktop,
                  onChanged: search.update,
                  onSubmitted: _submit,
                  textInputAction: TextInputAction.search,
                  style: Theme.of(context).textTheme.bodyLarge,
                  decoration: InputDecoration(
                    prefixIcon: const Icon(Icons.search),
                    hintText: l.searchHint,
                    suffixIcon: _text.text.isEmpty
                        ? null
                        : IconButton(
                            tooltip: l.commonClear,
                            onPressed: _clear,
                            icon: const Icon(Icons.close),
                          ),
                  ),
                ),
              ),
            ),
          ),
        ),
        ListenableBuilder(
          listenable: Listenable.merge([search, _history!]),
          builder: (context, _) => _body(context, search),
        ),
      ],
    );
  }

  Widget _body(BuildContext context, LibrarySearch search) {
    final l = context.l10n;
    if (_text.text.trim().isEmpty) {
      final history = _history!.value;
      if (history.isEmpty) {
        return SizedBox(
          height: 260,
          child: EmptyState(
            icon: Icons.search,
            title: l.searchPromptTitle,
            message: l.searchPromptMessage,
          ),
        );
      }
      return _HistoryList(
        entries: history,
        onPick: _fromHistory,
        onRemove: _history!.remove,
        onClear: _history!.clear,
      );
    }
    if (search.error != null) {
      return SizedBox(
        height: 260,
        child: EmptyState(icon: Icons.error_outline, title: l.searchFailed),
      );
    }
    if (search.isEmptyResult) {
      return SizedBox(
        height: 260,
        child: EmptyState(
          icon: Icons.search_off,
          title: l.searchNoResults(search.query),
        ),
      );
    }
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [for (final g in search.groups) _group(context, g)],
    );
  }

  Widget _group(BuildContext context, SearchGroup g) {
    final d = KDensity.of(context);
    final l = context.l10n;
    final posterRowH =
        d.posterWidth * 1.5 + cardCaptionHeight(d) + (d.isTv ? 12 : 6);
    final landscapeRowH =
        d.landscapeWidth * 9 / 16 + cardCaptionHeight(d) + (d.isTv ? 12 : 6);
    final items = g.items;
    final Widget row = switch (g.kind) {
      ItemKind.movie || ItemKind.show => MediaRow(
        key: ValueKey('search-${g.kind.name}'),
        title: g.kind == ItemKind.movie ? l.navMovies : l.navShows,
        itemCount: items.length,
        height: posterRowH,
        itemBuilder: (context, i) => PosterCard(
          key: ValueKey('search-${items[i].id}'),
          item: items[i],
          width: d.posterWidth,
          onTap: () => _open(() => openItem(context, items[i])),
        ),
      ),
      ItemKind.episode => MediaRow(
        key: const ValueKey('search-episode'),
        title: l.searchEpisodes,
        itemCount: items.length,
        height: landscapeRowH,
        itemBuilder: (context, i) => LandscapeCard(
          key: ValueKey('search-${items[i].id}'),
          width: d.landscapeWidth,
          seed: items[i].id,
          imageUrl: items[i].backdropUrl ?? items[i].posterUrl,
          title: items[i].title,
          subtitle: episodeCode(l, items[i]),
          onTap: () => _open(() => _openEpisode(items[i])),
        ),
      ),
      ItemKind.track => Column(
        key: const ValueKey('search-track'),
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Padding(
            padding: EdgeInsets.fromLTRB(
              d.pageInsets.left,
              0,
              d.pageInsets.right,
              8,
            ),
            child: Text(
              l.navMusic,
              style: Theme.of(context).textTheme.titleLarge,
            ),
          ),
          Padding(
            padding: EdgeInsets.symmetric(horizontal: d.pageInsets.left),
            child: FocusTraversalGroup(
              child: Column(
                children: [
                  for (var i = 0; i < items.length; i++)
                    TrackTile(
                      key: ValueKey('search-${items[i].id}'),
                      track: items[i],
                      showArt: true,
                      onTap: () => _open(() => playTracks(context, items, i)),
                    ),
                ],
              ),
            ),
          ),
        ],
      ),
    };
    return Padding(
      padding: EdgeInsets.only(bottom: d.rowGap),
      child: row,
    );
  }
}

/// Recent searches under the empty field.
class _HistoryList extends StatelessWidget {
  const _HistoryList({
    required this.entries,
    required this.onPick,
    required this.onRemove,
    required this.onClear,
  });

  final List<String> entries;
  final ValueChanged<String> onPick;
  final ValueChanged<String> onRemove;
  final VoidCallback onClear;

  @override
  Widget build(BuildContext context) {
    final d = KDensity.of(context);
    final l = context.l10n;
    final theme = Theme.of(context);
    return Padding(
      padding: EdgeInsets.symmetric(horizontal: d.pageInsets.left),
      child: ConstrainedBox(
        constraints: const BoxConstraints(maxWidth: 640),
        child: FocusTraversalGroup(
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Row(
                children: [
                  Expanded(
                    child: Text(
                      l.searchHistoryTitle,
                      style: theme.textTheme.titleLarge,
                      maxLines: 1,
                      overflow: TextOverflow.ellipsis,
                    ),
                  ),
                  TextButton(
                    onPressed: onClear,
                    child: Text(l.searchHistoryClear),
                  ),
                ],
              ),
              const SizedBox(height: 4),
              for (final q in entries)
                Row(
                  key: ValueKey('search-history-$q'),
                  children: [
                    Expanded(
                      child: KFocusable(
                        onActivate: () => onPick(q),
                        focusScale: 1.0,
                        light: false,
                        semanticLabel: q,
                        builder: (context, focused) => AnimatedContainer(
                          duration: KMotion.focus,
                          padding: const EdgeInsets.symmetric(
                            horizontal: 10,
                            vertical: 10,
                          ),
                          color: focused
                              ? Colors.white.withValues(alpha: .09)
                              : Colors.transparent,
                          child: Row(
                            children: [
                              Icon(
                                Icons.history,
                                size: 20,
                                color: KColors.textTertiary,
                              ),
                              const SizedBox(width: 12),
                              Expanded(
                                child: Text(
                                  q,
                                  maxLines: 1,
                                  overflow: TextOverflow.ellipsis,
                                  style: theme.textTheme.bodyLarge,
                                ),
                              ),
                            ],
                          ),
                        ),
                      ),
                    ),
                    IconButton(
                      tooltip: l.searchHistoryRemove(q),
                      onPressed: () => onRemove(q),
                      icon: const Icon(Icons.close, size: 18),
                    ),
                  ],
                ),
            ],
          ),
        ),
      ),
    );
  }
}
