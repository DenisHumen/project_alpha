// SPDX-License-Identifier: MPL-2.0

import 'dart:async';

import 'package:material_ui/material_ui.dart';

import '../integration/library.dart';
import '../services/app_services.dart';
import '../theme/kinolune_theme.dart';
import '../widgets/cards.dart';
import 'music_screen.dart' show TrackTile;
import 'playback.dart';
import 'routes.dart';

class SearchScreen extends StatefulWidget {
  const SearchScreen({super.key, this.initialQuery = ''});

  final String initialQuery;

  @override
  State<SearchScreen> createState() => _SearchScreenState();
}

class _SearchScreenState extends State<SearchScreen> {
  late final _query = TextEditingController(text: widget.initialQuery);
  final List<StreamSubscription<Object?>> _subs = [];
  List<LibraryItem> _movies = const [], _shows = const [], _tracks = const [];

  @override
  void didChangeDependencies() {
    super.didChangeDependencies();
    if (_subs.isNotEmpty) return;
    final lib = Services.of(context).library;
    _subs
      ..add(lib.movies().listen((v) => setState(() => _movies = v)))
      ..add(lib.shows().listen((v) => setState(() => _shows = v)))
      ..add(lib.tracks().listen((v) => setState(() => _tracks = v)));
  }

  @override
  void dispose() {
    for (final s in _subs) {
      s.cancel();
    }
    _query.dispose();
    super.dispose();
  }

  bool _match(LibraryItem i, String q) =>
      i.title.toLowerCase().contains(q) ||
      (i.originalTitle?.toLowerCase().contains(q) ?? false) ||
      (i.artist?.toLowerCase().contains(q) ?? false) ||
      (i.album?.toLowerCase().contains(q) ?? false) ||
      (i.year?.toString() == q);

  @override
  Widget build(BuildContext context) {
    final d = KDensity.of(context);
    final q = _query.text.trim().toLowerCase();
    final movies = q.isEmpty ? const <LibraryItem>[] : _movies.where((i) => _match(i, q)).toList();
    final shows = q.isEmpty ? const <LibraryItem>[] : _shows.where((i) => _match(i, q)).toList();
    final tracks = q.isEmpty ? const <LibraryItem>[] : _tracks.where((i) => _match(i, q)).toList();
    final posterRowH = d.posterWidth * 1.5 + (d.isTv ? 52 : 46);

    return ListView(
      clipBehavior: Clip.none,
      padding: EdgeInsets.only(bottom: d.pageInsets.bottom + 90),
      children: [
        const PageTitle('Search'),
        Padding(
          padding: EdgeInsets.fromLTRB(d.pageInsets.left, 0, d.pageInsets.right, 24),
          child: ConstrainedBox(
            constraints: const BoxConstraints(maxWidth: 640),
            child: TextField(
              controller: _query,
              autofocus: d.isDesktop,
              onChanged: (_) => setState(() {}),
              textInputAction: TextInputAction.search,
              style: Theme.of(context).textTheme.bodyLarge,
              decoration: InputDecoration(
                prefixIcon: const Icon(Icons.search),
                hintText: 'Movies, shows, artists, albums, tracks…',
                suffixIcon: q.isEmpty
                    ? null
                    : IconButton(
                        tooltip: 'Clear',
                        onPressed: () => setState(_query.clear),
                        icon: const Icon(Icons.close),
                      ),
              ),
            ),
          ),
        ),
        if (q.isEmpty)
          const SizedBox(height: 260, child: EmptyState(icon: Icons.search, title: 'Search your library', message: 'Type a title, a year, an artist or an album.'))
        else if (movies.isEmpty && shows.isEmpty && tracks.isEmpty)
          SizedBox(height: 260, child: EmptyState(icon: Icons.search_off, title: 'No results for “${_query.text.trim()}”'))
        else ...[
          if (movies.isNotEmpty) ...[
            MediaRow(
              title: 'Movies',
              itemCount: movies.length,
              height: posterRowH,
              itemBuilder: (context, i) => PosterCard(item: movies[i], width: d.posterWidth, onTap: () => openItem(context, movies[i])),
            ),
            SizedBox(height: d.rowGap),
          ],
          if (shows.isNotEmpty) ...[
            MediaRow(
              title: 'TV Shows',
              itemCount: shows.length,
              height: posterRowH,
              itemBuilder: (context, i) => PosterCard(item: shows[i], width: d.posterWidth, onTap: () => openItem(context, shows[i])),
            ),
            SizedBox(height: d.rowGap),
          ],
          if (tracks.isNotEmpty) ...[
            Padding(
              padding: EdgeInsets.fromLTRB(d.pageInsets.left, 0, d.pageInsets.right, 8),
              child: Text('Music', style: Theme.of(context).textTheme.titleLarge),
            ),
            Padding(
              padding: EdgeInsets.symmetric(horizontal: d.pageInsets.left),
              child: Column(children: [
                for (var i = 0; i < tracks.length; i++)
                  TrackTile(track: tracks[i], showArt: true, onTap: () => playTracks(context, tracks, i)),
              ]),
            ),
          ],
        ],
      ],
    );
  }
}
