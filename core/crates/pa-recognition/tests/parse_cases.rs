// SPDX-License-Identifier: MPL-2.0
//! Table-driven parser cases, grouped by hard category (03 §1.2, A1–A17).
//!
//! All names are written for this project (MPL-2.0). Release-group names are
//! public tags; titles are only used as strings.

mod common;

use common::run_table;
use serde_json::json;

#[test]
fn western_movies() {
    run_table(&[
        (
            "Inception.2010.1080p.BluRay.x264-SPARKS.mkv",
            json!({"title": "Inception", "year": 2010, "resolution": "1080p", "source": "BluRay", "video_codec": "x264", "release_group": "SPARKS", "media": "movie"}),
        ),
        (
            "The.Matrix.1999.720p.BRRip.x264.AAC-ETRG.mp4",
            json!({"title": "The Matrix", "year": 1999, "resolution": "720p", "source": "BRRip", "audio_codec": "AAC", "release_group": "ETRG"}),
        ),
        (
            "Dune (2021).mkv",
            json!({"title": "Dune", "year": 2021, "media": "movie"}),
        ),
        (
            "Dune.Part.Two.2024.2160p.WEB-DL.DDP5.1.Atmos.HDR.H.265-GROUP.mkv",
            json!({"title": "Dune Part Two", "year": 2024, "resolution": "2160p", "source": "WEB-DL", "audio_codec": "DDP5.1", "hdr": "HDR", "video_codec": "H.265", "release_group": "GROUP"}),
        ),
        (
            "Blade Runner 2049 (2017) [1080p].mkv",
            json!({"title": "Blade Runner 2049", "year": 2017, "resolution": "1080p"}),
        ),
        (
            "Blade.Runner.2049.2017.1080p.BluRay.mkv",
            json!({"title": "Blade Runner 2049", "year": 2017}),
        ),
        (
            "The Lord of the Rings The Fellowship of the Ring (2001) Extended Edition 1080p.mkv",
            json!({"title": "The Lord of the Rings The Fellowship of the Ring", "year": 2001, "edition": "Extended Edition"}),
        ),
        (
            "Apocalypse.Now.1979.Redux.720p.BluRay.mkv",
            json!({"title": "Apocalypse Now", "year": 1979, "edition": "Redux"}),
        ),
        (
            "Blade.Runner.1982.Final.Cut.1080p.BluRay.x264.mkv",
            json!({"title": "Blade Runner", "year": 1982, "edition": "Final Cut"}),
        ),
        (
            "Kingdom.of.Heaven.2005.Directors.Cut.1080p.mkv",
            json!({"title": "Kingdom of Heaven", "year": 2005, "edition": "Directors Cut"}),
        ),
        (
            "Aliens (1986) Director's Cut.mkv",
            json!({"title": "Aliens", "year": 1986, "edition": "Director's Cut"}),
        ),
        (
            "Movie.Title.2019.PROPER.1080p.WEB-DL.mkv",
            json!({"title": "Movie Title", "year": 2019, "other": ["PROPER"]}),
        ),
        (
            "Some.Film.2018.REPACK.720p.HDTV.x264.mkv",
            json!({"title": "Some Film", "year": 2018, "source": "HDTV", "other": ["REPACK"]}),
        ),
        (
            "Avatar.2009.UHD.BluRay.2160p.HDR10.HEVC.TrueHD.7.1.Atmos.mkv",
            json!({"title": "Avatar", "year": 2009, "source": "UHD BluRay", "hdr": "HDR10", "video_codec": "HEVC", "audio_codec": "TrueHD"}),
        ),
        (
            "Gravity.2013.3D.HSBS.1080p.BluRay.mkv",
            json!({"title": "Gravity", "year": 2013}),
        ),
        (
            "Heat 1995 1080p BluRay DTS x264.mkv",
            json!({"title": "Heat", "year": 1995, "audio_codec": "DTS"}),
        ),
        (
            "Ocean's.Eleven.2001.720p.mkv",
            json!({"title": "Ocean's Eleven", "year": 2001}),
        ),
        (
            "Amélie (2001).mkv",
            json!({"title": "Amélie", "year": 2001}),
        ),
        (
            "Léon The Professional 1994 Extended.mkv",
            json!({"title": "Léon The Professional", "year": 1994}),
        ),
        (
            "Mr. Nobody (2009).mkv",
            json!({"title": "Mr. Nobody", "year": 2009}),
        ),
        (
            "Spider-Man.No.Way.Home.2021.1080p.WEBRip.x265.mkv",
            json!({"title": "Spider-Man No Way Home", "year": 2021, "source": "WEBRip"}),
        ),
        (
            "WALL-E (2008).mkv",
            json!({"title": "WALL-E", "year": 2008}),
        ),
        (
            "X-Men.Days.of.Future.Past.2014.mkv",
            json!({"title": "X-Men Days of Future Past", "year": 2014}),
        ),
        (
            "Star Wars Episode IV A New Hope (1977).mkv",
            json!({"title": "Star Wars Episode IV A New Hope", "year": 1977}),
        ),
        (
            "Movie_Title_2015_720p.avi",
            json!({"title": "Movie Title", "year": 2015, "resolution": "720p"}),
        ),
        (
            "the_big_lebowski_1998_dvdrip_xvid.avi",
            json!({"title": "the big lebowski", "year": 1998, "source": "dvdrip", "video_codec": "xvid"}),
        ),
        (
            "Dune.mkv",
            json!({"title": "Dune", "year": null, "season": null}),
        ),
        (
            "Arrival.2016.1080p.AMZN.WEB-DL.DDP5.1.H.264-NTG.mkv",
            json!({"title": "Arrival", "year": 2016, "source": "WEB-DL", "release_group": "NTG"}),
        ),
        (
            "Parasite.2019.KOREAN.1080p.BluRay.mkv",
            json!({"title": "Parasite", "year": 2019, "languages": ["kor"]}),
        ),
        (
            "Amelie.2001.FRENCH.720p.mkv",
            json!({"title": "Amelie", "year": 2001, "languages": ["fre"]}),
        ),
        (
            "Movie.2020.MULTi.1080p.mkv",
            json!({"title": "Movie", "year": 2020, "other": ["MULTI"]}),
        ),
        (
            "Joker (2019) [2160p] [4K] [WEB] [5.1] [YTS.MX].mkv",
            json!({"title": "Joker", "year": 2019, "resolution": "2160p", "release_group": "YTS.MX"}),
        ),
        (
            "Old Movie (1931).avi",
            json!({"title": "Old Movie", "year": 1931}),
        ),
        (
            "Movie.Title.1080i.HDTV.mkv",
            json!({"title": "Movie Title", "resolution": "1080i"}),
        ),
        (
            "Some.Movie.2010.1920x1080.mkv",
            json!({"title": "Some Movie", "year": 2010, "resolution": "1920x1080"}),
        ),
        (
            "Fight.Club.10th.Anniversary.Edition.1999.mkv",
            json!({"title": "Fight Club 10th", "year": 1999, "edition": "Anniversary Edition"}),
        ),
        (
            "Inception.2010.HDR.DV.2160p.mkv",
            json!({"title": "Inception", "hdr": "HDR"}),
        ),
        (
            "The.Movie.2019.10bit.HEVC.mkv",
            json!({"title": "The Movie", "video_codec": "10bit"}),
        ),
        (
            "Pulp Fiction 1994 Remastered 1080p.mkv",
            json!({"title": "Pulp Fiction", "year": 1994, "edition": "Remastered"}),
        ),
        (
            "Movie (2012) Unrated.mkv",
            json!({"title": "Movie", "year": 2012, "edition": "Unrated"}),
        ),
    ]);
}

