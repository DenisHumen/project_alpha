// SPDX-License-Identifier: MPL-2.0
//! MUS-004 on a real folder: music tracks with album, format and play
//! counts; plays; audio playlists.

use std::path::Path;

use pa_catalog::{Catalog, ItemRef};
use pa_library::rusqlite::Connection;
use pa_scanner::CancelToken;

fn media(root: &Path, rel: &str, seed: u8) {
    let path = root.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let data: Vec<u8> = (0..150_000u32)
        .map(|i| (i as u8).wrapping_mul(31).wrapping_add(seed))
        .collect();
    std::fs::write(path, data).unwrap();
}

fn setup() -> (tempfile::TempDir, Catalog) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("media");
    media(&root, "Music/Artist/Album/01 - One.flac", 1);
    media(&root, "Music/Artist/Album/02 - Two.flac", 2);
    media(&root, "Music/Other/Single/01 - Three.mp3", 3);
    let cat = Catalog::open(dir.path().join("library.db")).unwrap();
    cat.add_source(None, "local", "Media", root.to_str().unwrap(), None)
        .unwrap();
    cat.scan(None, &CancelToken::new(), &mut |_| {}).unwrap();
    (dir, cat)
}

#[test]
fn tracks_carry_format_album_and_plays() {
    let (dir, cat) = setup();
    // The fake files have no readable tags: give one a format as a tag
    // reader would.
    let db = Connection::open(dir.path().join("library.db")).unwrap();
    db.execute_batch(
        "UPDATE music_tracks SET codec = 'flac', lossless = 1, sample_rate_hz = 96000,
                bit_depth = 24, channels = 2, disc_number = 1, disc_total = 2
         WHERE title LIKE '%One%';",
    )
    .unwrap();

    let tracks = cat.music_tracks().unwrap();
    assert_eq!(tracks.len(), 3, "{tracks:?}");
    assert_eq!(tracks.len(), cat.tracks().unwrap().len());
    let one = tracks
        .iter()
        .find(|t| t.item.title.contains("One"))
        .unwrap_or_else(|| panic!("{tracks:?}"));
    assert_eq!(one.codec.as_deref(), Some("flac"));
    assert_eq!(one.lossless, Some(true));
    assert_eq!(one.sample_rate_hz, Some(96_000));
    assert_eq!(one.bit_depth, Some(24));
    assert_eq!(one.disc_total, Some(2));
    assert!(one.local);
    assert!(one.album_id.is_some());
    assert_eq!(one.play_count, 0);
    assert_eq!(one.last_played_at_ms, None);
    // The two tracks of the album folder share it.
    let two = tracks.iter().find(|t| t.item.title.contains("Two")).unwrap();
    assert_eq!(two.album_id, one.album_id);

    // Plays are counted per track and shown by the next query.
    assert_eq!(cat.record_track_play(one.item.id).unwrap(), 1);
    assert_eq!(cat.record_track_play(one.item.id).unwrap(), 2);
    assert_eq!(cat.record_track_play(two.item.id).unwrap(), 1);
    let tracks = cat.music_tracks().unwrap();
    let one = tracks.iter().find(|t| t.item.id == one.item.id).unwrap();
    assert_eq!(one.play_count, 2);
    assert!(one.last_played_at_ms.is_some());
    assert!(cat.record_track_play(ItemRef::Movie(1)).is_err());

    // A rescan keeps the counts (they are keyed by the file fingerprint).
    cat.scan(None, &CancelToken::new(), &mut |_| {}).unwrap();
    let again = cat.music_tracks().unwrap();
    let one_again = again.iter().find(|t| t.item.id == one.item.id).unwrap();
    assert_eq!(one_again.play_count, 2);
}

#[test]
fn audio_playlists_hold_tracks() {
    let (_dir, cat) = setup();
    let tracks = cat.tracks().unwrap();
    let p = cat.create_music_playlist("  Road trip ").unwrap();
    assert_eq!(p.name, "Road trip");
    // Audio playlists are not video playlists.
    assert!(cat.playlists().unwrap().is_empty());
    assert_eq!(cat.music_playlists().unwrap().len(), 1);

    assert!(cat.add_to_music_playlist(p.id, tracks[2].id).unwrap());
    assert!(cat.add_to_music_playlist(p.id, tracks[0].id).unwrap());
    assert!(!cat.add_to_music_playlist(p.id, tracks[0].id).unwrap());
    let items = cat.music_playlist_items(p.id).unwrap();
    let ids: Vec<_> = items.iter().map(|i| i.id).collect();
    assert_eq!(ids, [tracks[2].id, tracks[0].id]);
    assert_eq!(cat.music_playlists().unwrap()[0].item_count, 2);

    assert!(cat.remove_from_music_playlist(p.id, tracks[2].id).unwrap());
    assert_eq!(cat.music_playlist_items(p.id).unwrap().len(), 1);
    cat.rename_music_playlist(p.id, "Drive").unwrap();
    assert_eq!(cat.music_playlists().unwrap()[0].name, "Drive");

    // A video playlist id is not an audio playlist.
    let video = cat.create_playlist("Movies").unwrap();
    assert!(cat.music_playlist_items(video.id).is_err());
    assert!(cat.add_to_music_playlist(p.id, ItemRef::Movie(1)).is_err());

    cat.delete_music_playlist(p.id).unwrap();
    assert!(cat.music_playlists().unwrap().is_empty());
}
