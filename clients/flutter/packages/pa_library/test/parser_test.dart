// SPDX-License-Identifier: MPL-2.0
//
// Parser cases written for project_alpha (not copied from other projects).

import 'package:pa_library/src/parser/filename_parser.dart';
import 'package:test/test.dart';

/// (input, title, year)
const _movies = <(String, String, int?)>[
  ('The.Matrix.1999.1080p.BluRay.x264-GRP.mkv', 'The Matrix', 1999),
  ('Inception (2010).mkv', 'Inception', 2010),
  ('Inception.2010.720p.WEB-DL.mkv', 'Inception', 2010),
  ('Blade.Runner.2049.2017.2160p.UHD.BluRay.REMUX.HDR.HEVC.Atmos-XYZ.mkv', 'Blade Runner 2049', 2017),
  ('Blade Runner 2049 (2017).mkv', 'Blade Runner 2049', 2017),
  ('2001.A.Space.Odyssey.1968.BDRip.mkv', '2001 A Space Odyssey', 1968),
  ('1917.2019.1080p.mkv', '1917', 2019),
  ('2012.2009.BDRip.avi', '2012', 2009),
  ('1917.mkv', '1917', null),
  ('Alien.mkv', 'Alien', null),
  ('Mad_Max_Fury_Road_2015_1080p.mp4', 'Mad Max Fury Road', 2015),
  ('Spider-Man.No.Way.Home.2021.WEBRip.mkv', 'Spider-Man No Way Home', 2021),
  ('Mr. Nobody (2009) [1080p].mkv', 'Mr Nobody', 2009),
  ('The Lord of the Rings - The Fellowship of the Ring (2001) Extended Edition.mkv', 'The Lord of the Rings - The Fellowship of the Ring', 2001),
  ('Pulp Fiction 1994 Remastered 1080p.mkv', 'Pulp Fiction', 1994),
  ('Amélie.2001.DVDRip.XviD.avi', 'Amélie', 2001),
  ('Léon.The.Professional.1994.Directors.Cut.mkv', 'Léon The Professional', 1994),
  ('Interstellar.2014.IMAX.2160p.mkv', 'Interstellar', 2014),
  ('Dune.Part.Two.2024.1080p.AMZN.WEB-DL.DDP5.1.H.264-FLUX.mkv', 'Dune Part Two', 2024),
  ('Oppenheimer.2023.2160p.WEB-DL.DDP5.1.Atmos.DV.HDR.H.265.mkv', 'Oppenheimer', 2023),
  ('Up (2009).mp4', 'Up', 2009),
  ('It.2017.mkv', 'It', 2017),
  ('Se7en.1995.mkv', 'Se7en', 1995),
  ('WALL-E.2008.BluRay.mkv', 'WALL-E', 2008),
  ('Ocean\'s.Eleven.2001.mkv', 'Ocean\'s Eleven', 2001),
  ('Star Wars Episode IV - A New Hope (1977).mkv', 'Star Wars Episode IV - A New Hope', 1977),
  ('Kill Bill Vol. 1 (2003).mkv', 'Kill Bill Vol 1', 2003),
  ('The.Godfather.Part.II.1974.1080p.mkv', 'The Godfather Part II', 1974),
  ('Toy.Story.3.2010.mkv', 'Toy Story 3', 2010),
  ('Avatar.The.Way.of.Water.2022.HDR10.2160p.mkv', 'Avatar The Way of Water', 2022),
  ('Parasite.2019.KOREAN.1080p.mkv', 'Parasite', 2019),
  ('Gladiator.2000.Extended.Remastered.mkv', 'Gladiator', 2000),
  ('the_shawshank_redemption_1994.avi', 'the shawshank redemption', 1994),
  ('Joker.2019.HDRip.XviD.AC3-EVO.avi', 'Joker', 2019),
  ('Arrival (2016) (1080p BluRay x265).mkv', 'Arrival', 2016),
  ('[YTS] Heat 1995 720p.mp4', 'Heat', 1995),
  ('Heat.1995.REMASTERED.1080p.BluRay.10bit.x265-GRP.mkv', 'Heat', 1995),
  ('No.Country.for.Old.Men.2007.PROPER.mkv', 'No Country for Old Men', 2007),
  ('Everything Everywhere All at Once 2022.mkv', 'Everything Everywhere All at Once', 2022),
  ('Back to the Future (1985) 1080p BrRip x264.mp4', 'Back to the Future', 1985),
  ('Fight Club 1999 CD1.avi', 'Fight Club', 1999),
  ('Terminator.2.Judgment.Day.1991.mkv', 'Terminator 2 Judgment Day', 1991),
  ('Am%C3%A9lie.mkv', 'Am%C3%A9lie', null),
  // Cyrillic
  ('Брат.1997.BDRip.mkv', 'Брат', 1997),
  ('Брат 2 (2000).mkv', 'Брат 2', 2000),
  ('Ирония судьбы, или С лёгким паром! (1975).mkv', 'Ирония судьбы или С лёгким паром!', 1975),
  ('Москва слезам не верит 1979 DVDRip.avi', 'Москва слезам не верит', 1979),
  ('Сталкер.1979.BDRip.1080p.mkv', 'Сталкер', 1979),
  ('Левиафан [2014, BDRip 720p].mkv', 'Левиафан', 2014),
  ('Иван Васильевич меняет профессию (1973) [HD].mkv', 'Иван Васильевич меняет профессию', 1973),
  ('Т-34.2018.WEB-DL.1080p.mkv', 'Т-34', 2018),
  ('Ёлки.2010.mkv', 'Ёлки', 2010),
  ('Служебный роман 1977 дубляж.avi', 'Служебный роман', 1977),
  ('Кин-дза-дза! 1986.mkv', 'Кин-дза-дза!', 1986),
  ('Легенда №17 (2013).mkv', 'Легенда №17', 2013),
];