#[test]
fn western_series() {
    run_table(&[
        (
            "Breaking.Bad.S01E01.720p.BluRay.x264.mkv",
            json!({"title": "Breaking Bad", "season": 1, "episodes": [1], "media": "episode"}),
        ),
        (
            "Breaking Bad S05E14 Ozymandias 1080p.mkv",
            json!({"title": "Breaking Bad", "season": 5, "episodes": [14], "episode_title": "Ozymandias"}),
        ),
        (
            "game.of.thrones.s08e06.1080p.web.h264-memento.mkv",
            json!({"title": "game of thrones", "season": 8, "episodes": [6], "source": "web", "release_group": "memento"}),
        ),
        (
            "The.Office.US.S02E01.mkv",
            json!({"title": "The Office US", "season": 2, "episodes": [1]}),
        ),
        (
            "Friends.s1e1.avi",
            json!({"title": "Friends", "season": 1, "episodes": [1]}),
        ),
        (
            "Lost.S01.E05.mkv",
            json!({"title": "Lost", "season": 1, "episodes": [5]}),
        ),
        (
            "Lost S01 E05.mkv",
            json!({"title": "Lost", "season": 1, "episodes": [5]}),
        ),
        (
            "House.M.D.S03E07.mkv",
            json!({"title": "House M D", "season": 3, "episodes": [7]}),
        ),
        (
            "Doctor.Who.2005.S10E01.mkv",
            json!({"title": "Doctor Who", "year": 2005, "season": 10, "episodes": [1]}),
        ),
        (
            "Shogun.2024.S01E01.1080p.mkv",
            json!({"title": "Shogun", "year": 2024, "season": 1, "episodes": [1]}),
        ),
        (
            "Firefly - 1x01 - Serenity.mkv",
            json!({"title": "Firefly", "season": 1, "episodes": [1], "episode_title": "Serenity"}),
        ),
        (
            "Futurama 2x05.avi",
            json!({"title": "Futurama", "season": 2, "episodes": [5]}),
        ),
        (
            "Seinfeld - 3x10 - The Stranded.avi",
            json!({"title": "Seinfeld", "season": 3, "episodes": [10], "episode_title": "The Stranded"}),
        ),
        (
            "The Wire - Season 2 Episode 3.mkv",
            json!({"title": "The Wire", "season": 2, "episodes": [3]}),
        ),
        (
            "The.Wire.Season.2.Episode.3.mkv",
            json!({"title": "The Wire", "season": 2, "episodes": [3]}),
        ),
        (
            "Sherlock.S02E01.A.Scandal.in.Belgravia.720p.mkv",
            json!({"title": "Sherlock", "season": 2, "episodes": [1], "episode_title": "A Scandal in Belgravia"}),
        ),
        (
            "Show.Name.S01E01.PROPER.720p.HDTV.mkv",
            json!({"title": "Show Name", "season": 1, "episodes": [1], "other": ["PROPER"]}),
        ),
        (
            "Show Name - S01E01 - Pilot.mkv",
            json!({"title": "Show Name", "season": 1, "episodes": [1], "episode_title": "Pilot"}),
        ),
        (
            "Mr. Robot S01E01.mkv",
            json!({"title": "Mr. Robot", "season": 1, "episodes": [1]}),
        ),
        (
            "Stranger.Things.S04E09.2160p.NF.WEB-DL.DDP5.1.Atmos.DV.HDR.H.265-FLUX.mkv",
            json!({"title": "Stranger Things", "season": 4, "episodes": [9], "resolution": "2160p", "source": "WEB-DL", "release_group": "FLUX"}),
        ),
        (
            "The Mandalorian S02E08 Chapter 16.mkv",
            json!({"title": "The Mandalorian", "season": 2, "episodes": [8]}),
        ),
        (
            "Show.S2024E05.mkv",
            json!({"title": "Show", "season": 2024, "episodes": [5]}),
        ),
        (
            "Show.S01E100.mkv",
            json!({"title": "Show", "season": 1, "episodes": [100]}),
        ),
        (
            "Show.Name.E05.mkv",
            json!({"title": "Show Name", "episodes": [5]}),
        ),
        (
            "Show Name Ep 05.mkv",
            json!({"title": "Show Name", "episodes": [5]}),
        ),
        (
            "Show Name Ep.05.mkv",
            json!({"title": "Show Name", "episodes": [5]}),
        ),
        (
            "Show Name Episode 5.mkv",
            json!({"title": "Show Name", "episodes": [5]}),
        ),
        (
            "Show Name - EP05.mkv",
            json!({"title": "Show Name", "episodes": [5]}),
        ),
        (
            "Show Name #05.mkv",
            json!({"title": "Show Name", "episodes": [5]}),
        ),
        (
            "Show.Name.S03.1080p.BluRay.mkv",
            json!({"title": "Show Name", "season": 3}),
        ),
        (
            "Show Name Season 1 Complete.mkv",
            json!({"title": "Show Name", "season": 1}),
        ),
        (
            "Show.Name.S01E01.Rus.Eng.mkv",
            json!({"languages": ["rus", "eng"]}),
        ),
        (
            "Black Mirror - S03E01 - Nosedive (1080p).mkv",
            json!({"title": "Black Mirror", "season": 3, "episodes": [1], "episode_title": "Nosedive", "resolution": "1080p"}),
        ),
        (
            "Severance.S02E01.Hello.Ms.Cobel.1080p.ATVP.WEB-DL.mkv",
            json!({"title": "Severance", "season": 2, "episodes": [1]}),
        ),
        (
            "show.name.s01e01.web.h264-group.mkv",
            json!({"title": "show name", "release_group": "group"}),
        ),
    ]);
}

