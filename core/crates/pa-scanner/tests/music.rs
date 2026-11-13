// SPDX-License-Identifier: MPL-2.0
//! Music tags and indexing (MUS-001) on tiny audio files generated here:
//! valid container headers with silent or empty payloads, tagged with lofty.

use std::path::Path;
use std::time::Instant;

use lofty::config::WriteOptions;
use lofty::file::{AudioFile, TaggedFileExt};
use lofty::picture::{MimeType, Picture, PictureType};
use lofty::tag::{Accessor, ItemKey, ItemValue, Tag, TagExt, TagItem, TagType};
use pa_library::music::{self, ArtKind, VARIOUS_ARTISTS};
use pa_library::rusqlite::Connection;
use pa_scanner::{
    CancelToken, MusicOptions, ScanEvent, ScanOptions, index_music, read_embedded_art, read_tags,
    scan,
};
use pa_vfs::{LocalFs, VfsPath};

const MBID: &str = "b1a9c0e9-d987-4042-ae91-78d6a3267d69";
const JPEG: &[u8] = &[
    0xff, 0xd8, 0xff, 0xe0, 0, 0x10, b'J', b'F', b'I', b'F', 0, 1, 0xff, 0xd9,
];

// --- Container generators ------------------------------------------------------

fn atom(name: &[u8; 4], body: &[u8]) -> Vec<u8> {
    let mut out = u32::try_from(body.len() + 8)
        .unwrap()
        .to_be_bytes()
        .to_vec();
    out.extend_from_slice(name);
    out.extend_from_slice(body);
    out
}

fn cat(parts: &[&[u8]]) -> Vec<u8> {
    parts.concat()
}

fn wav() -> Vec<u8> {
    let data = vec![0u8; 17_640]; // 0.1 s of 16-bit stereo at 44.1 kHz
    let mut fmt = Vec::new();
    fmt.extend_from_slice(&1u16.to_le_bytes()); // PCM
    fmt.extend_from_slice(&2u16.to_le_bytes());
    fmt.extend_from_slice(&44_100u32.to_le_bytes());
    fmt.extend_from_slice(&176_400u32.to_le_bytes());
    fmt.extend_from_slice(&4u16.to_le_bytes());
    fmt.extend_from_slice(&16u16.to_le_bytes());
    let chunk = |id: &[u8], body: &[u8]| {
        cat(&[id, &u32::try_from(body.len()).unwrap().to_le_bytes(), body])
    };
    let body = cat(&[b"WAVE", &chunk(b"fmt ", &fmt), &chunk(b"data", &data)]);
    chunk(b"RIFF", &body)
}

fn flac() -> Vec<u8> {
    // STREAMINFO only: 44.1 kHz, stereo, 16 bit, 4410 samples.
    let packed: u64 = (44_100 << 44) | (1 << 41) | (15 << 36) | 4410;
    let mut info = vec![0x10, 0x00, 0x10, 0x00, 0, 0, 0, 0, 0, 0];
    info.extend_from_slice(&packed.to_be_bytes());
    info.extend_from_slice(&[0; 16]);
    cat(&[b"fLaC", &[0x80, 0, 0, 34], &info])
}

fn mp3_frames(n: usize) -> Vec<u8> {
    let mut out = Vec::new();
    for _ in 0..n {
        out.extend_from_slice(&[0xff, 0xfb, 0x90, 0x64]);
        out.resize(out.len() + 413, 0);
    }
    out
}

fn ogg_crc(data: &[u8]) -> u32 {
    let mut crc = 0u32;
    for &b in data {
        crc ^= u32::from(b) << 24;
        for _ in 0..8 {
            crc = if crc & 0x8000_0000 != 0 {
                (crc << 1) ^ 0x04c1_1db7
            } else {
                crc << 1
            };
        }
    }
    crc
}