/// (input, show, season, episode)
const _episodes = <(String, String, int, int)>[
  ('Breaking.Bad.S01E01.720p.BluRay.x264-DEMAND.mkv', 'Breaking Bad', 1, 1),
  ('breaking.bad.s05e16.mkv', 'breaking bad', 5, 16),
  ('Game of Thrones - S08E06 - The Iron Throne.mkv', 'Game of Thrones', 8, 6),
  ('The.Office.US.S02E01.mkv', 'The Office US', 2, 1),
  ('Doctor.Who.2005.S01E01.Rose.mkv', 'Doctor Who', 1, 1),
  ('Friends.1x01.The.One.Where.Monica.Gets.a.Roommate.avi', 'Friends', 1, 1),
  ('Friends 10x17.avi', 'Friends', 10, 17),
  ('Lost.S1E1.mkv', 'Lost', 1, 1),
  ('Lost S01 E05.mkv', 'Lost', 1, 5),
  ('Sherlock.S03E02.1080p.mkv', 'Sherlock', 3, 2),
  ('The Mandalorian S02E08 Chapter 16.mkv', 'The Mandalorian', 2, 8),
  ('Stranger.Things.S04E09.2160p.NF.WEB-DL.DDP5.1.Atmos.DV.HEVC.mkv', 'Stranger Things', 4, 9),
  ('Mr.Robot.S01E01.eps1.0_hellofriend.mov.mkv', 'Mr Robot', 1, 1),
  ('Chernobyl.S01E05.mkv', 'Chernobyl', 1, 5),
  ('Severance.S02E10.Cold.Harbor.1080p.ATVP.WEB-DL.mkv', 'Severance', 2, 10),
  ('the_wire_s01e01.avi', 'the wire', 1, 1),
  ('Westworld.S01E01-E02.mkv', 'Westworld', 1, 1),
  ('Westworld.S01E01E02.mkv', 'Westworld', 1, 1),
  ('Show Name Season 2 Episode 5.mkv', 'Show Name', 2, 5),
  ('The Expanse - 3x07 - Delta-V.mkv', 'The Expanse', 3, 7),
  ('House.of.the.Dragon.S02E01.HMAX.WEB-DL.mkv', 'House of the Dragon', 2, 1),
  ('Shogun.2024.S01E01.Anjin.mkv', 'Shogun', 1, 1),
  ('S.W.A.T.2017.S01E01.mkv', 'S W A T', 1, 1),
  ('Rick and Morty S07E01 1080p.mkv', 'Rick and Morty', 7, 1),
  ('Rick.and.Morty.S07E01.1080p.WEB.H264-GRP[rarbg].mkv', 'Rick and Morty', 7, 1),
  ('The.Boys.S04E08.mkv', 'The Boys', 4, 8),
  ('The.Last.of.Us.S01E03.Long.Long.Time.mkv', 'The Last of Us', 1, 3),
  ('Dark S03E08 - The Paradise.mkv', 'Dark', 3, 8),
  ('Fargo.S05E10.720p.mkv', 'Fargo', 5, 10),
  ('Twin Peaks 2x22.avi', 'Twin Peaks', 2, 22),
  ('Simpsons S35E100.mkv', 'Simpsons', 35, 100),
  ('One Piece - S01E1071.mkv', 'One Piece', 1, 1071),
  // Cyrillic
  ('Метод.S01E01.WEB-DL.1080p.mkv', 'Метод', 1, 1),
  ('Кухня.S02E05.mkv', 'Кухня', 2, 5),
  ('Слово пацана. Кровь на асфальте S01E03.mkv', 'Слово пацана Кровь на асфальте', 1, 3),
  ('Интерны 1 сезон 5 серия.avi', 'Интерны', 1, 5),
  ('Мажор Сезон 2 Серия 4.mp4', 'Мажор', 2, 4),
  ('Бригада.1x03.avi', 'Бригада', 1, 3),
  ('Склифосовский s04e12.mkv', 'Склифосовский', 4, 12),
  ('Универ. Новая общага S01E01 [WEB-DL 720p].mkv', 'Универ Новая общага', 1, 1),
];