#[test]
fn multi_episode_a9() {
    run_table(&[
        (
            "Show.S01E01E02.mkv",
            json!({"season": 1, "episodes": [1, 2]}),
        ),
        (
            "Show.S01E01-E03.mkv",
            json!({"season": 1, "episodes": [1, 2, 3]}),
        ),
        (
            "Show.S01E01-03.mkv",
            json!({"season": 1, "episodes": [1, 2, 3]}),
        ),
        (
            "Show.S01E01E02E03.mkv",
            json!({"season": 1, "episodes": [1, 2, 3]}),
        ),
        ("Show 1x01-02.mkv", json!({"season": 1, "episodes": [1, 2]})),
        ("Show 1x01x02.mkv", json!({"season": 1, "episodes": [1, 2]})),
        (
            "TV/Breaking Bad/Season 1/1x06-07.mkv",
            json!({"title": "Breaking Bad", "season": 1, "episodes": [6, 7]}),
        ),
        (
            "[Group] Show - 01-02 [1080p].mkv",
            json!({"title": "Show", "episodes": [1, 2], "release_group": "Group"}),
        ),
        (
            "[Group] Show - 01~03 [1080p].mkv",
            json!({"title": "Show", "episodes": [1, 2, 3]}),
        ),
        (
            "[Group] Show - 01-12 [BD 1080p] [Batch].mkv",
            json!({"title": "Show", "episodes": [1,2,3,4,5,6,7,8,9,10,11,12]}),
        ),
        (
            "Show.E01-E02.mkv",
            json!({"title": "Show", "episodes": [1, 2]}),
        ),
        (
            "Show S02E09-E10 720p.mkv",
            json!({"season": 2, "episodes": [9, 10]}),
        ),
    ]);
}