fn ogg_page(kind: u8, granule: u64, seq: u32, packets: &[&[u8]]) -> Vec<u8> {
    let mut lacing = Vec::new();
    for p in packets {
        let mut n = p.len();
        while n >= 255 {
            lacing.push(255);
            n -= 255;
        }
        lacing.push(u8::try_from(n).unwrap());
    }
    let mut page = b"OggS".to_vec();
    page.extend_from_slice(&[0, kind]);
    page.extend_from_slice(&granule.to_le_bytes());
    page.extend_from_slice(&0x5041_u32.to_le_bytes());
    page.extend_from_slice(&seq.to_le_bytes());
    page.extend_from_slice(&[0; 4]);
    page.push(u8::try_from(lacing.len()).unwrap());
    page.extend_from_slice(&lacing);
    for p in packets {
        page.extend_from_slice(p);
    }
    let crc = ogg_crc(&page);
    page[22..26].copy_from_slice(&crc.to_le_bytes());
    page
}

fn opus() -> Vec<u8> {
    let mut head = b"OpusHead".to_vec();
    head.extend_from_slice(&[1, 2]);
    head.extend_from_slice(&312u16.to_le_bytes());
    head.extend_from_slice(&48_000u32.to_le_bytes());
    head.extend_from_slice(&[0, 0, 0]);
    let tags = cat(&[
        b"OpusTags",
        &4u32.to_le_bytes(),
        b"test",
        &0u32.to_le_bytes(),
    ]);
    cat(&[
        &ogg_page(2, 0, 0, &[&head]),
        &ogg_page(0, 0, 1, &[&tags]),
        &ogg_page(4, 312 + 4800, 2, &[&[0xf8, 0xff, 0xfe]]),
    ])
}

fn vorbis() -> Vec<u8> {
    let mut ident = b"\x01vorbis".to_vec();
    ident.extend_from_slice(&0u32.to_le_bytes());
    ident.push(2);
    ident.extend_from_slice(&44_100u32.to_le_bytes());
    ident.extend_from_slice(&0i32.to_le_bytes());
    ident.extend_from_slice(&128_000i32.to_le_bytes());
    ident.extend_from_slice(&0i32.to_le_bytes());
    ident.extend_from_slice(&[0xb8, 1]);
    let comment = cat(&[
        b"\x03vorbis",
        &4u32.to_le_bytes(),
        b"test",
        &0u32.to_le_bytes(),
        &[1],
    ]);
    let setup = cat(&[b"\x05vorbis", &[0; 8]]);
    cat(&[
        &ogg_page(2, 0, 0, &[&ident]),
        &ogg_page(0, 0, 1, &[&comment, &setup]),
        &ogg_page(4, 4410, 2, &[&[0; 4]]),
    ])
}

fn m4a(alac: bool) -> Vec<u8> {
    let mut mdhd = vec![0; 12];
    mdhd.extend_from_slice(&44_100u32.to_be_bytes());
    mdhd.extend_from_slice(&4410u32.to_be_bytes());
    mdhd.extend_from_slice(&[0; 4]);
    let hdlr = cat(&[&[0; 8], b"soun", &[0; 13]]);

    let mut entry = vec![
        0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 2, 0, 16, 0, 0, 0, 0,
    ];
    entry.extend_from_slice(&(44_100u32 << 16).to_be_bytes());
    let sample_entry = if alac {
        let mut cookie = vec![0; 4];
        cookie.extend_from_slice(&4096u32.to_be_bytes());
        cookie.extend_from_slice(&[0, 16, 40, 10, 14, 2, 0, 255]);
        cookie.extend_from_slice(&[0; 8]);
        cookie.extend_from_slice(&44_100u32.to_be_bytes());
        atom(b"alac", &cat(&[&entry, &atom(b"alac", &cookie)]))
    } else {
        let esds = [
            0, 0, 0, 0, 0x03, 0x19, 0, 1, 0, 0x04, 0x11, 0x40, 0x15, 0, 0, 0, 0, 1, 0xf4, 0, 0, 1,
            0xf4, 0, 0x05, 0x02, 0x12, 0x10, 0x06, 0x01, 0x02,
        ];
        atom(b"mp4a", &cat(&[&entry, &atom(b"esds", &esds)]))
    };
    let stsd = atom(b"stsd", &cat(&[&[0, 0, 0, 0, 0, 0, 0, 1], &sample_entry]));
    let stts = atom(
        b"stts",
        &[0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0x11, 0x3a],
    );
    let minf = atom(
        b"minf",
        &cat(&[
            &atom(b"smhd", &[0; 8]),
            &atom(b"stbl", &cat(&[&stsd, &stts])),
        ]),
    );
    let mdia = atom(
        b"mdia",
        &cat(&[&atom(b"mdhd", &mdhd), &atom(b"hdlr", &hdlr), &minf]),
    );
    cat(&[
        &atom(b"ftyp", b"M4A \0\0\0\0M4A mp42isom"),
        &atom(b"mdat", &[0; 64]),
        &atom(b"moov", &atom(b"trak", &mdia)),
    ])
}

