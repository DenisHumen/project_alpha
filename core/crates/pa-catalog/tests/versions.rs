// SPDX-License-Identifier: MPL-2.0
//! LIB-010 end to end on a real folder: several files of one film are one
//! card with versions and a default version; favorites, collections and
//! video playlists over the scanned items.

use std::path::Path;

use pa_catalog::{Catalog, ItemKind, ItemRef};
use pa_scanner::CancelToken;

fn media(root: &Path, rel: &str, seed: u8) {
    let path = root.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let data: Vec<u8> = (0..150_000u32)
        .map(|i| (i as u8).wrapping_mul(31).wrapping_add(seed))
        .collect();
    std::fs::write(path, data).unwrap();
}

const HD: &str = "Movies/Inception (2010)/Inception.2010.1080p.BluRay.mkv";
const UHD: &str = "Movies/Inception (2010)/Inception.2010.2160p.HDR10.WEB-DL.mkv";
const CUT: &str = "Movies/Blade Runner (1982)/Blade.Runner.1982.Directors.Cut.1080p.mkv";
const THEATRICAL: &str = "Movies/Blade Runner 1982/Blade.Runner.1982.720p.mkv";

fn setup() -> (tempfile::TempDir, Catalog) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("media");
    media(&root, HD, 1);
    media(&root, UHD, 2);
    media(&root, CUT, 3);
    media(&root, THEATRICAL, 4);
    media(&root, "Movies/Arrival (2016)/Arrival.2016.1080p.mkv", 5);
    media(&root, "Shows/Dark/Season 1/Dark.S01E01.1080p.mkv", 6);
    media(&root, "Shows/Dark/Season 1/Dark.S01E02.1080p.mkv", 7);
    let cat = Catalog::open(dir.path().join("library.db")).unwrap();
    cat.add_source(None, "local", "Media", root.to_str().unwrap(), None)
        .unwrap();
    cat.scan(None, &CancelToken::new(), &mut |_| {}).unwrap();
    (dir, cat)
}

fn movie<'a>(movies: &'a [pa_catalog::Item], title: &str) -> &'a pa_catalog::Item {
    movies
        .iter()
        .find(|m| m.title == title)
        .unwrap_or_else(|| panic!("no {title} in {movies:?}"))
}

#[test]
fn two_files_of_one_film_are_one_card_with_versions() {
    let (_dir, cat) = setup();
    let movies = cat.movies().unwrap();
    let titles: Vec<_> = movies.iter().map(|m| m.title.as_str()).collect();
    assert_eq!(titles, ["Arrival", "Blade Runner", "Inception"], "{movies:?}");

    let inception = movie(&movies, "Inception");
    assert_eq!(inception.version_count, 2);
    assert_eq!(movie(&movies, "Arrival").version_count, 1);
    // Two folders of one film are versions too.
    let blade = movie(&movies, "Blade Runner");
    assert_eq!(blade.version_count, 2);

    // Best resolution first; the item plays its first file until a default
    // is chosen.
    let versions = cat.versions(inception.id).unwrap();
    let labels: Vec<String> = versions.iter().map(|v| v.label()).collect();
    assert_eq!(labels, ["4K · HDR10", "1080p"], "{versions:?}");
    assert_eq!(versions[0].rel_path, UHD);
    assert_eq!(versions[1].source.as_deref(), Some("BluRay"));
    let default: Vec<_> = versions.iter().filter(|v| v.is_default).collect();
    assert_eq!(default.len(), 1);
    assert_eq!(default[0].rel_path, inception.rel_path);
    assert!(versions.iter().all(|v| v.local_path.is_some()));

    let cut = cat.versions(blade.id).unwrap();
    assert!(
        cut.iter().any(|v| v.rel_path == CUT
            && v.edition.as_deref().is_some_and(|e| e.contains("Cut"))),
        "{cut:?}"
    );

    // Choosing the 4K version: the card plays it, still one card.
    cat.set_default_version(inception.id, Some(versions[0].file_id))
        .unwrap();
    let item = cat.item(inception.id).unwrap().unwrap();
    assert_eq!(item.rel_path, UHD);
    assert_eq!(item.version_count, 2);
    assert_eq!(
        cat.movies()
            .unwrap()
            .iter()
            .filter(|m| m.title == "Inception")
            .count(),
        1
    );
    let after = cat.versions(inception.id).unwrap();
    assert!(after[0].is_default && !after[1].is_default);
    // Queries (LIB-009) show the same card.
    let page = cat
        .query(
            &pa_catalog::LibraryQuery {
                kind: ItemKind::Movie,
                filter: pa_catalog::LibraryFilter::default(),
                sort: pa_catalog::LibrarySort::default(),
            },
            0,
            10,
        )
        .unwrap();
    let card = page.items.iter().find(|i| i.id == inception.id).unwrap();
    assert_eq!((card.rel_path.as_str(), card.version_count), (UHD, 2));

    // A file of another film is refused; clearing goes back to the first.
    let other = cat.versions(blade.id).unwrap()[0].file_id;
    assert!(cat.set_default_version(inception.id, Some(other)).is_err());
    assert!(
        cat.set_default_version(ItemRef::Show(1), None).is_err(),
        "defaults are per movie"
    );
    cat.set_default_version(inception.id, None).unwrap();
    assert_eq!(
        cat.item(inception.id).unwrap().unwrap().rel_path,
        inception.rel_path
    );

    // Progress belongs to the film, whatever version plays.
    assert!(
        cat.save_progress(inception.id, 60_000, 8_880_000, 1, false)
            .unwrap()
    );
    cat.set_default_version(inception.id, Some(versions[0].file_id))
        .unwrap();
    assert_eq!(
        cat.progress(inception.id).unwrap().unwrap().position_ms,
        60_000
    );
}