#[test]
fn anime_fansub_a1_a6() {
    run_table(&[
        (
            "[SubsPlease] One Piece - 1085 (1080p) [ABCD1234].mkv",
            json!({"title": "One Piece", "absolute": [1085], "release_group": "SubsPlease", "crc32": "ABCD1234", "resolution": "1080p", "anime_hint": true}),
        ),
        (
            "[SubsPlease] Sousou no Frieren - 05 (1080p) [5E9A1C3B].mkv",
            json!({"title": "Sousou no Frieren", "episodes": [5], "crc32": "5E9A1C3B"}),
        ),
        (
            "[Erai-raws] Spy x Family - 01v2 [1080p][Multiple Subtitle][0F1E2D3C].mkv",
            json!({"title": "Spy x Family", "episodes": [1], "version": 2, "release_group": "Erai-raws", "crc32": "0F1E2D3C"}),
        ),
        (
            "[Erai-raws] Shingeki no Kyojin - The Final Season - 05 [1080p].mkv",
            json!({"title": "Shingeki no Kyojin - The Final Season", "episodes": [5]}),
        ),
        (
            "[HorribleSubs] Boku no Hero Academia - 88 [720p].mkv",
            json!({"title": "Boku no Hero Academia", "episodes": [88], "resolution": "720p"}),
        ),
        (
            "[Judas] Vinland Saga - S02E05.mkv",
            json!({"title": "Vinland Saga", "season": 2, "episodes": [5], "release_group": "Judas"}),
        ),
        (
            "[ASW] Dungeon Meshi - 12 [1080p HEVC x265 10Bit][AAC].mkv",
            json!({"title": "Dungeon Meshi", "episodes": [12], "resolution": "1080p", "video_codec": "HEVC"}),
        ),
        (
            "[Group] Steins;Gate 0 - 01 [1080p].mkv",
            json!({"title": "Steins;Gate 0", "episodes": [1]}),
        ),
        (
            "[Group] Mob Psycho 100 - 05 [1080p].mkv",
            json!({"title": "Mob Psycho 100", "episodes": [5]}),
        ),
        (
            "[Group] Mob Psycho 100 III - 05 [1080p].mkv",
            json!({"title": "Mob Psycho 100 III", "episodes": [5]}),
        ),
        (
            "[Group] 86 - 05 [1080p].mkv",
            json!({"title": "86", "episodes": [5]}),
        ),
        (
            "[Group] Re Zero - 18.5 [1080p].mkv",
            json!({"title": "Re Zero", "episodes": [18.5]}),
        ),
        (
            "[Group] Title - 05 END [1080p].mkv",
            json!({"title": "Title", "episodes": [5]}),
        ),
        (
            "[Group] Title - 05v3 [720p].mkv",
            json!({"title": "Title", "episodes": [5], "version": 3}),
        ),
        (
            "[Group] Title - 05 [v2] [720p].mkv",
            json!({"title": "Title", "episodes": [5], "version": 2}),
        ),
        (
            "[Group] Title 05 [1080p].mkv",
            json!({"title": "Title", "episodes": [5]}),
        ),
        (
            "[Group] Detective Conan - 1100 [720p].mkv",
            json!({"title": "Detective Conan", "absolute": [1100]}),
        ),
        (
            "Detective Conan - 1100 [720p].mkv",
            json!({"title": "Detective Conan", "absolute": [1100]}),
        ),
        (
            "[Group]_Title_-_05_[720p].mkv",
            json!({"title": "Title", "episodes": [5], "release_group": "Group"}),
        ),
        (
            "[Group].Title.-.05.[720p].mkv",
            json!({"title": "Title", "episodes": [5]}),
        ),
        (
            "[Nekomoe kissaten][Sousou no Frieren][05][1080p][JPSC].mp4",
            json!({"title": "Sousou no Frieren", "episodes": [5], "release_group": "Nekomoe kissaten"}),
        ),
        (
            "【喵萌奶茶屋】[Sousou no Frieren][05][1080p].mp4",
            json!({"title": "Sousou no Frieren", "episodes": [5], "release_group": "喵萌奶茶屋"}),
        ),
        (
            "[Group] Kaguya-sama wa Kokurasetai - 03 [1080p].mkv",
            json!({"title": "Kaguya-sama wa Kokurasetai", "episodes": [3]}),
        ),
        (
            "[Group] Title (2019) - 03 [1080p].mkv",
            json!({"title": "Title (2019)", "alt_titles": ["Title"], "year": 2019, "episodes": [3]}),
        ),
        (
            "[Group] Title - 03 (BD 1080p FLAC) [12345678].mkv",
            json!({"title": "Title", "episodes": [3], "source": "BD", "audio_codec": "FLAC", "crc32": "12345678"}),
        ),
        (
            "[Group] Title - 03 [deadbeef].mkv",
            json!({"crc32": "DEADBEEF"}),
        ),
        (
            "[Group] Title - 03 [Dual Audio] [1080p].mkv",
            json!({"title": "Title", "episodes": [3], "other": ["MULTI"]}),
        ),
        (
            "[Group] Bocchi the Rock! - 07 [1080p].mkv",
            json!({"title": "Bocchi the Rock!", "episodes": [7]}),
        ),
        (
            "[Group] Oshi no Ko - 11 (1080p) [ABCDEF01].mkv",
            json!({"title": "Oshi no Ko", "episodes": [11]}),
        ),
        (
            "Jujutsu Kaisen S2 - 01.mkv",
            json!({"title": "Jujutsu Kaisen", "season": 2, "episodes": [1]}),
        ),
        (
            "Re Zero kara Hajimeru Isekai Seikatsu 2nd Season - 01 [1080p].mkv",
            json!({"title": "Re Zero kara Hajimeru Isekai Seikatsu 2nd Season", "alt_titles": ["Re Zero kara Hajimeru Isekai Seikatsu"], "season": 2, "episodes": [1]}),
        ),
        (
            "[Group] Title 3rd Season - 04.mkv",
            json!({"title": "Title 3rd Season", "alt_titles": ["Title"], "season": 3, "episodes": [4]}),
        ),
        (
            "[Group] Title Season 2 - 04.mkv",
            json!({"title": "Title Season 2", "alt_titles": ["Title"], "season": 2, "episodes": [4]}),
        ),
        (
            "[Group] Shingeki no Kyojin The Final Season Part 2 - 01 [1080p][1A2B3C4D].mkv",
            json!({"title": "Shingeki no Kyojin The Final Season Part 2", "part": 2, "episodes": [1]}),
        ),
        (
            "[Group] Title - 1000 [1080p].mkv",
            json!({"absolute": [1000]}),
        ),
        (
            "Breaking Bad - 008.mkv",
            json!({"title": "Breaking Bad", "absolute": [8]}),
        ),
        ("[Group] Title - 012 [720p].mkv", json!({"absolute": [12]})),
        (
            "[Group] Title [01][1080p].mkv",
            json!({"title": "Title", "episodes": [1]}),
        ),
        (
            "[Group] Title - 05 (1280x720 x264 AAC).mkv",
            json!({"title": "Title", "episodes": [5], "resolution": "1280x720"}),
        ),
        (
            "[Group] Title - 05 [Hi10P][720p].mkv",
            json!({"video_codec": "Hi10P"}),
        ),
    ]);
}

