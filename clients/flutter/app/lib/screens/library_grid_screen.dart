// SPDX-License-Identifier: MPL-2.0
//
// Library grid of Movies and TV Shows (UI-008): 2:3 posters or a list,
// combinable filter chips from the library facets, a sort menu saved per
// view, an alphabet index, watch badges and a context menu. Items load page
// by page (PagedItems) and the grid builds only what is on screen. On the
// v0 fallback (Rust core off) the filters are hidden.

import 'dart:async';

import 'package:flutter/rendering.dart';
import 'package:material_ui/material_ui.dart';

import '../integration/library.dart';
import '../l10n/l10n.dart';
import '../library/library_queries.dart';
import '../library/paged_items.dart';
import '../services/app_services.dart';
import '../services/preferences.dart';
import '../shell/app_shell.dart';
import '../shell/breakpoints.dart';
import '../theme/kinolune_theme.dart';
import '../widgets/alphabet_scrubber.dart';
import '../widgets/artwork.dart';
import '../widgets/cards.dart';
import '../widgets/focusable.dart';
import '../widgets/format.dart';
import 'library_item_menu.dart';
import 'routes.dart';

/// How long a title counts as "new" after it was added.
const newBadgeAge = Duration(days: 14);

/// Sort fields in the menu, with the direction picked first.
const _sortDefaults = <SortField, bool>{
  SortField.title: false,
  SortField.added: true,
  SortField.year: true,
  SortField.lastWatched: true,
  SortField.duration: false,
};

String _sortLabel(AppLocalizations l, SortField f) => switch (f) {
  SortField.title => l.librarySortTitle,
  SortField.year => l.librarySortYear,
  SortField.added => l.librarySortAdded,
  SortField.duration => l.librarySortDuration,
  SortField.lastWatched => l.librarySortLastWatched,
};

String _resolutionLabel(VideoResolution r) => switch (r) {
  VideoResolution.sd => 'SD', // l10n-ignore
  VideoResolution.hd => '720p', // l10n-ignore
  VideoResolution.fullHd => '1080p', // l10n-ignore
  VideoResolution.uhd => '4K', // l10n-ignore
};

String _hdrLabel(HdrFilter h) => switch (h) {
  HdrFilter.sdr => 'SDR', // l10n-ignore
  HdrFilter.anyHdr => 'HDR', // l10n-ignore
  HdrFilter.hdr10 => 'HDR10', // l10n-ignore
  HdrFilter.hdr10Plus => 'HDR10+', // l10n-ignore
  HdrFilter.dolbyVision => 'Dolby Vision', // l10n-ignore
  HdrFilter.hlg => 'HLG', // l10n-ignore
};

/// Poster grid (or list) for Movies and Shows.
class LibraryGridScreen extends StatefulWidget {
  const LibraryGridScreen({super.key, required this.kind});

  final ItemKind kind; // movie or show

  @override
  State<LibraryGridScreen> createState() => _LibraryGridScreenState();
}

class _LibraryGridScreenState extends State<LibraryGridScreen> {
  late AppServices _services;
  LibraryQueries get _queries => _services.queries;
  String get _view => widget.kind == ItemKind.show ? 'shows' : 'movies';

  final _scroll = ScrollController();
  ViewPrefs? _prefs;
  LibrarySort _sort = const LibrarySort();
  LibraryFilter _filter = const LibraryFilter();
  bool _list = false;
  PagedItems? _paged;
  int _pagedGeneration = -1;
  LibraryFacets? _facets;
  List<AlphabetEntry> _alphabet = const [];
  Map<String, ItemBadge> _badges = const {};
  StreamSubscription<Map<String, ItemBadge>>? _badgeSub;

  // Layout of the item sliver, for alphabet jumps.
  SliverConstraints? _itemConstraints;

  bool _started = false;

  @override
  void didChangeDependencies() {
    super.didChangeDependencies();
    _services = Services.of(context);
    if (_started) return;
    _started = true;
    _badgeSub = _queries
        .badges(widget.kind)
        .listen((b) => setState(() => _badges = b));
    unawaited(_loadPrefs());
  }