/// (input, show, episode)
const _anime = <(String, String, int)>[
  ('[SubsPlease] Sousou no Frieren - 05 (1080p) [ABCD1234].mkv', 'Sousou no Frieren', 5),
  ('[Erai-raws] Jujutsu Kaisen - 24 [1080p][Multiple Subtitle].mkv', 'Jujutsu Kaisen', 24),
  ('[HorribleSubs] Shingeki no Kyojin - 59 [720p].mkv', 'Shingeki no Kyojin', 59),
  ('[Judas] One Piece - 1000 [1080p][HEVC x265 10bit].mkv', 'One Piece', 1000),
  ('[SubsPlease] Spy x Family - 12 (1080p) [7D5A6B3C].mkv', 'Spy x Family', 12),
  ('[Group] Cowboy Bebop - 01v2 [BD 1080p].mkv', 'Cowboy Bebop', 1),
  ('[Group] Kimi no Na wa 01 [1080p].mkv', 'Kimi no Na wa', 1),
  ('[Fansub] Mushoku Tensei - 00 [720p].mkv', 'Mushoku Tensei', 0),
  ('[ASW] Dungeon Meshi - 03 [1080p HEVC][C1A2B3D4].mkv', 'Dungeon Meshi', 3),
  ('[SubsPlease] Boku no Hero Academia - 138 (720p).mkv', 'Boku no Hero Academia', 138),
  ('[Anime Time] Naruto - 001 [1080p].mkv', 'Naruto', 1),
  ('[Group] Re Zero kara Hajimeru Isekai Seikatsu - 26.mkv', 'Re Zero kara Hajimeru Isekai Seikatsu', 26),
  ('Frieren - 12 [1080p].mkv', 'Frieren', 12),
  ('[AniLibria] Атака титанов - 05 [WEBRip 1080p].mkv', 'Атака титанов', 5),
  ('[Group] Steins;Gate - 11.mkv', 'Steins Gate', 11),
];