#[test]
fn anime_specials_a4() {
    run_table(&[
        (
            "[Group] Sousou no Frieren - NCOP1 [BD 1080p].mkv",
            json!({"title": "Sousou no Frieren", "episode_kind": "ncop", "episodes": [1], "extra": "opening", "media": "extra"}),
        ),
        (
            "[Group] Sousou no Frieren - NCED2 [BD 1080p].mkv",
            json!({"episode_kind": "nced", "episodes": [2], "extra": "ending"}),
        ),
        (
            "[Group] Title - NCOP [BD].mkv",
            json!({"title": "Title", "episode_kind": "ncop"}),
        ),
        (
            "[Group] Title - NC OP 1 [BD].mkv",
            json!({"title": "Title", "episode_kind": "ncop"}),
        ),
        (
            "[Group] Shingeki no Kyojin OVA - 01 [BD 1080p].mkv",
            json!({"title": "Shingeki no Kyojin OVA", "alt_titles": ["Shingeki no Kyojin"], "episode_kind": "ova", "episodes": [1]}),
        ),
        (
            "[Group] Title OVA 2 [720p].mkv",
            json!({"title": "Title", "episode_kind": "ova", "episodes": [2]}),
        ),
        (
            "[Group] Title OVA2 [720p].mkv",
            json!({"title": "Title", "episode_kind": "ova", "episodes": [2]}),
        ),
        (
            "[Group] Title - OVA [720p].mkv",
            json!({"title": "Title", "episode_kind": "ova"}),
        ),
        (
            "[Group] Title - OAD 1 [720p].mkv",
            json!({"title": "Title", "episode_kind": "ova", "episodes": [1]}),
        ),
        (
            "[Group] Title - SP1 [720p].mkv",
            json!({"title": "Title", "episode_kind": "sp", "episodes": [1]}),
        ),
        (
            "[Group] Title - SP 2 [720p].mkv",
            json!({"title": "Title", "episode_kind": "sp", "episodes": [2]}),
        ),
        (
            "[Group] Title - ONA 3.mkv",
            json!({"title": "Title", "episode_kind": "ona", "episodes": [3]}),
        ),
        (
            "[Group] Title - Special 1.mkv",
            json!({"title": "Title", "episode_kind": "special", "episodes": [1]}),
        ),
        (
            "[Group] Title - OP1 [BD].mkv",
            json!({"title": "Title", "extra": "opening"}),
        ),
        (
            "[Group] Title - ED2 [BD].mkv",
            json!({"title": "Title", "extra": "ending"}),
        ),
        (
            "[Group] Title - PV1 [BD].mkv",
            json!({"title": "Title", "extra": "preview"}),
        ),
        (
            "[Group] Title - Menu [BD].mkv",
            json!({"title": "Title", "extra": "menu"}),
        ),
        (
            "Anime/Sousou no Frieren/Extras/[Group] Sousou no Frieren - NCOP1 [BD 1080p].mkv",
            json!({"extra": "opening"}),
        ),
        (
            "Anime/Title/Specials/Title - 01.mkv",
            json!({"title": "Title", "season": 0, "episodes": [1]}),
        ),
    ]);
}

#[test]
fn cjk_names_a5() {
    run_table(&[
        (
            "葬送のフリーレン 第2話.mkv",
            json!({"title": "葬送のフリーレン", "episodes": [2], "anime_hint": true}),
        ),
        (
            "葬送のフリーレン 第02話 (1080p).mkv",
            json!({"title": "葬送のフリーレン", "episodes": [2], "resolution": "1080p"}),
        ),
        (
            "進撃の巨人 第2期 第5話.mkv",
            json!({"title": "進撃の巨人", "season": 2, "episodes": [5]}),
        ),
        (
            "[Group] 葬送的芙莉莲 第05集 [1080p].mp4",
            json!({"title": "葬送的芙莉莲", "episodes": [5]}),
        ),
        (
            "鬼滅の刃 第１話.mkv",
            json!({"title": "鬼滅の刃", "episodes": [1]}),
        ),
        (
            "Anime/進撃の巨人/Attack on Titan - 01.mkv",
            json!({"title": "Attack on Titan", "alt_titles": ["進撃の巨人"], "episodes": [1]}),
        ),
        (
            "Anime/Frieren/Frieren - Beyond Journey's End - 01.mkv",
            json!({"title": "Frieren - Beyond Journey's End", "episodes": [1], "alt_titles": null}),
        ),
    ]);
}

#[test]
fn russian_releases_a7() {
    run_table(&[
        (
            "Мастер и Маргарита ∕ The Master and Margarita (2024) WEB-DL 1080p.mkv",
            json!({"title": "Мастер и Маргарита", "alt_titles": ["The Master and Margarita"], "year": 2024, "source": "WEB-DL"}),
        ),
        (
            "Мастер и Маргарита | The Master and Margarita (2024).mkv",
            json!({"title": "Мастер и Маргарита", "alt_titles": ["The Master and Margarita"]}),
        ),
        (
            "Ирония.судьбы.или.С.легким.паром.1975.BDRip.mkv",
            json!({"title": "Ирония судьбы или С легким паром", "year": 1975, "source": "BDRip"}),
        ),
        (
            "Ironiya.sudby.ili.S.legkim.parom.1975.Part1.BDRip.avi",
            json!({"title": "Ironiya sudby ili S legkim parom", "year": 1975, "part": 1}),
        ),
        (
            "Slovo.patsana.Krov.na.asfalte.S01E03.WEB-DL.1080p.mkv",
            json!({"title": "Slovo patsana Krov na asfalte", "season": 1, "episodes": [3]}),
        ),
        (
            "Breaking.Bad.S01E01.1080p.BDRip.Rus.Eng.LostFilm.mkv",
            json!({"dub_studio": "LostFilm", "languages": ["rus", "eng"]}),
        ),
        (
            "[AniLibria] Sousou no Frieren - 05 [WEBRip 1080p].mkv",
            json!({"release_group": "AniLibria", "dub_studio": "AniLibria", "languages": ["rus"], "episodes": [5]}),
        ),
        (
            "[AniLibria.TV] Sousou no Frieren - 05 [WEBRip 1080p].mkv",
            json!({"release_group": "AniLibria.TV", "dub_studio": "AniLibria.TV"}),
        ),
        (
            "[AniDub] Title [01] [720p].mkv",
            json!({"title": "Title", "dub_studio": "AniDub", "episodes": [1]}),
        ),
        (
            "Сериал.S02E05.NewStudio.mkv",
            json!({"title": "Сериал", "season": 2, "episodes": [5], "dub_studio": "NewStudio"}),
        ),
        (
            "Дюна (2021) BDRip 1080p.mkv",
            json!({"title": "Дюна", "year": 2021, "source": "BDRip"}),
        ),
        (
            "Брат 2 (2000).avi",
            json!({"title": "Брат 2", "year": 2000}),
        ),
        (
            "Сериалы/Слово пацана. Кровь на асфальте (2023)/Slovo.patsana.Krov.na.asfalte.S01E03.WEB-DL.1080p.mkv",
            json!({"alt_titles": ["Слово пацана. Кровь на асфальте"]}),
        ),
        (
            "Аниме/Фрирен/[AniLibria] Sousou no Frieren - 05 [WEBRip 1080p].mkv",
            json!({"alt_titles": ["Фрирен"], "anime_hint": true}),
        ),
        (
            "Шоу.Сезон 2.Серия 5.mkv",
            json!({"season": 2, "episodes": [5]}),
        ),
        (
            "Шоу 2 сезон 5 серия.mkv",
            json!({"title": "Шоу", "season": 2, "episodes": [5]}),
        ),
        (
            "Сериалы/Шоу/Сезон 3/Шоу - 07.mkv",
            json!({"title": "Шоу", "season": 3, "episodes": [7]}),
        ),
        (
            "Фильм.2010.DVDRip.MVO.avi",
            json!({"title": "Фильм", "year": 2010, "languages": ["rus"]}),
        ),
        (
            "Ведьмак.S01E01.Jaskier.mkv",
            json!({"dub_studio": "Jaskier"}),
        ),
        ("Film.2020.WEB-DLRip.avi", json!({"source": "WEB-DLRip"})),
        (
            "Фильм.2019.HDRip.Дубляж.avi",
            json!({"source": "HDRip", "languages": ["rus"]}),
        ),
    ]);
}