  Future<void> _loadPrefs() async {
    final layout = await _services.preferences.read(
      Preferences.libraryLayout(_view),
    );
    ViewPrefs prefs;
    try {
      prefs = await _queries.viewPrefs(_view);
    } catch (_) {
      prefs = ViewPrefs(_view);
    }
    if (!mounted) return;
    setState(() {
      _prefs = prefs;
      _sort = prefs.sort;
      _list = layout == 'list';
    });
    _requery();
  }

  LibraryQuery get _query =>
      LibraryQuery(widget.kind, filter: _filter, sort: _sort);

  void _requery() {
    _paged?.removeListener(_onPaged);
    _paged?.dispose();
    _pagedGeneration = -1;
    _paged = PagedItems(_queries, _query)..addListener(_onPaged);
    _alphabet = const [];
    if (_scroll.hasClients) _scroll.jumpTo(0);
    setState(() {});
  }

  /// A new first page (query change or library change): refresh the index
  /// and the facets.
  void _onPaged() {
    final p = _paged;
    if (p == null) return;
    final gen = p.generation;
    if (gen != _pagedGeneration) {
      _pagedGeneration = gen;
      unawaited(_loadIndex(p));
      if (_queries.supportsFilters) unawaited(_loadFacets());
    }
    setState(() {});
  }

  Future<void> _loadIndex(PagedItems p) async {
    if (_sort.field != SortField.title) return;
    try {
      final a = await _queries.alphabetIndex(p.query);
      if (mounted && identical(p, _paged)) setState(() => _alphabet = a);
    } catch (_) {
      // No index; the grid still scrolls.
    }
  }

  Future<void> _loadFacets() async {
    try {
      final f = await _queries.facets(widget.kind);
      if (mounted) setState(() => _facets = f);
    } catch (_) {
      // Chips need facets; without them only the watch chips show.
    }
  }

  void _setSort(LibrarySort s) {
    if (s == _sort) return;
    _sort = s;
    final prefs = _prefs ?? ViewPrefs(_view);
    _prefs = ViewPrefs(
      _view,
      sort: s,
      foldersFirst: prefs.foldersFirst,
      filter: prefs.filter,
    );
    unawaited(_queries.setViewPrefs(_prefs!).catchError((_) {}));
    _requery();
  }

  void _setFilter(LibraryFilter f) {
    _filter = f;
    _requery();
  }

  void _setList(bool list) {
    setState(() => _list = list);
    unawaited(
      _services.preferences.write(
        Preferences.libraryLayout(_view),
        list ? 'list' : null,
      ),
    );
  }

  @override
  void dispose() {
    _badgeSub?.cancel();
    _paged?.removeListener(_onPaged);
    _paged?.dispose();
    _scroll.dispose();
    super.dispose();
  }

  CardGridDelegate _gridDelegate(KDensity d) => CardGridDelegate(
    minItemWidth: d.posterWidth,
    mainSpacing: d.isTv ? 22 : 18,
    crossSpacing: d.gap,
    artAspect: 1.5,
    extraHeight: cardCaptionHeight(d),
  );

  double _rowExtent(KDensity d) => d.isTv ? 104 : 88;

  /// Scrolls so that item [index] is at the top.
  void _jumpTo(int index) {
    final c = _itemConstraints;
    if (c == null || !_scroll.hasClients) return;
    final d = KDensity.of(context);
    final offset = _list
        ? index * _rowExtent(d)
        : _gridDelegate(d)
              .getLayout(c)
              .getGeometryForChildIndex(index)
              .scrollOffset;
    final pos = _scroll.position;
    _scroll.jumpTo(
      (c.precedingScrollExtent + offset).clamp(
        pos.minScrollExtent,
        pos.maxScrollExtent,
      ),
    );
  }

  bool _isNew(LibraryItem item, ItemBadge? badge) {
    final added = item.addedAt;
    if (added == null || badge != null) return false;
    return _services.clock().difference(added) <= newBadgeAge;
  }

  void _openMenu(LibraryItem item, Offset position) => showLibraryItemMenu(
    context,
    item,
    position: position,
    badge: _badges[item.id],
  );