fn dsf(id3: &[u8]) -> Vec<u8> {
    let data = vec![0u8; 8192];
    let meta = 28 + 52 + 12 + data.len();
    let total = meta + id3.len();
    let mut out = b"DSD ".to_vec();
    out.extend_from_slice(&28u64.to_le_bytes());
    out.extend_from_slice(&u64::try_from(total).unwrap().to_le_bytes());
    out.extend_from_slice(&u64::try_from(meta).unwrap().to_le_bytes());
    out.extend_from_slice(b"fmt ");
    out.extend_from_slice(&52u64.to_le_bytes());
    for v in [1u32, 0, 2, 2, 2_822_400, 1] {
        out.extend_from_slice(&v.to_le_bytes());
    }
    out.extend_from_slice(&282_240u64.to_le_bytes()); // 0.1 s
    out.extend_from_slice(&4096u32.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&u64::try_from(12 + data.len()).unwrap().to_le_bytes());
    out.extend_from_slice(&data);
    out.extend_from_slice(id3);
    out
}

// --- Tagging ---------------------------------------------------------------------

#[derive(Default, Clone)]
struct Meta<'a> {
    title: &'a str,
    artists: &'a [&'a str],
    album: Option<&'a str>,
    album_artist: Option<&'a str>,
    compilation: bool,
    track: Option<u32>,
    disc: Option<(u32, u32)>,
    year: Option<&'a str>,
    genres: &'a [&'a str],
    extra: &'a [(ItemKey, &'a str)],
    cover: bool,
}

fn fill_tag(tag: &mut Tag, m: &Meta) {
    tag.set_title(m.title.into());
    for a in m.artists {
        tag.push(TagItem::new(
            ItemKey::TrackArtist,
            ItemValue::Text((*a).into()),
        ));
    }
    if let Some(a) = m.album {
        tag.set_album(a.into());
    }
    if let Some(a) = m.album_artist {
        tag.insert_text(ItemKey::AlbumArtist, a.into());
    }
    if m.compilation {
        tag.insert_text(ItemKey::FlagCompilation, "1".into());
    }
    if let Some(n) = m.track {
        tag.set_track(n);
    }
    if let Some((d, total)) = m.disc {
        tag.set_disk(d);
        tag.set_disk_total(total);
    }
    if let Some(y) = m.year {
        tag.insert_text(ItemKey::RecordingDate, y.into());
    }
    for g in m.genres {
        tag.push(TagItem::new(ItemKey::Genre, ItemValue::Text((*g).into())));
    }
    for (k, v) in m.extra {
        tag.insert_text(*k, (*v).into());
    }
    if m.cover {
        tag.push_picture(
            Picture::unchecked(JPEG.to_vec())
                .pic_type(PictureType::CoverFront)
                .mime_type(MimeType::Jpeg)
                .build(),
        );
    }
}

/// Writes an audio file of format `ext` with tags `m` at `path`.
fn write_track(path: &Path, ext: &str, m: &Meta) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    if ext == "dsf" {
        let mut tag = Tag::new(TagType::Id3v2);
        fill_tag(&mut tag, m);
        let mut id3 = Vec::new();
        tag.dump_to(&mut id3, WriteOptions::default()).unwrap();
        std::fs::write(path, dsf(&id3)).unwrap();
        return;
    }
    let bytes = match ext {
        "mp3" => mp3_frames(4),
        "flac" => flac(),
        "m4a" => m4a(false),
        "alac" => m4a(true),
        "opus" => opus(),
        "ogg" => vorbis(),
        "wav" => wav(),
        other => panic!("no generator for {other}"),
    };
    std::fs::write(path, bytes).unwrap();
    let mut file = lofty::read_from_path(path).unwrap();
    let mut tag = Tag::new(file.primary_tag_type());
    fill_tag(&mut tag, m);
    file.insert_tag(tag);
    file.save_to_path(path, WriteOptions::default()).unwrap();
}