#[test]
fn numbers_in_titles_a14() {
    run_table(&[
        (
            "1917.2019.2160p.UHD.BluRay.x265.mkv",
            json!({"title": "1917", "year": 2019, "source": "UHD BluRay"}),
        ),
        ("1917 (2019).mkv", json!({"title": "1917", "year": 2019})),
        (
            "2001.A.Space.Odyssey.1968.mkv",
            json!({"title": "2001 A Space Odyssey", "year": 1968}),
        ),
        ("2012 (2009).mkv", json!({"title": "2012", "year": 2009})),
        (
            "9-1-1.S01E01.720p.HDTV.mkv",
            json!({"title": "9-1-1", "season": 1, "episodes": [1]}),
        ),
        (
            "24.S02E03.DVDRip.avi",
            json!({"title": "24", "season": 2, "episodes": [3]}),
        ),
        (
            "13 Reasons Why S01E01.mkv",
            json!({"title": "13 Reasons Why", "season": 1, "episodes": [1]}),
        ),
        ("300 (2006).mkv", json!({"title": "300", "year": 2006})),
        (
            "Apollo 13 (1995).mkv",
            json!({"title": "Apollo 13", "year": 1995}),
        ),
        (
            "Ocean's 11 (2001).mkv",
            json!({"title": "Ocean's 11", "year": 2001}),
        ),
        (
            "Toy Story 3 (2010).mkv",
            json!({"title": "Toy Story 3", "year": 2010}),
        ),
        (
            "Fantastic 4 (2015).mkv",
            json!({"title": "Fantastic 4", "year": 2015}),
        ),
        ("Se7en (1995).mkv", json!({"title": "Se7en", "year": 1995})),
        (
            "District 9 2009 720p.mkv",
            json!({"title": "District 9", "year": 2009}),
        ),
        (
            "The 100 S03E05.mkv",
            json!({"title": "The 100", "season": 3, "episodes": [5]}),
        ),
        ("11.22.63.S01E01.mkv", json!({"season": 1, "episodes": [1]})),
        (
            "Catch-22.S01E01.mkv",
            json!({"title": "Catch-22", "season": 1, "episodes": [1]}),
        ),
        (
            "Fahrenheit 451 (2018).mkv",
            json!({"title": "Fahrenheit 451", "year": 2018}),
        ),
        (
            "Steins;Gate 0 - 01.mkv",
            json!({"title": "Steins;Gate 0", "episodes": [1]}),
        ),
        (
            "Movies/1917 (2019)/1917.2019.2160p.UHD.BluRay.x265.mkv",
            json!({"title": "1917", "year": 2019}),
        ),
    ]);
}

#[test]
fn daily_shows_a11() {
    run_table(&[
        (
            "The.Daily.Show.2025.03.14.Guest.Name.1080p.WEB.mkv",
            json!({"title": "The Daily Show", "date": "2025-03-14", "media": "episode"}),
        ),
        (
            "Last.Week.Tonight.with.John.Oliver.2025.03.16.1080p.mkv",
            json!({"title": "Last Week Tonight with John Oliver", "date": "2025-03-16"}),
        ),
        (
            "Late Show 2024-11-02 720p.mkv",
            json!({"title": "Late Show", "date": "2024-11-02"}),
        ),
        (
            "Vecherniy.Urgant.2021.12.30.WEB-DL.mkv",
            json!({"title": "Vecherniy Urgant", "date": "2021-12-30"}),
        ),
        (
            "Новости 14.03.2025.mkv",
            json!({"title": "Новости", "date": "2025-03-14"}),
        ),
        ("Show.2025.13.45.mkv", json!({"date": null})),
    ]);
}

#[test]
fn disc_structures_a12() {
    run_table(&[
        (
            "Movies/Inception (2010)/BDMV/STREAM/00001.m2ts",
            json!({"title": "Inception", "year": 2010, "disc": "bdmv", "container": "m2ts"}),
        ),
        (
            "Movies/The Matrix (1999)/VIDEO_TS/VTS_01_1.VOB",
            json!({"title": "The Matrix", "year": 1999, "disc": "video_ts"}),
        ),
        (
            "Movies/The Matrix (1999)/VIDEO_TS/VIDEO_TS.IFO",
            json!({"title": "The Matrix", "disc": "video_ts"}),
        ),
        (
            "Anime Movies/Spirited Away (2001).iso",
            json!({"title": "Spirited Away", "year": 2001, "disc": "iso", "media": "movie"}),
        ),
        (
            "Movies/Inception (2010)",
            json!({"title": "Inception", "year": 2010}),
        ),
        (
            "Movies/Some Concert (2015)/BDMV/index.bdmv",
            json!({"title": "Some Concert", "disc": "bdmv"}),
        ),
    ]);
}