  @override
  Widget build(BuildContext context) {
    final d = KDensity.of(context);
    final l = context.l10n;
    final movies = widget.kind == ItemKind.movie;
    // The phone tab bar has one Library tab for Movies and TV Shows (UI-002);
    // the switch between them lives in the page header.
    final libraryTab = NavLayout.of(context) == NavLayout.tabs;
    final paged = _paged;
    final total = paged?.total;
    final showIndex =
        _sort.field == SortField.title && _alphabet.length > 1 && !d.isTv;
    final right = showIndex && d.pageInsets.right < 32
        ? 32.0
        : d.pageInsets.right;

    final scroll = CustomScrollView(
      controller: _scroll,
      clipBehavior: Clip.none,
      slivers: [
        SliverToBoxAdapter(
          child: PageTitle(
            libraryTab ? l.navLibrary : (movies ? l.navMovies : l.navShows),
            trailing: total == null
                ? null
                : Text(
                    movies
                        ? l.libraryMovieCount(total)
                        : l.libraryShowCount(total),
                    style: Theme.of(context).textTheme.labelMedium,
                  ),
          ),
        ),
        if (libraryTab)
          SliverToBoxAdapter(
            child: Padding(
              padding: EdgeInsets.fromLTRB(
                d.pageInsets.left,
                0,
                d.pageInsets.right,
                12,
              ),
              child: _LibraryKindSwitch(movies: movies),
            ),
          ),
        SliverToBoxAdapter(
          child: Padding(
            padding: EdgeInsets.fromLTRB(
              d.pageInsets.left,
              0,
              d.pageInsets.right,
              d.isTv ? 18 : 20,
            ),
            child: _Toolbar(
              sort: _sort,
              onSort: _setSort,
              list: _list,
              onList: _setList,
              filters: _queries.supportsFilters,
              filter: _filter,
              facets: _facets,
              onFilter: _setFilter,
            ),
          ),
        ),
        if (paged == null || total == null)
          const SliverToBoxAdapter(child: SizedBox.shrink())
        else if (total == 0)
          SliverFillRemaining(
            hasScrollBody: false,
            child: EmptyState(
              icon: movies ? Icons.movie_outlined : Icons.tv_outlined,
              title: _filter.isEmpty
                  ? (movies ? l.libraryNoMovies : l.libraryNoShows)
                  : l.libraryNoMatches,
              message: _filter.isEmpty ? l.libraryEmptyHint : null,
            ),
          )
        else
          SliverPadding(
            padding: EdgeInsets.fromLTRB(
              d.pageInsets.left,
              d.isTv ? 6 : 0,
              right,
              d.pageInsets.bottom + 24,
            ),
            sliver: SliverLayoutBuilder(
              builder: (context, c) {
                _itemConstraints = c;
                return _list
                    ? _listSliver(d, paged, total)
                    : _gridSliver(d, paged, total);
              },
            ),
          ),
      ],
    );

    if (!showIndex) return scroll;
    return Stack(
      children: [
        Positioned.fill(child: scroll),
        Positioned(
          right: 4,
          top: d.safeTop(context) + 56,
          bottom: d.pageInsets.bottom + 16,
          child: ExcludeFocus(
            child: AlphabetScrubber(
              key: const ValueKey('alphabet-index'),
              entries: _alphabet,
              semanticLabel: l.libraryAlphabetIndex,
              onSelected: (e) => _jumpTo(e.offset),
            ),
          ),
        ),
      ],
    );
  }

  Widget _gridSliver(KDensity d, PagedItems paged, int total) => SliverGrid(
    gridDelegate: _gridDelegate(d),
    delegate: SliverChildBuilderDelegate((context, i) {
      final item = paged.itemAt(i);
      if (item == null) return const _LoadingPoster();
      final badge = _badges[item.id];
      return LayoutBuilder(
        builder: (context, c) => PosterCard(
          key: ValueKey(item.id),
          item: item,
          width: c.maxWidth,
          onTap: () => openItem(context, item),
          onContextMenu: (p) => _openMenu(item, p),
          overlay: _PosterBadges(
            item: item,
            badge: badge,
            isNew: _isNew(item, badge),
          ),
        ),
      );
    }, childCount: total),
  );

