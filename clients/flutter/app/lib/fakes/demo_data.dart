// SPDX-License-Identifier: MPL-2.0
//
// Demo library: fictional, project-original titles and descriptions. Artwork is
// generated in code (lib/widgets/artwork.dart), so no third-party assets.

import '../integration/library.dart';
import '../widgets/artwork.dart' show kDemoArtScheme;

class DemoLibraryData {
  DemoLibraryData({
    required this.movies,
    required this.shows,
    required this.episodes,
    required this.tracks,
    required this.progress,
  });

  final List<LibraryItem> movies;
  final List<LibraryItem> shows;
  final List<LibraryItem> episodes;
  final List<LibraryItem> tracks;
  final List<Progress> progress;
}

const _movies = <(String, int, int, String)>[
  (
    'The Quiet Orbit',
    2024,
    118,
    'A lone engineer aboard a failing relay station starts receiving messages from the crew that left her behind — dated ten years in the future.',
  ),
  (
    'Harbor Lights',
    2023,
    104,
    'Two rival ferry captains are forced to share one boat for a winter of storms, smugglers and surprisingly good soup.',
  ),
  (
    'Northbound',
    2022,
    126,
    'A night train across the tundra, a stolen violin and a conductor who never seems to sleep.',
  ),
  (
    'Paper Lanterns',
    2021,
    97,
    'On the last night of the festival, a shy cartographer maps the city by the lanterns its people release.',
  ),
  (
    'The Last Lighthouse',
    2020,
    112,
    'An automated lighthouse wakes up after a century and decides the coast still needs a keeper.',
  ),
  (
    'Glass Horizon',
    2024,
    131,
    'A desert architect builds a city of mirrors and discovers that some reflections refuse to leave.',
  ),
  (
    'Midnight Cartographer',
    2019,
    109,
    'Every map she draws comes true by morning. Her newest commission is a map of someone else\'s past.',
  ),
  (
    'Salt & Ember',
    2023,
    101,
    'A chef and a fire-lookout trade letters across a valley that is slowly catching fire.',
  ),
  (
    'Low Tide',
    2018,
    94,
    'Three siblings return to their mother\'s island and find the sea has given back what it took.',
  ),
  (
    'Copper Sky',
    2025,
    122,
    'When the sky over a mining town turns copper, a geologist has three days to explain why.',
  ),
  (
    'Atlas of Small Things',
    2021,
    88,
    'A documentary-style comedy about a museum devoted entirely to objects smaller than a coin.',
  ),
  (
    'The Long Weekend',
    2022,
    99,
    'A family reunion in a house with too many clocks, all of them slightly wrong.',
  ),
  (
    'Kites over Kestrel Bay',
    2020,
    106,
    'An eleven-year-old enters the national kite championship with a kite made of her grandfather\'s maps.',
  ),
  (
    'Static Bloom',
    2025,
    115,
    'A radio host discovers that the dead air between stations is full of flowers — and they are listening.',
  ),
];

const _shows = <(String, int, int, int, String)>[
  // title, year, seasons, episodes per season, overview
  (
    'Moonlit Bureau',
    2022,
    3,
    8,
    'A night-shift office that handles complaints about the Moon. Nobody knows who files them.',
  ),
  (
    'Coastline',
    2021,
    2,
    6,
    'Lifeguards, surfers and one very stubborn seal share a stretch of northern beach.',
  ),
  (
    'The Archive',
    2023,
    2,
    10,
    'Librarians of a vast underground archive catalogue stories that have not been written yet.',
  ),
  (
    'Signal Fires',
    2024,
    1,
    8,
    'Mountain villages relight an ancient chain of beacons when the network goes dark.',
  ),
  (
    'Orchard Street',
    2019,
    4,
    6,
    'A gentle comedy about the people who keep a city orchard alive through four seasons.',
  ),
];

const _episodeTitles = <String>[
  'Pilot',
  'The Night Desk',
  'Lunar Paperwork',
  'Low Orbit',
  'The Visitor',
  'Half Light',
  'Small Hours',
  'Crossing',
  'Paper Trails',
  'Undertow',
  'Second Sunrise',
  'The Long Way Round',
];

const _albums = <(String, String, int, List<String>)>[
  (
    'Nocturne Avenue',
    'Velvet Static',
    2023,
    [
      'Streetlamp Waltz',
      'After Hours',
      'Neon Rain',
      'Slow Taxi',
      'Moonwalk Blues',
      'Last Call',
      'Dawn Bus',
    ],
  ),
  (
    'Analog Dawn',
    'The Lumen Set',
    2021,
    [
      'Warm Tubes',
      'Tape Hiss Sunrise',
      'Polaroid',
      'Signal Path',
      'Overdub',
      'Low Pass',
    ],
  ),
  (
    'Tides & Tunnels',
    'Mara Quill',
    2024,
    ['Undertow', 'Lantern', 'Saltwater Choir', 'Tunnel Vision', 'Harbor Song'],
  ),
  (
    'Paper Planets',
    'Orbit Kids',
    2022,
    [
      'Countdown',
      'Zero G',
      'Paper Planets',
      'Mission Control',
      'Re-entry',
      'Splashdown',
    ],
  ),
  (
    'Quiet Machines',
    'Hollow Pines',
    2020,
    ['Idle', 'Fan Noise', 'Standby', 'Wake on LAN', 'Shutdown Sequence'],
  ),
];

