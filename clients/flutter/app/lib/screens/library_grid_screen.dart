// SPDX-License-Identifier: MPL-2.0

import 'package:material_ui/material_ui.dart';

import '../integration/library.dart';
import '../l10n/l10n.dart';
import '../services/app_services.dart';
import '../shell/app_shell.dart';
import '../shell/breakpoints.dart';
import '../theme/kinolune_theme.dart';
import '../widgets/cards.dart';
import '../widgets/focusable.dart';
import 'routes.dart';

enum SortOrder { added, title, year }

extension on SortOrder {
  String label(AppLocalizations l) => switch (this) {
    SortOrder.added => l.librarySortAdded,
    SortOrder.title => l.librarySortTitle,
    SortOrder.year => l.librarySortYear,
  };
}

/// Grid filter: everything, titles in progress, or one decade (e.g. 1990).
const _filterAll = -1, _filterInProgress = -2;

/// Poster grid for Movies and Shows with sorting and filters.
class LibraryGridScreen extends StatefulWidget {
  const LibraryGridScreen({super.key, required this.kind});

  final ItemKind kind; // movie or show

  @override
  State<LibraryGridScreen> createState() => _LibraryGridScreenState();
}

class _LibraryGridScreenState extends State<LibraryGridScreen> {
  Stream<List<LibraryItem>>? _items;
  Stream<List<(LibraryItem, Progress)>>? _cw;
  SortOrder _sort = SortOrder.added;
  int _filter = _filterAll;

  @override
  void didChangeDependencies() {
    super.didChangeDependencies();
    final lib = Services.of(context).library;
    _items ??= widget.kind == ItemKind.movie ? lib.movies() : lib.shows();
    _cw ??= lib.continueWatching();
  }

  @override
  Widget build(BuildContext context) {
    final d = KDensity.of(context);
    final l = context.l10n;
    final movies = widget.kind == ItemKind.movie;
    // The phone tab bar has one Library tab for Movies and TV Shows (UI-002);
    // the switch between them lives in the page header.
    final libraryTab = NavLayout.of(context) == NavLayout.tabs;
    String filterLabel(int f) => switch (f) {
      _filterAll => l.libraryFilterAll,
      _filterInProgress => l.libraryFilterInProgress,
      _ => l.libraryDecade(f),
    };
    return StreamBuilder<List<LibraryItem>>(
      stream: _items,
      builder: (context, snap) => StreamBuilder<List<(LibraryItem, Progress)>>(
        stream: _cw,
        builder: (context, cwSnap) {
          final all = snap.data ?? const <LibraryItem>[];
          final inProgress = <String>{
            for (final (i, _)
                in cwSnap.data ?? const <(LibraryItem, Progress)>[])
              i.showId ?? i.id,
          };
          final decades = {
            for (final i in all)
              if (i.year != null) i.year! ~/ 10 * 10,
          }.toList()..sort((a, b) => b - a);
          final filters = [_filterAll, _filterInProgress, ...decades];
          var items = all.where((i) {
            if (_filter == _filterAll) return true;
            if (_filter == _filterInProgress) return inProgress.contains(i.id);
            return i.year != null && i.year! ~/ 10 * 10 == _filter;
          }).toList();
          switch (_sort) {
            case SortOrder.added:
              break;
            case SortOrder.title:
              items.sort(
                (a, b) =>
                    a.title.toLowerCase().compareTo(b.title.toLowerCase()),
              );
            case SortOrder.year:
              items.sort((a, b) => (b.year ?? 0).compareTo(a.year ?? 0));
          }
          items = items.toList();

          return CustomScrollView(
            clipBehavior: Clip.none,
            slivers: [
              SliverToBoxAdapter(
                child: PageTitle(
                  libraryTab ? l.navLibrary : (movies ? l.navMovies : l.navShows),
                  trailing: Text(
                    movies
                        ? l.libraryMovieCount(all.length)
                        : l.libraryShowCount(all.length),
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
                  child: FocusTraversalGroup(
                    child: SingleChildScrollView(
                      scrollDirection: Axis.horizontal,
                      clipBehavior: Clip.none,
                      child: Row(
                        children: [
                          _SortButton(
                            value: _sort,
                            onChanged: (s) => setState(() => _sort = s),
                          ),
                          const SizedBox(width: 16),
                          for (final f in filters) ...[
                            FilterPill(
                              label: filterLabel(f),
                              selected: f == _filter,
                              onTap: () => setState(() => _filter = f),
                            ),
                            const SizedBox(width: 8),
                          ],
                        ],
                      ),
                    ),
                  ),
                ),
              ),
              if (items.isEmpty)
                SliverFillRemaining(
                  hasScrollBody: false,
                  child: EmptyState(
                    icon: widget.kind == ItemKind.movie
                        ? Icons.movie_outlined
                        : Icons.tv_outlined,
                    title: all.isEmpty
                        ? (movies ? l.libraryNoMovies : l.libraryNoShows)
                        : l.libraryNoMatches,
                    message: all.isEmpty ? l.libraryEmptyHint : null,
                  ),
                )
              else
                SliverPadding(
                  padding: EdgeInsets.fromLTRB(
                    d.pageInsets.left,
                    d.isTv ? 6 : 0,
                    d.pageInsets.right,
                    d.pageInsets.bottom + 24,
                  ),
                  sliver: SliverGrid(
                    gridDelegate: CardGridDelegate(
                      minItemWidth: d.posterWidth,
                      mainSpacing: d.isTv ? 22 : 18,
                      crossSpacing: d.gap,
                      artAspect: 1.5,
                      extraHeight: cardCaptionHeight(d),
                    ),
                    delegate: SliverChildBuilderDelegate(
                      (context, i) => LayoutBuilder(
                        builder: (context, c) => PosterCard(
                          item: items[i],
                          width: c.maxWidth,
                          onTap: () => openItem(context, items[i]),
                        ),
                      ),
                      childCount: items.length,
                    ),
                  ),
                ),
            ],
          );
        },
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
  });

  final String label;
  final bool selected;
  final VoidCallback onTap;
  final IconData? icon;

  @override
  Widget build(BuildContext context) {
    final d = KDensity.of(context);
    return KFocusable(
      onActivate: onTap,
      radius: 999,
      light: false,
      focusScale: d.focusScaleButton,
      semanticLabel: label,
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

class _SortButton extends StatelessWidget {
  const _SortButton({required this.value, required this.onChanged});

  final SortOrder value;
  final ValueChanged<SortOrder> onChanged;

  @override
  Widget build(BuildContext context) {
    return FilterPill(
      icon: Icons.sort,
      label: context.l10n.librarySort(value.label(context.l10n)),
      selected: true,
      onTap: () => onChanged(
        SortOrder.values[(value.index + 1) % SortOrder.values.length],
      ),
    );
  }
}
