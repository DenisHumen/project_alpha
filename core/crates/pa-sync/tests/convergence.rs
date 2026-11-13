// SPDX-License-Identifier: MPL-2.0
//! SYNC-001 done-when: applying the same ops in any order (with duplicates)
//! gives one state. Small value domains force conflicting writes.

use pa_model::{MediaKey, MediaKind};
use pa_sync::{Hlc, Op, OpBody, OpId, SyncState};
use proptest::prelude::*;
use uuid::Uuid;

const PROFILES: [&str; 2] = ["default", "kid"];
const KEYS: [&str; 3] = ["tmdb:movie:603", "tmdb:episode:1399:1:1", "twitch:vod:123"];
const REFS: [&str; 2] = ["tmdb:tv:1399", "file:ab12"];

fn key(i: usize) -> MediaKey {
    KEYS[i].parse().unwrap()
}

fn body() -> impl Strategy<Value = OpBody> {
    let p = (0..PROFILES.len()).prop_map(|i| PROFILES[i].to_owned());
    let k = (0..KEYS.len()).prop_map(key);
    let r = (0..REFS.len()).prop_map(|i| REFS[i].to_owned());
    let pl = (0..2u8).prop_map(|i| format!("pl-{i}"));
    prop_oneof![
        (p.clone(), k.clone(), 0..4u64, proptest::option::of(0..2u64)).prop_map(
            |(profile, key, pos, dur)| OpBody::ProgressUpsert {
                profile,
                key,
                kind: MediaKind::Movie,
                position_ms: pos * 1000,
                duration_ms: dur,
            }
        ),
        (p.clone(), k.clone()).prop_map(|(profile, key)| OpBody::Watched { profile, key }),
        (p.clone(), k.clone()).prop_map(|(profile, key)| OpBody::Unwatch { profile, key }),
        (p.clone(), r.clone(), any::<bool>()).prop_map(|(profile, target, favorite)| {
            OpBody::Favorite {
                profile,
                target,
                favorite,
            }
        }),
        (p.clone(), r, proptest::option::of(0..=10u8)).prop_map(|(profile, target, rating)| {
            OpBody::Rating {
                profile,
                target,
                rating,
            }
        }),
        (p.clone(), pl.clone(), "[ab]", any::<bool>()).prop_map(
            |(profile, playlist, name, video)| OpBody::PlaylistUpsert {
                profile,
                playlist,
                name,
                video,
            }
        ),
        (p.clone(), pl.clone())
            .prop_map(|(profile, playlist)| OpBody::PlaylistDelete { profile, playlist }),
        (p.clone(), pl, proptest::collection::vec(k, 0..3)).prop_map(
            |(profile, playlist, items)| OpBody::PlaylistItems {
                profile,
                playlist,
                items,
            }
        ),
        ("rule-[12]", proptest::option::of("[0-9]")).prop_map(|(rule_key, rule)| {
            OpBody::FolderRule { rule_key, rule }
        }),
        ("player\\.[ab]", proptest::option::of("[0-9]"))
            .prop_map(|(key, value)| OpBody::Setting { key, value }),
        (p, "[12]", any::<bool>()).prop_map(|(profile, channel_id, followed)| {
            OpBody::TwitchFollow {
                profile,
                channel_id,
                followed,
            }
        }),
    ]
}

/// Ops from three devices with colliding wall times and counters.
fn ops() -> impl Strategy<Value = Vec<Op>> {
    proptest::collection::vec((0..3usize, 0..4u64, 0..2u32, body()), 0..40).prop_map(|raw| {
        raw.into_iter()
            .enumerate()
            .map(|(i, (dev, wall_ms, counter, body))| Op {
                op_id: OpId(Uuid::from_u128(i as u128 + 1)),
                hlc: Hlc {
                    wall_ms,
                    counter,
                    device: ["a", "b", "c"][dev].to_owned(),
                },
                body,
            })
            .collect()
    })
}

fn apply_all<'a>(ops: impl IntoIterator<Item = &'a Op>) -> SyncState {
    let mut s = SyncState::default();
    for op in ops {
        s.apply(op);
    }
    s
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    #[test]
    fn any_order_gives_one_state(
        (ops, shuffled) in ops().prop_flat_map(|o| (Just(o.clone()), Just(o).prop_shuffle()))
    ) {
        prop_assert_eq!(apply_all(&ops), apply_all(&shuffled));
    }

    #[test]
    fn duplicates_do_not_change_the_state(
        (ops, dups) in ops().prop_flat_map(|o| {
            let n = o.len();
            (Just(o), proptest::collection::vec(0..n.max(1), 0..20))
        })
    ) {
        let once = apply_all(&ops);
        let mut twice = once.clone();
        for i in dups.into_iter().filter(|i| *i < ops.len()) {
            prop_assert!(!twice.apply(&ops[i]));
        }
        prop_assert_eq!(once, twice);
    }

    #[test]
    fn replicas_converge_after_exchange(
        (ops, split) in ops().prop_flat_map(|o| {
            let n = o.len();
            (Just(o), proptest::collection::vec(any::<bool>(), n))
        })
    ) {
        // Each replica first sees its own subset, then the other's ops in
        // reverse order.
        let side = |left: bool| -> Vec<&Op> {
            ops.iter().zip(&split).filter(|(_, l)| **l == left).map(|(o, _)| o).collect()
        };
        let (mine, theirs) = (side(true), side(false));
        let mut a = apply_all(mine.iter().copied());
        let mut b = apply_all(theirs.iter().copied());
        for op in theirs.iter().rev() { a.apply(op); }
        for op in mine.iter().rev() { b.apply(op); }
        prop_assert_eq!(&a, &b);
        prop_assert_eq!(a, apply_all(&ops));
    }
}