  Widget _listSliver(KDensity d, PagedItems paged, int total) =>
      SliverFixedExtentList(
        itemExtent: _rowExtent(d),
        delegate: SliverChildBuilderDelegate((context, i) {
          final item = paged.itemAt(i);
          if (item == null) return const SizedBox.shrink();
          final badge = _badges[item.id];
          return _ListRow(
            key: ValueKey(item.id),
            item: item,
            badge: badge,
            isNew: _isNew(item, badge),
            onTap: () => openItem(context, item),
            onContextMenu: (p) => _openMenu(item, p),
          );
        }, childCount: total),
      );
}

/// Sort, grid/list switch and filter chips.
class _Toolbar extends StatelessWidget {
  const _Toolbar({
    required this.sort,
    required this.onSort,
    required this.list,
    required this.onList,
    required this.filters,
    required this.filter,
    required this.facets,
    required this.onFilter,
  });

  final LibrarySort sort;
  final ValueChanged<LibrarySort> onSort;
  final bool list;
  final ValueChanged<bool> onList;
  final bool filters;
  final LibraryFilter filter;
  final LibraryFacets? facets;
  final ValueChanged<LibraryFilter> onFilter;

  static Set<T> _toggle<T>(Set<T> s, T v) =>
      s.contains(v) ? ({...s}..remove(v)) : {...s, v};

  LibraryFilter _copy({
    List<String>? genres,
    (int?, int?)? years,
    Set<WatchState>? watched,
    Set<VideoResolution>? resolutions,
    Set<HdrFilter>? hdr,
  }) => LibraryFilter(
    genres: genres ?? filter.genres,
    yearMin: years == null ? filter.yearMin : years.$1,
    yearMax: years == null ? filter.yearMax : years.$2,
    watched: watched ?? filter.watched,
    resolutions: resolutions ?? filter.resolutions,
    hdr: hdr ?? filter.hdr,
    audioFormats: filter.audioFormats,
    audioLanguages: filter.audioLanguages,
    sources: filter.sources,
  );

  @override
  Widget build(BuildContext context) {
    final l = context.l10n;
    final f = facets;
    final decades = <int>[
      if (f != null && f.yearMin != null && f.yearMax != null)
        for (var y = f.yearMax! ~/ 10 * 10; y >= f.yearMin! ~/ 10 * 10; y -= 10)
          y,
    ];
    final chips = <Widget>[
      if (!filter.isEmpty)
        FilterPill(
          key: const ValueKey('filter-clear'),
          icon: Icons.close,
          label: l.libraryFilterClear,
          selected: false,
          onTap: () => onFilter(const LibraryFilter()),
        ),
      for (final (s, label) in [
        (WatchState.unwatched, l.libraryFilterUnwatched),
        (WatchState.inProgress, l.libraryFilterInProgress),
        (WatchState.watched, l.libraryFilterWatched),
      ])
        FilterPill(
          key: ValueKey('filter-watch-${s.name}'),
          label: label,
          selected: filter.watched.contains(s),
          onTap: () => onFilter(_copy(watched: _toggle(filter.watched, s))),
        ),
      for (final y in decades)
        FilterPill(
          key: ValueKey('filter-decade-$y'),
          label: l.libraryDecade(y),
          selected: filter.yearMin == y && filter.yearMax == y + 9,
          onTap: () => onFilter(
            _copy(
              years: filter.yearMin == y && filter.yearMax == y + 9
                  ? (null, null)
                  : (y, y + 9),
            ),
          ),
        ),
      for (final r in f?.resolutions ?? const <FacetCount<VideoResolution>>[])
        FilterPill(
          key: ValueKey('filter-res-${r.value.name}'),
          label: _resolutionLabel(r.value),
          selected: filter.resolutions.contains(r.value),
          onTap: () => onFilter(
            _copy(resolutions: _toggle(filter.resolutions, r.value)),
          ),
        ),
      for (final h in f?.hdr ?? const <FacetCount<HdrFilter>>[])
        FilterPill(
          key: ValueKey('filter-hdr-${h.value.name}'),
          label: _hdrLabel(h.value),
          selected: filter.hdr.contains(h.value),
          onTap: () => onFilter(_copy(hdr: _toggle(filter.hdr, h.value))),
        ),
      for (final g in f?.genres ?? const <FacetCount<String>>[])
        FilterPill(
          key: ValueKey('filter-genre-${g.value}'),
          label: g.value,
          selected: filter.genres.contains(g.value),
          onTap: () => onFilter(
            _copy(
              genres: filter.genres.contains(g.value)
                  ? [
                      for (final x in filter.genres)
                        if (x != g.value) x,
                    ]
                  : [...filter.genres, g.value],
            ),
          ),
        ),
    ];
    return FocusTraversalGroup(
      child: SingleChildScrollView(
        scrollDirection: Axis.horizontal,
        clipBehavior: Clip.none,
        child: Row(
          children: [
            _IconPill(
              key: const ValueKey('layout-grid'),
              icon: Icons.grid_view_rounded,
              label: l.libraryViewGrid,
              selected: !list,
              onTap: () => onList(false),
            ),
            const SizedBox(width: 4),
            _IconPill(
              key: const ValueKey('layout-list'),
              icon: Icons.view_list_rounded,
              label: l.libraryViewList,
              selected: list,
              onTap: () => onList(true),
            ),
            const SizedBox(width: 12),
            _SortButton(value: sort, onChanged: onSort),
            if (filters && chips.isNotEmpty) ...[
              const SizedBox(width: 16),
              for (final c in chips) ...[c, const SizedBox(width: 8)],
            ],
          ],
        ),
      ),
    );
  }
}