void main() {
  group('movies', () {
    for (final (input, title, year) in _movies) {
      test(input, () {
        final r = parseName(_strip(input));
        expect(r.isEpisode, isFalse, reason: '$r');
        expect(r.title, title, reason: '$r');
        expect(r.year, year, reason: '$r');
      });
    }
  });

  group('episodes', () {
    for (final (input, show, s, e) in _episodes) {
      test(input, () {
        final r = parseName(_strip(input));
        expect(r.isEpisode, isTrue, reason: '$r');
        expect(r.title, show, reason: '$r');
        expect(r.season, s, reason: '$r');
        expect(r.episode, e, reason: '$r');
      });
    }
  });

  group('anime', () {
    for (final (input, show, e) in _anime) {
      test(input, () {
        final r = parseName(_strip(input));
        expect(r.isEpisode, isTrue, reason: '$r');
        expect(r.title, show, reason: '$r');
        expect(r.episode, e, reason: '$r');
        expect(r.season, 1, reason: '$r');
      });
    }
  });

  group('details', () {
    test('multi-episode end', () {
      expect(parseName('Westworld.S01E01-E02').episodeEnd, 2);
      expect(parseName('Westworld.S01E01E02E03').episodeEnd, 3);
      expect(parseName('Show.S01E05-06').episodeEnd, 6);
      expect(parseName('Show.S01E05').episodeEnd, isNull);
    });
    test('episode titles', () {
      expect(parseName('Game of Thrones - S08E06 - The Iron Throne').episodeTitle,
          'The Iron Throne');
      expect(parseName('The.Last.of.Us.S01E03.Long.Long.Time.1080p').episodeTitle,
          'Long Long Time');
      expect(parseName('Breaking.Bad.S01E01.720p').episodeTitle, isNull);
    });
    test('release groups', () {
      expect(parseName('[SubsPlease] Frieren - 05 (1080p)').releaseGroup,
          'SubsPlease');
      expect(parseName('The.Matrix.1999.1080p.BluRay.x264-GRP').releaseGroup,
          'GRP');
    });
    test('anime flag', () {
      expect(parseName('[SubsPlease] Frieren - 05').isAnime, isTrue);
      expect(parseName('Breaking.Bad.S01E01').isAnime, isFalse);
    });
    test('resolution', () {
      expect(parseName('Inception.2010.720p.WEB-DL').resolution, '720p');
      expect(parseName('[SubsPlease] Frieren - 05 (1080p)').resolution, '1080p');
    });
    test('dual titles', () {
      final r = parseName('Мастер и Маргарита / The Master and Margarita (2024)');
      expect(r.title, 'Мастер и Маргарита');
      expect(r.altTitle, 'The Master and Margarita');
      expect(r.year, 2024);
    });
    test('show year after title', () {
      final r = parseName('Doctor.Who.2005.S01E01');
      expect(r.year, 2005);
      expect(r.title, 'Doctor Who');
    });
    test('anime season in title', () {
      final a = parseName('[Group] Kaguya-sama S2 - 05 [1080p]');
      expect(a.title, 'Kaguya-sama');
      expect(a.season, 2);
      expect(a.episode, 5);
      final b = parseName('[Group] Mob Psycho 100 Season 3 - 02');
      expect(b.title, 'Mob Psycho 100');
      expect(b.season, 3);
      final c = parseName('[Group] Vinland Saga 2nd Season - 10');
      expect(c.title, 'Vinland Saga');
      expect(c.season, 2);
    });
    test('confidence ordering', () {
      expect(parseName('Breaking.Bad.S01E01').confidence,
          greaterThan(parseName('Alien').confidence));
      expect(parseName('Inception (2010)').confidence,
          greaterThan(parseName('Inception').confidence));
    });
    test('junk tokens', () {
      for (final j in ['1080p', 'x264', 'H264', 'WEB-DL', 'BluRay', 'DDP5.1',
          '10bit', 'HEVC', 'x265-GRP', 'AAC2.0', 'HDR10', 'REMUX', 'dts-hd']) {
        expect(isJunkToken(j), isTrue, reason: j);
      }
      for (final w in ['Matrix', 'Брат', 'Part', 'Two', '2']) {
        expect(isJunkToken(w), isFalse, reason: w);
      }
    });
    test('normalizeTitle', () {
      expect(normalizeTitle('The Office (US)'), 'office us');
      expect(normalizeTitle('Ёлки'), 'елки');
      expect(normalizeTitle('Rick & Morty'), 'rick and morty');
      expect(normalizeTitle("Ocean's Eleven"), 'oceans eleven');
      expect(normalizeTitle('Breaking.Bad'), 'breaking bad');
    });
  });

  group('folder context', () {
    test('season folder gives show and season', () {
      final r = parsePath('/media/TV/Breaking Bad/Season 2/03.mkv', root: '/media/TV');
      expect(r.isEpisode, isTrue);
      expect(r.title, 'Breaking Bad');
      expect(r.season, 2);
      expect(r.episode, 3);
    });
    test('Cyrillic season folder', () {
      final r = parsePath('/v/Сериалы/Кухня/Сезон 3/Серия 7.avi', root: '/v');
      expect(r.title, 'Кухня');
      expect(r.season, 3);
      expect(r.episode, 7);
    });
    test('"2 сезон" folder', () {
      final r = parsePath('/v/Интерны/2 сезон/05.avi', root: '/v');
      expect(r.title, 'Интерны');
      expect(r.season, 2);
      expect(r.episode, 5);
    });
    test('S01 folder with bare SxxExx files', () {
      final r = parsePath('/v/Dark (2017)/S01/S01E04.mkv', root: '/v');
      expect(r.title, 'Dark');
      expect(r.year, 2017);
      expect(r.season, 1);
      expect(r.episode, 4);
    });
    test('show folder with episode-only files', () {
      final r = parsePath('/v/Chernobyl/Episode 2.mkv', root: '/v');
      expect(r.isEpisode, isTrue);
      expect(r.title, 'Chernobyl');
      expect(r.season, 1);
      expect(r.episode, 2);
    });
    test('specials folder is season 0', () {
      final r = parsePath('/v/Doctor Who/Specials/Doctor.Who.S00E01.mkv', root: '/v');
      expect(r.season, 0);
      expect(r.title, 'Doctor Who');
    });
    test('season folder overrides filename show title', () {
      final r = parsePath('/v/The Office (US)/Season 1/The.Office.S01E01.mkv', root: '/v');
      expect(r.title, 'The Office US');
    });
    test('filename title kept when parent is not a season folder', () {
      final r = parsePath('/v/Downloads/Fargo.S05E10.720p.mkv', root: '/v');
      expect(r.title, 'Fargo');
    });
    test('movie folder with year beats messy file name', () {
      final r = parsePath('/m/Heat (1995)/heat-remastered.mkv', root: '/m');
      expect(r.isEpisode, isFalse);
      expect(r.title, 'Heat');
      expect(r.year, 1995);
    });
    test('file year wins over folder', () {
      final r = parsePath('/m/Collection/Alien.1979.mkv', root: '/m');
      expect(r.title, 'Alien');
      expect(r.year, 1979);
    });
    test('root folder never used as title', () {
      final r = parsePath('/m/My Movies (2020)/alien.mkv', root: '/m/My Movies (2020)');
      expect(r.title, 'alien');
      expect(r.year, isNull);
    });
    test('generic folders ignored', () {
      final r = parsePath('/data/Movies/Alien.mkv', root: '/data');
      expect(r.title, 'Alien');
      expect(r.year, isNull);
    });
    test('anime in a show folder', () {
      final r = parsePath('/a/Frieren/[SubsPlease] Sousou no Frieren - 05 (1080p).mkv', root: '/a');
      expect(r.title, 'Sousou no Frieren');
      expect(r.episode, 5);
    });
    test('seasonFromFolder', () {
      expect(seasonFromFolder('Season 1'), 1);
      expect(seasonFromFolder('season 12'), 12);
      expect(seasonFromFolder('S02'), 2);
      expect(seasonFromFolder('Сезон 3'), 3);
      expect(seasonFromFolder('3 сезон'), 3);
      expect(seasonFromFolder('Specials'), 0);
      expect(seasonFromFolder('Breaking Bad'), isNull);
      expect(seasonFromFolder('Superman'), isNull);
    });
  });
}

String _strip(String name) {
  final i = name.lastIndexOf('.');
  if (i > 0 && name.length - i <= 5) return name.substring(0, i);
  return name;
}