DemoLibraryData buildDemoLibraryData({DateTime? now}) {
  final t0 = now ?? DateTime(2026, 11, 13, 20);
  final movies = <LibraryItem>[];
  for (var i = 0; i < _movies.length; i++) {
    final (title, year, minutes, overview) = _movies[i];
    final slug = _slug(title);
    movies.add(
      LibraryItem(
        id: 'movie-$slug',
        kind: ItemKind.movie,
        title: title,
        year: year,
        overview: overview,
        filePath: '/media/Movies/$title ($year)/$title.$year.1080p.mkv',
        durationMs: minutes * 60000,
        posterUrl: '${kDemoArtScheme}poster',
        backdropUrl: '${kDemoArtScheme}backdrop',
        matchConfidence: i == 6 ? 0.62 : 0.9 + (i % 3) * 0.03,
      ),
    );
  }

  final shows = <LibraryItem>[];
  final episodes = <LibraryItem>[];
  for (var i = 0; i < _shows.length; i++) {
    final (title, year, seasons, perSeason, overview) = _shows[i];
    final showId = 'show-${_slug(title)}';
    shows.add(
      LibraryItem(
        id: showId,
        kind: ItemKind.show,
        title: title,
        year: year,
        overview: overview,
        filePath: '/media/Shows/$title',
        posterUrl: '${kDemoArtScheme}poster',
        backdropUrl: '${kDemoArtScheme}backdrop',
        matchConfidence: 0.95,
      ),
    );
    for (var s = 1; s <= seasons; s++) {
      for (var e = 1; e <= perSeason; e++) {
        final epTitle = _episodeTitles[(e - 1 + s + i) % _episodeTitles.length];
        episodes.add(
          LibraryItem(
            id: '$showId-s${s}e$e',
            kind: ItemKind.episode,
            title: epTitle,
            showId: showId,
            season: s,
            episode: e,
            year: year + s - 1,
            overview:
                'Episode $e of season $s of $title. ${e.isOdd ? 'A quiet case turns out to be anything but.' : 'Old friends return with new problems.'}',
            filePath:
                '/media/Shows/$title/Season ${s.toString().padLeft(2, '0')}/'
                '$title - S${s.toString().padLeft(2, '0')}E${e.toString().padLeft(2, '0')}.mkv',
            durationMs: (38 + (e * 7 + s) % 17) * 60000,
            backdropUrl: '${kDemoArtScheme}still',
            matchConfidence: 0.97,
          ),
        );
      }
    }
  }

  final tracks = <LibraryItem>[];
  for (final (album, artist, year, names) in _albums) {
    for (var n = 0; n < names.length; n++) {
      tracks.add(
        LibraryItem(
          id: 'track-${_slug(album)}-${n + 1}',
          kind: ItemKind.track,
          title: names[n],
          artist: artist,
          album: album,
          year: year,
          trackNumber: n + 1,
          posterUrl: '${kDemoArtScheme}album',
          filePath:
              '/media/Music/$artist/$album/${(n + 1).toString().padLeft(2, '0')} ${names[n]}.flac',
          durationMs: (150 + (n * 37 + album.length * 11) % 130) * 1000,
        ),
      );
    }
  }

  Progress p(String id, double fraction, int durationMs, int hoursAgo) =>
      Progress(
        itemId: id,
        positionMs: (durationMs * fraction).round(),
        durationMs: durationMs,
        updatedAt: t0.subtract(Duration(hours: hoursAgo)),
      );
  final byId = {
    for (final x in [...movies, ...episodes]) x.id: x,
  };
  int dur(String id) => byId[id]!.durationMs!;
  final progress = <Progress>[
    p('movie-the-quiet-orbit', .42, dur('movie-the-quiet-orbit'), 1),
    p('show-moonlit-bureau-s2e4', .67, dur('show-moonlit-bureau-s2e4'), 3),
    p('movie-northbound', .18, dur('movie-northbound'), 26),
    p('show-the-archive-s1e7', .81, dur('show-the-archive-s1e7'), 30),
    p('movie-salt-ember', .55, dur('movie-salt-ember'), 52),
    p('show-coastline-s1e2', .30, dur('show-coastline-s1e2'), 80),
    // Finished episodes: Home's Up Next offers the following ones (UI-007).
    for (final (id, hoursAgo) in const [
      ('show-signal-fires-s1e3', 5),
      ('show-orchard-street-s2e6', 40),
    ])
      Progress(
        itemId: id,
        positionMs: dur(id),
        durationMs: dur(id),
        updatedAt: t0.subtract(Duration(hours: hoursAgo)),
        watched: true,
      ),
  ];

  return DemoLibraryData(
    movies: movies,
    shows: shows,
    episodes: episodes,
    tracks: tracks,
    progress: progress,
  );
}

String _slug(String s) => s
    .toLowerCase()
    .replaceAll(RegExp(r'[^a-z0-9]+'), '-')
    .replaceAll(RegExp(r'^-|-$'), '');