#[test]
fn split_files_a13() {
    run_table(&[
        (
            "The.Matrix.1999.DVDRip.cd1.avi",
            json!({"title": "The Matrix", "year": 1999, "part": 1}),
        ),
        ("The.Matrix.1999.DVDRip.CD2.avi", json!({"part": 2})),
        (
            "2001.A.Space.Odyssey.1968.disc1.mkv",
            json!({"title": "2001 A Space Odyssey", "part": 1}),
        ),
        (
            "Dune (1984) - part1.mkv",
            json!({"title": "Dune", "year": 1984, "part": 1}),
        ),
        (
            "Dune (1984) - Part 2.mkv",
            json!({"title": "Dune", "year": 1984, "part": 2}),
        ),
        ("Movie.2000.pt2.avi", json!({"title": "Movie", "part": 2})),
        (
            "Movie (2000) CD 1.avi",
            json!({"title": "Movie", "part": 1}),
        ),
        (
            "Movie (2000) Disc 2.mkv",
            json!({"title": "Movie", "part": 2}),
        ),
        ("Movies/Long Movie (1990)/CD2/movie.avi", json!({"part": 2})),
        (
            "Фильм.1975.Часть 2.avi",
            json!({"title": "Фильм", "part": 2}),
        ),
        (
            "Dune.Part.Two.2024.mkv",
            json!({"title": "Dune Part Two", "part": null}),
        ),
    ]);
}

#[test]
fn extras_a10() {
    run_table(&[
        (
            "Movies/Inception (2010)/Inception-trailer.mkv",
            json!({"extra": "trailer", "media": "extra"}),
        ),
        (
            "Movies/Inception (2010)/Inception (2010)-sample.mkv",
            json!({"extra": "sample"}),
        ),
        (
            "Movies/The Matrix (1999)/sample.mkv",
            json!({"extra": "sample", "title": "The Matrix"}),
        ),
        (
            "Movie.2010.1080p.BluRay.x264-GRP.sample.mkv",
            json!({"extra": "sample"}),
        ),
        (
            "Movies/Inception (2010)/Featurettes/Dream Team.mkv",
            json!({"extra": "featurette"}),
        ),
        (
            "Movies/Inception (2010)/Behind The Scenes/Making it.mkv",
            json!({"extra": "behind_the_scenes"}),
        ),
        (
            "Movies/Inception (2010)/Trailers/Teaser 1.mkv",
            json!({"extra": "trailer"}),
        ),
        (
            "Movies/Inception (2010)/Deleted Scenes/Scene 1.mkv",
            json!({"extra": "deleted_scene"}),
        ),
        (
            "TV/Breaking Bad/Season 1/Extras/Gag reel.mkv",
            json!({"extra": "other"}),
        ),
        (
            "Movie (2010) - featurette.mkv",
            json!({"extra": "featurette"}),
        ),
        (
            "Movie (2010) - behindthescenes.mkv",
            json!({"extra": "behind_the_scenes"}),
        ),
        (
            "Movie (2010)-deleted.mkv",
            json!({"extra": "deleted_scene"}),
        ),
        ("Movie (2010)-interview.mkv", json!({"extra": "interview"})),
        ("Movie (2010) Trailer.mkv", json!({"extra": "trailer"})),
        (
            "The Trailer Park Boys S01E01.mkv",
            json!({"title": "The Trailer Park Boys", "extra": null}),
        ),
        (
            "Web of Lies (2010).mkv",
            json!({"title": "Web of Lies", "source": null}),
        ),
        (
            "City of Ember (2008).mkv",
            json!({"title": "City of Ember", "release_group": null}),
        ),
    ]);
}

#[test]
fn folder_context_a2_a3_a15() {
    run_table(&[
        (
            "TV/Shogun/Season 1/Shogun.S01E01.mkv",
            json!({"title": "Shogun", "season": 1, "episodes": [1]}),
        ),
        (
            "TV/Shogun/Season 1/01.mkv",
            json!({"title": "Shogun", "season": 1, "episodes": [1]}),
        ),
        (
            "TV/Shogun/Season 1/Episode 01.mkv",
            json!({"title": "Shogun", "season": 1, "episodes": [1]}),
        ),
        (
            "TV/Shogun/Season 2/Shogun - 03.mkv",
            json!({"title": "Shogun", "season": 2, "episodes": [3]}),
        ),
        (
            "TV/Show (2019)/Season 02/Show.S02E05.mkv",
            json!({"title": "Show", "year": 2019, "season": 2, "episodes": [5]}),
        ),
        (
            "Anime/Jujutsu Kaisen S2/Jujutsu Kaisen S2 - 01.mkv",
            json!({"title": "Jujutsu Kaisen", "season": 2, "episodes": [1]}),
        ),
        (
            "Anime/Spy x Family Part 2/Spy x Family - 13 [1080p].mkv",
            json!({"title": "Spy x Family", "episodes": [13]}),
        ),
        (
            "Downloads/Inception.2010.1080p.BluRay.x264.mkv",
            json!({"title": "Inception", "year": 2010, "alt_titles": null}),
        ),
        (
            "Downloads/Breaking.Bad.S02E01.720p.mkv",
            json!({"title": "Breaking Bad", "season": 2, "episodes": [1]}),
        ),
        (
            "Movies/Dune.mkv",
            json!({"title": "Dune", "media": "movie"}),
        ),
        (
            "TV/Some Show/S03/Some.Show.E04.mkv",
            json!({"title": "Some Show", "season": 3, "episodes": [4]}),
        ),
        (
            "TV/Some Show/Сезон 2/Some Show 05.mkv",
            json!({"title": "Some Show", "season": 2, "episodes": [5]}),
        ),
        (
            "Anime/Attack on Titan The Final Season Part 2/Attack.on.Titan.S04E17.1080p.WEB.mkv",
            json!({"title": "Attack on Titan", "season": 4, "episodes": [17]}),
        ),
        (
            "C:\\Media\\Movies\\Heat (1995)\\Heat.1995.mkv",
            json!({"title": "Heat", "year": 1995}),
        ),
        (
            "Anime/Neon Genesis Evangelion/The End of Evangelion (1997).mkv",
            json!({"title": "The End of Evangelion", "year": 1997}),
        ),
    ]);
}