/// Round icon toggle (grid / list) with a tooltip.
class _IconPill extends StatelessWidget {
  const _IconPill({
    super.key,
    required this.icon,
    required this.label,
    required this.selected,
    required this.onTap,
  });

  final IconData icon;
  final String label;
  final bool selected;
  final VoidCallback onTap;

  @override
  Widget build(BuildContext context) {
    final d = KDensity.of(context);
    return Tooltip(
      message: label,
      child: KFocusable(
        onActivate: onTap,
        radius: 999,
        light: false,
        focusScale: d.focusScaleButton,
        semanticLabel: label,
        builder: (context, focused) => AnimatedContainer(
          duration: KMotion.focus,
          padding: EdgeInsets.all(d.isTv ? 6 : 7),
          decoration: BoxDecoration(
            color: focused
                ? Colors.white
                : (selected
                      ? Colors.white.withValues(alpha: .2)
                      : Colors.white.withValues(alpha: .06)),
            shape: BoxShape.circle,
          ),
          child: Icon(
            icon,
            size: d.type.caption + 6,
            color: focused
                ? Colors.black
                : (selected ? Colors.white : KColors.textSecondary),
          ),
        ),
      ),
    );
  }
}

/// Opens the sort menu; picking the current field flips the direction.
class _SortButton extends StatelessWidget {
  const _SortButton({required this.value, required this.onChanged});

  final LibrarySort value;
  final ValueChanged<LibrarySort> onChanged;

  Future<void> _open(BuildContext context) async {
    final l = context.l10n;
    final box = context.findRenderObject()! as RenderBox;
    final overlay =
        Overlay.of(context).context.findRenderObject()! as RenderBox;
    final rect = Rect.fromPoints(
      box.localToGlobal(box.size.bottomLeft(Offset.zero), ancestor: overlay),
      box.localToGlobal(box.size.bottomRight(Offset.zero), ancestor: overlay),
    );
    final field = await showMenu<SortField>(
      context: context,
      position: RelativeRect.fromRect(rect, Offset.zero & overlay.size),
      items: [
        for (final f in _sortDefaults.keys)
          PopupMenuItem(
            key: ValueKey('sort-${f.name}'),
            value: f,
            child: Row(
              children: [
                SizedBox(
                  width: 28,
                  child: f == value.field
                      ? Icon(
                          value.descending
                              ? Icons.arrow_downward
                              : Icons.arrow_upward,
                          size: 18,
                        )
                      : null,
                ),
                Text(_sortLabel(l, f)),
              ],
            ),
          ),
      ],
    );
    if (field == null) return;
    onChanged(
      field == value.field
          ? LibrarySort(field: field, descending: !value.descending)
          : LibrarySort(field: field, descending: _sortDefaults[field]!),
    );
  }

  @override
  Widget build(BuildContext context) {
    final l = context.l10n;
    return Builder(
      builder: (context) => FilterPill(
        key: const ValueKey('sort-button'),
        icon: value.descending ? Icons.arrow_downward : Icons.arrow_upward,
        label: l.librarySort(_sortLabel(l, value.field)),
        semanticLabel: l.librarySort(
          '${_sortLabel(l, value.field)}, '
          '${value.descending ? l.librarySortDescending : l.librarySortAscending}',
        ),
        selected: true,
        onTap: () => _open(context),
      ),
    );
  }
}