#[test]
fn favorites_pin_to_home_and_survive_a_rescan() {
    let (_dir, cat) = setup();
    let movies = cat.movies().unwrap();
    let arrival = movie(&movies, "Arrival").id;
    let dark = cat.shows().unwrap()[0].id;
    let e1 = cat.episodes(dark).unwrap()[0].id;

    assert!(!cat.is_favorite(arrival).unwrap());
    assert!(cat.set_favorite(arrival, true).unwrap());
    assert!(!cat.set_favorite(arrival, true).unwrap());
    assert!(cat.set_favorite(dark, true).unwrap());
    assert!(cat.set_favorite(e1, true).unwrap());
    let ids: Vec<ItemRef> = cat.favorites(false).unwrap().iter().map(|i| i.id).collect();
    assert_eq!(ids.len(), 3);
    assert!(ids.contains(&arrival) && ids.contains(&dark) && ids.contains(&e1));

    assert!(cat.set_favorite_pinned(e1, false).unwrap());
    let pinned: Vec<ItemRef> = cat.favorites(true).unwrap().iter().map(|i| i.id).collect();
    assert!(!pinned.contains(&e1) && pinned.contains(&arrival));

    cat.scan(None, &CancelToken::new(), &mut |_| {}).unwrap();
    assert_eq!(cat.favorites(false).unwrap().len(), 3);

    assert!(cat.set_favorite(arrival, false).unwrap());
    assert!(!cat.is_favorite(arrival).unwrap());
}

#[test]
fn collections_and_video_playlists_keep_their_order() {
    let (_dir, cat) = setup();
    let movies = cat.movies().unwrap();
    let arrival = movie(&movies, "Arrival").id;
    let inception = movie(&movies, "Inception").id;
    let dark = cat.shows().unwrap()[0].id;
    let episodes: Vec<ItemRef> = cat.episodes(dark).unwrap().iter().map(|e| e.id).collect();

    let c = cat.create_collection("Nolan & co").unwrap();
    assert!(cat.add_to_collection(c.id, inception).unwrap());
    assert!(cat.add_to_collection(c.id, arrival).unwrap());
    assert!(!cat.add_to_collection(c.id, arrival).unwrap());
    let ids: Vec<_> = cat
        .collection_items(c.id)
        .unwrap()
        .iter()
        .map(|i| i.id)
        .collect();
    assert_eq!(ids, [inception, arrival]);
    assert_eq!(cat.collections().unwrap()[0].item_count, 2);
    assert!(cat.remove_from_collection(c.id, inception).unwrap());
    cat.rename_collection(c.id, "Villeneuve").unwrap();
    assert_eq!(cat.item_collections(arrival).unwrap()[0].name, "Villeneuve");
    cat.delete_collection(c.id).unwrap();
    assert!(cat.collections().unwrap().is_empty());
    assert!(cat.collection_items(c.id).is_err());

    let p = cat.create_playlist("Weekend").unwrap();
    assert_eq!(cat.add_to_playlist(p.id, inception).unwrap(), 1);
    assert_eq!(cat.add_to_playlist(p.id, dark).unwrap(), 2, "every episode");
    assert_eq!(cat.add_to_playlist(p.id, episodes[0]).unwrap(), 0, "once");
    assert_eq!(cat.add_to_playlist(p.id, arrival).unwrap(), 1);
    let order = |cat: &Catalog| -> Vec<ItemRef> {
        cat.playlist_items(p.id)
            .unwrap()
            .iter()
            .map(|i| i.id)
            .collect()
    };
    assert_eq!(order(&cat), [inception, episodes[0], episodes[1], arrival]);
    cat.reorder_playlist(p.id, &[arrival, episodes[1]]).unwrap();
    assert_eq!(order(&cat), [arrival, episodes[1], inception, episodes[0]]);
    assert_eq!(cat.remove_from_playlist(p.id, inception).unwrap(), 1);
    assert_eq!(cat.playlists().unwrap()[0].item_count, 3);
    cat.rename_playlist(p.id, "Sunday").unwrap();
    assert_eq!(cat.playlists().unwrap()[0].name, "Sunday");
    cat.delete_playlist(p.id).unwrap();
    assert!(cat.playlists().unwrap().is_empty());
}