#[test]
fn edge_inputs() {
    run_table(&[
        ("", json!({"title": null})),
        (".mkv", json!({})),
        ("mkv", json!({"title": "mkv"})),
        ("[]", json!({})),
        ("[Group]", json!({"release_group": "Group"})),
        ("()()()", json!({})),
        ("----", json!({})),
        ("....", json!({})),
        ("Movie (2010", json!({"title": "Movie", "year": 2010})),
        ("Movie 2010)", json!({"title": "Movie"})),
        (
            "S01E01.mkv",
            json!({"season": 1, "episodes": [1], "title": null}),
        ),
        ("01.mkv", json!({})),
        ("VID_20240101_120000.mp4", json!({})),
        ("   Spaces Around   .mkv", json!({"title": "Spaces Around"})),
    ]);
}

/// META-006: anime and Russian release rules (names are made up, not corpus records).
#[test]
fn anime_and_russian_rules_meta006() {
    run_table(&[
        // A number that is the whole title, episode in its own bracket.
        (
            "[AniDUB] 86 [01] [1080p].mkv",
            json!({"title": "86", "episodes": [1], "release_group": "AniDUB"}),
        ),
        // A number before a dash-delimited kind names the work.
        (
            "[Group] 86 - NCOP (BD 1080p).mkv",
            json!({"title": "86", "episode_kind": "ncop", "episodes": null}),
        ),
        // Season qualifiers before ` - NN` stay in the title (AniDB naming).
        (
            "[Group] Some Show 2nd Season - 05 (1080p) [ABCDEF12].mkv",
            json!({"title": "Some Show 2nd Season", "alt_titles": ["Some Show"], "season": 2, "episodes": [5]}),
        ),
        (
            "[Group] Some Show (2011) - 120 [1080p].mkv",
            json!({"title": "Some Show (2011)", "year": 2011, "absolute": [120]}),
        ),
        (
            "[Group] Some Show Part 2 - NCED (BD 1080p).mkv",
            json!({"title": "Some Show Part 2", "part": 2, "episode_kind": "nced"}),
        ),
        // `S2` is a marker, not a name.
        (
            "Some Show S2 - 03.mkv",
            json!({"title": "Some Show", "season": 2, "episodes": [3]}),
        ),
        // Transliterated Russian episode marker and dub studio.
        (
            "Kovboy.Bibop.14.seriya.JAM.CLUB.mkv",
            json!({"title": "Kovboy Bibop", "episodes": [14], "dub_studio": "JAM CLUB"}),
        ),
        (
            "Nekiy.serial.03.seriya.Anything.Group.mkv",
            json!({"title": "Nekiy serial", "episodes": [3], "dub_studio": "Anything Group"}),
        ),
        // «Сезон N, серии 1–8 из 10».
        (
            "Сериал - Сезон 2, серии 1–8 из 10.mkv",
            json!({"title": "Сериал", "season": 2, "episodes": [1, 2, 3, 4, 5, 6, 7, 8]}),
        ),
        (
            "Аниме/Название ∕ Title [TV] [11 из 11] [RUS(ext), JAP+Sub]/[AniDUB] Title [03] [1080p].mkv",
            json!({"title": "Title", "episodes": [3]}),
        ),
        // Dub studio as the bracketed group.
        (
            "[AniLibria.TV] Some Show - 05 [WEBRip 1080p].mkv",
            json!({"title": "Some Show", "dub_studio": "AniLibria.TV", "episodes": [5]}),
        ),
        // Title words that look like release info.
        (
            "The.Italian.Job.1969.576p.DVD5.mkv",
            json!({"title": "The Italian Job", "year": 1969}),
        ),
        (
            "WWE.Raw.2023.01.16.720p.WEB.h264-GRP.mkv",
            json!({"title": "WWE Raw", "release_group": "GRP"}),
        ),
        (
            "[Group] Title (RAW) - 05 [1080p].mkv",
            json!({"title": "Title", "episodes": [5]}),
        ),
        (
            "Some Movie You Are (Not) Alone (2007).mkv",
            json!({"title": "Some Movie You Are (Not) Alone", "year": 2007}),
        ),
        (
            "Kill Bill - Volume 2 (2004).mkv",
            json!({"title": "Kill Bill - Volume 2", "year": 2004}),
        ),
        (
            "Blade.Runner.2049.720p.WEB-DL.mkv",
            json!({"title": "Blade Runner 2049", "absolute": null}),
        ),
        // Release group in the glued position.
        (
            "Show.S02E09.720p.WEB.H.264-Erai-raws.mkv",
            json!({"title": "Show", "release_group": "Erai-raws"}),
        ),
        (
            "Movie.2009.DVDRip.XviD-GRP.CD1.avi",
            json!({"title": "Movie", "release_group": "GRP", "part": 1}),
        ),
        (
            "Movie.2019.1080p.WEB-DL.x264-iNTERNAL.mkv",
            json!({"title": "Movie", "release_group": "iNTERNAL"}),
        ),
    ]);
}