/// Watched mark, progress bar and "new" label over a poster.
class _PosterBadges extends StatelessWidget {
  const _PosterBadges({required this.item, this.badge, this.isNew = false});

  final LibraryItem item;
  final ItemBadge? badge;
  final bool isNew;

  @override
  Widget build(BuildContext context) {
    final b = badge;
    final progress = b?.progress;
    return IgnorePointer(
      child: Stack(
        children: [
          if (isNew)
            Positioned(
              left: 6,
              top: 6,
              child: _NewLabel(key: ValueKey('badge-new-${item.id}')),
            ),
          if (b != null && progress == null)
            Positioned(
              right: 6,
              top: 6,
              child: _StateMark(
                key: ValueKey('badge-${b.state.name}-${item.id}'),
                state: b.state,
              ),
            ),
          if (b != null && progress != null)
            Positioned(
              left: 8,
              right: 8,
              bottom: 8,
              child: KProgressBar(
                key: ValueKey('badge-progress-${item.id}'),
                value: progress,
              ),
            ),
        ],
      ),
    );
  }
}

class _NewLabel extends StatelessWidget {
  const _NewLabel({super.key});

  @override
  Widget build(BuildContext context) => Container(
    padding: const EdgeInsets.symmetric(horizontal: 6, vertical: 2),
    decoration: BoxDecoration(
      color: KColors.accent,
      borderRadius: BorderRadius.circular(4),
    ),
    child: Text(
      context.l10n.libraryBadgeNew,
      style: Theme.of(context).textTheme.labelSmall
          ?.copyWith(color: Colors.white, fontWeight: FontWeight.w700),
    ),
  );
}

class _StateMark extends StatelessWidget {
  const _StateMark({super.key, required this.state});

  final WatchState state;

  @override
  Widget build(BuildContext context) {
    final l = context.l10n;
    final watched = state == WatchState.watched;
    return Semantics(
      label: watched ? l.libraryFilterWatched : l.libraryFilterInProgress,
      child: Container(
        width: 22,
        height: 22,
        decoration: BoxDecoration(
          color: watched ? KColors.accent : Colors.black.withValues(alpha: .6),
          shape: BoxShape.circle,
        ),
        child: Icon(
          watched ? Icons.check_rounded : Icons.timelapse_rounded,
          size: 15,
          color: Colors.white,
        ),
      ),
    );
  }
}

/// A poster whose page is still loading.
class _LoadingPoster extends StatelessWidget {
  const _LoadingPoster();

  @override
  Widget build(BuildContext context) {
    final d = KDensity.of(context);
    return Align(
      alignment: Alignment.topCenter,
      child: AspectRatio(
        aspectRatio: 2 / 3,
        child: DecoratedBox(
          decoration: BoxDecoration(
            color: KColors.bgElevated,
            borderRadius: BorderRadius.circular(d.cardRadius),
          ),
        ),
      ),
    );
  }
}

/// A row of the list mode: small poster, title, year and runtime, badges.
class _ListRow extends StatelessWidget {
  const _ListRow({
    super.key,
    required this.item,
    required this.badge,
    required this.isNew,
    required this.onTap,
    required this.onContextMenu,
  });

  final LibraryItem item;
  final ItemBadge? badge;
  final bool isNew;
  final VoidCallback onTap;
  final ValueChanged<Offset> onContextMenu;

