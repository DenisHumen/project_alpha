// SPDX-License-Identifier: MPL-2.0
//! Real-world name shapes that mix several conventions. Written for this project.

mod common;

use common::run_table;
use serde_json::json;

#[test]
fn mixed_real_world_shapes() {
    run_table(&[
        (
            "Show.Name.S01E01.720p.HDTV.x264-[eztv].mkv",
            json!({"title": "Show Name", "release_group": "eztv"}),
        ),
        (
            "Silo.S02E10.2160p.WEB.H265-SuccessfulCrab[TGx].mkv",
            json!({"title": "Silo", "season": 2, "episodes": [10], "release_group": "SuccessfulCrab"}),
        ),
        (
            "Show.Name.2x03.HDTV.XviD-LOL.avi",
            json!({"title": "Show Name", "season": 2, "episodes": [3], "release_group": "LOL", "video_codec": "XviD"}),
        ),
        (
            "Game of Thrones/Season 8/Game.of.Thrones.S08E03.The.Long.Night.mkv",
            json!({"title": "Game of Thrones", "episode_title": "The Long Night"}),
        ),
        (
            "[Group] Kimetsu no Yaiba - Yuukaku-hen - 05 [1080p].mkv",
            json!({"title": "Kimetsu no Yaiba - Yuukaku-hen", "episodes": [5]}),
        ),
        (
            "Avengers.Infinity.War.2018.IMAX.2160p.mkv",
            json!({"title": "Avengers Infinity War", "year": 2018, "edition": "IMAX"}),
        ),
        (
            "[Group] One Punch Man S2 - 03 [720p].mkv",
            json!({"title": "One Punch Man", "season": 2, "episodes": [3]}),
        ),
        (
            "Movie.Title.(2010).[1080p].mkv",
            json!({"title": "Movie Title", "year": 2010, "resolution": "1080p"}),
        ),
        (
            "Movie Name [2010] [1080p].mkv",
            json!({"title": "Movie Name", "year": 2010}),
        ),
        (
            "[Group] Title - 24 [Fin] [1080p].mkv",
            json!({"title": "Title", "episodes": [24]}),
        ),
        (
            "[Group] Tensei Shitara Slime Datta Ken - 48.5 [1080p].mkv",
            json!({"episodes": [48.5]}),
        ),
        (
            "[Group] Title - 13 (BD 1920x1080 x265 10bit FLAC) [Dual-Audio].mkv",
            json!({"title": "Title", "episodes": [13], "source": "BD", "video_codec": "x265", "audio_codec": "FLAC"}),
        ),
        (
            "[Group] Title - Vol.1 [BD].mkv",
            json!({"title": "Title", "episodes": null}),
        ),
        (
            "ООО Сериал (2020) S01E01 WEB-DL 1080p.mkv",
            json!({"title": "ООО Сериал", "year": 2020, "season": 1, "episodes": [1]}),
        ),
        (
            "Ходячие мертвецы 1 сезон 3 серия.avi",
            json!({"title": "Ходячие мертвецы", "season": 1, "episodes": [3]}),
        ),
        (
            "The.Simpsons.S35E01.1080p.mkv",
            json!({"title": "The Simpsons", "season": 35, "episodes": [1]}),
        ),
        (
            "Some.Show.S01E01.1080p.AMZN.WEB-DL.DDP2.0.H.264-NTb.mkv",
            json!({"title": "Some Show", "audio_codec": "DDP2.0", "video_codec": "H.264", "release_group": "NTb"}),
        ),
        (
            "Show_Name_-_01_[720p].mkv",
            json!({"title": "Show Name", "episodes": [1]}),
        ),
        (
            "Show.Name.1080p.S01E01.mkv",
            json!({"title": "Show Name", "season": 1, "episodes": [1]}),
        ),
        (
            "Movie (2010) [Rus, Eng].mkv",
            json!({"title": "Movie", "languages": ["rus", "eng"]}),
        ),
    ]);
}