fn vpath(s: &str) -> VfsPath {
    VfsPath::parse(s).unwrap()
}

// --- Reading tags --------------------------------------------------------------

#[test]
fn reads_tags_of_every_format() {
    let dir = tempfile::tempdir().unwrap();
    let meta = Meta {
        title: "Song",
        artists: &["Artist"],
        album: Some("Album"),
        album_artist: Some("Album Artist"),
        track: Some(3),
        disc: Some((1, 2)),
        year: Some("1999"),
        genres: &["Rock"],
        extra: &[(ItemKey::MusicBrainzRecordingId, MBID)],
        cover: true,
        ..Meta::default()
    };
    // (generator, file extension, codec, lossless)
    let formats = [
        ("mp3", "mp3", "mp3", false),
        ("flac", "flac", "flac", true),
        ("m4a", "m4a", "aac", false),
        ("alac", "m4a", "alac", true),
        ("opus", "opus", "opus", false),
        ("ogg", "ogg", "vorbis", false),
        ("wav", "wav", "pcm", true),
        ("dsf", "dsf", "dsd", true),
    ];
    let vfs = LocalFs::new(dir.path()).unwrap();
    for (generator, ext, codec, lossless) in formats {
        let name = format!("{generator}.{ext}");
        write_track(&dir.path().join(&name), generator, &meta);
        let tags =
            read_tags(&vfs, &vpath(&format!("/{name}"))).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(tags.title.as_deref(), Some("Song"), "{name}");
        assert_eq!(tags.artists, ["Artist"], "{name}");
        assert_eq!(tags.album.as_deref(), Some("Album"), "{name}");
        assert_eq!(tags.album_artists, ["Album Artist"], "{name}");
        assert_eq!(tags.track_number, Some(3), "{name}");
        assert_eq!(
            (tags.disc_number, tags.disc_total),
            (Some(1), Some(2)),
            "{name}"
        );
        assert_eq!(tags.year, Some(1999), "{name}");
        assert_eq!(tags.genres, ["Rock"], "{name}");
        assert_eq!(tags.recording_mbid.as_deref(), Some(MBID), "{name}");
        assert!(tags.has_embedded_art, "{name}");
        assert_eq!(tags.audio.codec.as_deref(), Some(codec), "{name}");
        assert_eq!(tags.audio.lossless, Some(lossless), "{name}");
        assert!(!tags.tags_failed, "{name}");

        let art = read_embedded_art(&vfs, &vpath(&format!("/{name}")))
            .unwrap()
            .unwrap_or_else(|| panic!("{name}: no art"));
        assert_eq!(art.data, JPEG, "{name}");
    }

    let dsd = read_tags(&vfs, &vpath("/dsf.dsf")).unwrap();
    assert_eq!(dsd.audio.sample_rate_hz, Some(2_822_400));
    assert_eq!(dsd.audio.duration_ms, Some(100));
    assert_eq!(dsd.audio.bit_depth, Some(1));
    let flac = read_tags(&vfs, &vpath("/flac.flac")).unwrap();
    assert_eq!(
        (
            flac.audio.sample_rate_hz,
            flac.audio.bit_depth,
            flac.audio.channels
        ),
        (Some(44_100), Some(16), Some(2))
    );
    assert_eq!(flac.audio.duration_ms, Some(100));
}