  @override
  Widget build(BuildContext context) {
    final d = KDensity.of(context);
    final l = context.l10n;
    final theme = Theme.of(context);
    final b = badge;
    final meta = [
      if (item.year != null) '${item.year}',
      formatRuntime(l, item.durationMs),
    ].where((s) => s.isNotEmpty).join('  ·  ');
    return Padding(
      padding: const EdgeInsets.symmetric(vertical: 4),
      child: KFocusable(
        onActivate: onTap,
        onContextMenu: onContextMenu,
        focusScale: d.focusScaleButton,
        light: false,
        semanticLabel: item.title,
        builder: (context, focused) => Container(
          color: focused ? Colors.white.withValues(alpha: .08) : null,
          padding: const EdgeInsets.all(4),
          child: Row(
            children: [
              AspectRatio(
                aspectRatio: 2 / 3,
                child: ClipRRect(
                  borderRadius: BorderRadius.circular(4),
                  child: Artwork(
                    url: item.posterUrl,
                    seed: item.id,
                    style: ArtStyle.poster,
                    title: item.title,
                  ),
                ),
              ),
              const SizedBox(width: 14),
              Expanded(
                child: Column(
                  mainAxisAlignment: MainAxisAlignment.center,
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    Text(
                      item.title,
                      maxLines: 1,
                      overflow: TextOverflow.ellipsis,
                      style: theme.textTheme.titleSmall,
                    ),
                    if (meta.isNotEmpty)
                      Text(
                        meta,
                        maxLines: 1,
                        overflow: TextOverflow.ellipsis,
                        style: theme.textTheme.labelSmall,
                      ),
                    if (b?.progress case final p?) ...[
                      const SizedBox(height: 6),
                      SizedBox(
                        width: 120,
                        child: KProgressBar(
                          key: ValueKey('badge-progress-${item.id}'),
                          value: p,
                          height: 3,
                        ),
                      ),
                    ],
                  ],
                ),
              ),
              if (isNew) ...[
                const SizedBox(width: 8),
                _NewLabel(key: ValueKey('badge-new-${item.id}')),
              ],
              if (b != null && b.progress == null) ...[
                const SizedBox(width: 8),
                _StateMark(
                  key: ValueKey('badge-${b.state.name}-${item.id}'),
                  state: b.state,
                ),
              ],
              const SizedBox(width: 8),
            ],
          ),
        ),
      ),
    );
  }
}

/// Movies / TV Shows switch under the phone Library tab.
class _LibraryKindSwitch extends StatelessWidget {
  const _LibraryKindSwitch({required this.movies});

  final bool movies;

  @override
  Widget build(BuildContext context) {
    final l = context.l10n;
    final shell = context.findAncestorStateOfType<AppShellState>();
    return FocusTraversalGroup(
      child: Row(
        children: [
          FilterPill(
            icon: Icons.movie_outlined,
            label: l.navMovies,
            selected: movies,
            onTap: () => shell?.select(Section.movies),
          ),
          const SizedBox(width: 8),
          FilterPill(
            icon: Icons.tv_outlined,
            label: l.navShows,
            selected: !movies,
            onTap: () => shell?.select(Section.shows),
          ),
        ],
      ),
    );
  }
}

class FilterPill extends StatelessWidget {
  const FilterPill({
    super.key,
    required this.label,
    required this.selected,
    required this.onTap,
    this.icon,
    this.semanticLabel,
  });

  final String label;
  final bool selected;
  final VoidCallback onTap;
  final IconData? icon;

  /// Read by screen readers instead of [label].
  final String? semanticLabel;

  @override
  Widget build(BuildContext context) {
    final d = KDensity.of(context);
    return KFocusable(
      onActivate: onTap,
      radius: 999,
      light: false,
      focusScale: d.focusScaleButton,
      semanticLabel: semanticLabel ?? label,
      builder: (context, focused) => AnimatedContainer(
        duration: KMotion.focus,
        padding: EdgeInsets.symmetric(horizontal: 14, vertical: d.isTv ? 6 : 7),
        decoration: BoxDecoration(
          color: focused
              ? Colors.white
              : (selected
                    ? Colors.white.withValues(alpha: .2)
                    : Colors.white.withValues(alpha: .06)),
          borderRadius: BorderRadius.circular(999),
        ),
        child: Row(
          mainAxisSize: MainAxisSize.min,
          children: [
            if (icon != null) ...[
              Icon(
                icon,
                size: d.type.caption + 4,
                color: focused ? Colors.black : Colors.white,
              ),
              const SizedBox(width: 6),
            ],
            Text(
              label,
              style: Theme.of(context).textTheme.labelLarge?.copyWith(
                fontSize: d.type.caption + 1,
                color: focused
                    ? Colors.black
                    : (selected ? Colors.white : KColors.textSecondary),
              ),
            ),
          ],
        ),
      ),
    );
  }
}