#[test]
fn reads_replaygain_multi_values_and_flags() {
    let dir = tempfile::tempdir().unwrap();
    let vfs = LocalFs::new(dir.path()).unwrap();
    write_track(
        &dir.path().join("rg.flac"),
        "flac",
        &Meta {
            title: "T",
            artists: &["A", "B"],
            genres: &["Jazz", "Funk"],
            compilation: true,
            extra: &[
                (ItemKey::ReplayGainTrackGain, "-6.50 dB"),
                (ItemKey::ReplayGainTrackPeak, "0.988"),
                (ItemKey::ReplayGainAlbumGain, "-7.25 dB"),
                (ItemKey::ReplayGainAlbumPeak, "1.000"),
                (ItemKey::Composer, "C"),
            ],
            ..Meta::default()
        },
    );
    let t = read_tags(&vfs, &vpath("/rg.flac")).unwrap();
    assert_eq!(t.artists, ["A", "B"]);
    assert_eq!(t.genres, ["Jazz", "Funk"]);
    assert_eq!(t.composers, ["C"]);
    assert!(t.compilation);
    assert_eq!(t.replay_gain.track_gain_db, Some(-6.5));
    assert_eq!(t.replay_gain.track_peak, Some(0.988));
    assert_eq!(t.replay_gain.album_gain_db, Some(-7.25));
    assert_eq!(t.replay_gain.album_peak, Some(1.0));
    assert!(!t.has_embedded_art);

    write_track(
        &dir.path().join("r128.opus"),
        "opus",
        &Meta {
            title: "T",
            extra: &[
                (ItemKey::R128TrackGain, "-512"),
                (ItemKey::R128AlbumGain, "256"),
            ],
            ..Meta::default()
        },
    );
    let t = read_tags(&vfs, &vpath("/r128.opus")).unwrap();
    assert_eq!(t.replay_gain.track_gain_db, Some(3.0));
    assert_eq!(t.replay_gain.album_gain_db, Some(6.0));

    std::fs::write(dir.path().join("bad.mp3"), b"not audio at all").unwrap();
    assert!(read_tags(&vfs, &vpath("/bad.mp3")).is_err());
}

// --- Indexing --------------------------------------------------------------------

fn library() -> Connection {
    let conn = pa_library::open_in_memory().unwrap();
    conn.execute(
        "INSERT INTO sources (id, kind, name, uri, created_at_ms) VALUES (1, 'local', 'Music', 'file:///music', 0)",
        [],
    )
    .unwrap();
    conn
}

fn run(conn: &mut Connection, root: &Path, now: i64) -> (pa_scanner::MusicReport, Vec<ScanEvent>) {
    let vfs = LocalFs::new(root).unwrap();
    let opts = ScanOptions {
        now_ms: Some(now),
        ..ScanOptions::default()
    };
    scan(conn, 1, &vfs, &opts, &CancelToken::new(), &mut |_| {}).unwrap();
    let mut events = Vec::new();
    let report = index_music(
        conn,
        1,
        &vfs,
        &MusicOptions {
            now_ms: Some(now),
            ..MusicOptions::default()
        },
        &CancelToken::new(),
        &mut |e| events.push(e.clone()),
    )
    .unwrap();
    (report, events)
}

fn album_by_title(conn: &Connection, title: &str) -> Vec<pa_model::Album> {
    music::albums(conn)
        .unwrap()
        .into_iter()
        .filter(|a| a.title == title)
        .collect()
}

fn artist_names(conn: &Connection, ids: &[i64]) -> Vec<String> {
    let all = music::artists(conn).unwrap();
    ids.iter()
        .map(|id| all.iter().find(|a| a.id == *id).unwrap().name.clone())
        .collect()
}

fn compilation(conn: &Connection, album_id: i64) -> bool {
    conn.query_row(
        "SELECT compilation FROM music_albums WHERE id = ?1",
        [album_id],
        |r| r.get(0),
    )
    .unwrap()
}

#[test]
fn indexes_albums_compilations_discs_and_covers() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    fn m<'a>(title: &'a str, artists: &'a [&'a str], album: &'a str) -> Meta<'a> {
        Meta {
            title,
            artists,
            album: Some(album),
            ..Meta::default()
        }
    }
    // A regular album with a folder cover.
    write_track(
        &root.join("Artist A/Album One/01 - First.flac"),
        "flac",
        &Meta {
            track: Some(1),
            year: Some("2001"),
            ..m("First", &["Artist A"], "Album One")
        },
    );
    write_track(
        &root.join("Artist A/Album One/02 - Second.flac"),
        "flac",
        &Meta {
            track: Some(2),
            year: Some("2001"),
            ..m("Second", &["Artist A", "Guest"], "Album One")
        },
    );
    std::fs::write(root.join("Artist A/Album One/Folder.JPG"), JPEG).unwrap();
    // No album artist, several artists in one folder: a compilation.
    write_track(
        &root.join("Mixes/Hits/01.mp3"),
        "mp3",
        &Meta {
            cover: true,
            ..m("X1", &["X"], "Hits")
        },
    );
    write_track(
        &root.join("Mixes/Hits/02.mp3"),
        "mp3",
        &m("Y1", &["Y"], "Hits"),
    );
    // Tagged compilation over two disc folders.
    for (disc, artist) in [(1, ["P"]), (2, ["Q"])] {
        write_track(
            &root.join(format!("Best Of/CD{disc}/01.m4a")),
            "m4a",
            &Meta {
                album_artist: Some(VARIOUS_ARTISTS),
                compilation: true,
                disc: Some((disc, 2)),
                track: Some(1),
                ..m("Tune", &artist, "Best Of")
            },
        );
    }
    // Same title, different artists and folders: two albums.
    write_track(
        &root.join("Band 1/Greatest Hits/01.ogg"),
        "ogg",
        &m("S1", &["Band 1"], "Greatest Hits"),
    );
    write_track(
        &root.join("Band 2/Greatest Hits/01.ogg"),
        "ogg",
        &m("S2", &["Band 2"], "Greatest Hits"),
    );
    // Album artist tag joins folders: one album.
    for n in 1..=2 {
        write_track(
            &root.join(format!("Loose/part{n}/0{n}.opus")),
            "opus",
            &Meta {
                album_artist: Some("Singer"),
                ..m("Part", &["Singer"], "Split")
            },
        );
    }
    // Untagged file: everything from the path.
    std::fs::create_dir_all(root.join("Band/Record (2001)")).unwrap();
    std::fs::write(root.join("Band/Record (2001)/03 - Song.wav"), wav()).unwrap();
    // Unreadable file: indexed from the path, reported.
    std::fs::create_dir_all(root.join("Broken")).unwrap();
    std::fs::write(root.join("Broken/bad.mp3"), b"garbage").unwrap();

    let mut conn = library();
    let (report, events) = run(&mut conn, root, 1_000);
    assert_eq!(report.files, 12);
    assert_eq!((report.read, report.failed), (11, 1));
    assert!(events.iter().any(
        |e| matches!(e, ScanEvent::TagsFailed { path, .. } if path.as_str() == "/Broken/bad.mp3")
    ));
    assert!(matches!(
        events.last(),
        Some(ScanEvent::TagsProgress {
            done: 12,
            total: 12
        })
    ));

    let one = &album_by_title(&conn, "Album One")[0];
    assert_eq!(artist_names(&conn, &one.artist_ids), ["Artist A"]);
    assert_eq!(one.year, Some(2001));
    assert!(!compilation(&conn, one.id));
    let art = music::album_art(&conn, one.id).unwrap().unwrap();
    assert_eq!(
        (art.kind, art.path.as_str()),
        (ArtKind::Folder, "/Artist A/Album One/Folder.JPG")
    );
    let tracks = music::album_tracks(&conn, one.id).unwrap();
    assert_eq!(
        tracks.iter().map(|t| t.title.as_str()).collect::<Vec<_>>(),
        ["First", "Second"]
    );
    assert_eq!(
        artist_names(&conn, &tracks[1].artist_ids),
        ["Artist A", "Guest"]
    );

    let hits = &album_by_title(&conn, "Hits")[0];
    assert_eq!(artist_names(&conn, &hits.artist_ids), [VARIOUS_ARTISTS]);
    assert!(compilation(&conn, hits.id));
    let art = music::album_art(&conn, hits.id).unwrap().unwrap();
    assert_eq!(
        (art.kind, art.path.as_str()),
        (ArtKind::Embedded, "/Mixes/Hits/01.mp3")
    );

    let best = album_by_title(&conn, "Best Of");
    assert_eq!(best.len(), 1);
    assert!(compilation(&conn, best[0].id));
    assert_eq!(artist_names(&conn, &best[0].artist_ids), [VARIOUS_ARTISTS]);
    let discs: i64 = conn
        .query_row(
            "SELECT disc_count FROM music_albums WHERE id = ?1",
            [best[0].id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(discs, 2);
    let tracks = music::album_tracks(&conn, best[0].id).unwrap();
    assert_eq!(
        tracks.iter().map(|t| t.disc_number).collect::<Vec<_>>(),
        [Some(1), Some(2)]
    );

    let greatest = album_by_title(&conn, "Greatest Hits");
    assert_eq!(greatest.len(), 2);
    assert!(greatest.iter().all(|a| !compilation(&conn, a.id)));
    assert_eq!(album_by_title(&conn, "Split").len(), 1);

    let record = &album_by_title(&conn, "Record")[0];
    assert_eq!(artist_names(&conn, &record.artist_ids), ["Band"]);
    let song = &music::album_tracks(&conn, record.id).unwrap()[0];
    assert_eq!((song.title.as_str(), song.track_number), ("Song", Some(3)));
    assert_eq!(song.duration_ms, Some(100));

    let failed: (String, i64) = conn
        .query_row(
            "SELECT t.title, t.tags_failed FROM music_tracks t JOIN files f ON f.id = t.file_id
             WHERE f.path = '/Broken/bad.mp3'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(failed, ("bad".to_owned(), 1));
    assert_eq!(
        music::genres(&conn).unwrap().len(),
        0,
        "no genre tags in this collection"
    );

    // A second run reads nothing.
    let (again, _) = run(&mut conn, root, 2_000);
    assert_eq!(again.files, 0);

    // Retagging moves a track; the emptied album and artist go away.
    let path = root.join("Band 2/Greatest Hits/01.ogg");
    write_track(&path, "ogg", &m("S2", &["Band 1"], "Elsewhere"));
    let f = std::fs::File::options().write(true).open(&path).unwrap();
    f.set_modified(std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(86_400))
        .unwrap();
    let (moved, _) = run(&mut conn, root, 3_000);
    assert_eq!(moved.files, 1);
    assert_eq!(album_by_title(&conn, "Greatest Hits").len(), 1);
    assert_eq!(album_by_title(&conn, "Elsewhere").len(), 1);
    assert!(moved.orphans.albums >= 1);
    assert!(
        music::artists(&conn)
            .unwrap()
            .iter()
            .all(|a| a.name != "Band 2")
    );
}

/// done when (MUS-001): a 5 000-track collection is indexed.
#[test]
fn indexes_five_thousand_tracks() {
    let dir = tempfile::tempdir().unwrap();
    let frames = mp3_frames(2);
    for a in 0..50 {
        for b in 0..10 {
            let album = format!("Album {b}");
            let artist = format!("Artist {a}");
            let folder = dir.path().join(&artist).join(&album);
            std::fs::create_dir_all(&folder).unwrap();
            for t in 1..=10u32 {
                let mut tag = Tag::new(TagType::Id3v2);
                fill_tag(
                    &mut tag,
                    &Meta {
                        title: &format!("Track {t}"),
                        artists: &[&artist],
                        album: Some(&album),
                        track: Some(t),
                        genres: &["Pop"],
                        ..Meta::default()
                    },
                );
                let mut bytes = Vec::new();
                tag.dump_to(&mut bytes, WriteOptions::default()).unwrap();
                bytes.extend_from_slice(&frames);
                std::fs::write(folder.join(format!("{t:02}.mp3")), bytes).unwrap();
            }
        }
    }
    let mut conn = library();
    let started = Instant::now();
    let (report, _) = run(&mut conn, dir.path(), 1_000);
    eprintln!("indexed {} tracks in {:?}", report.files, started.elapsed());
    assert_eq!((report.files, report.read, report.failed), (5000, 5000, 0));
    assert_eq!(music::albums(&conn).unwrap().len(), 500);
    assert_eq!(music::artists(&conn).unwrap().len(), 50);
    assert_eq!(music::genres(&conn).unwrap()[0].tracks, 5000);
    let (again, _) = run(&mut conn, dir.path(), 2_000);
    assert_eq!(again.files, 0);
}
