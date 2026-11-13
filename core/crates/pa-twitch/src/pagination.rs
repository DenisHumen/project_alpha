// SPDX-License-Identifier: MPL-2.0
//! Cursor pagination with de-duplication (06 §1.4: lists "float").

use std::collections::HashSet;
use std::future::Future;

use serde::{Deserialize, Serialize};

use crate::error::Result;

/// Largest `first` Helix accepts on list endpoints.
pub const MAX_PAGE_SIZE: u32 = 100;

/// Opaque Helix pagination cursor.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Cursor(pub String);

/// One page of a list endpoint.
#[derive(Debug, Clone, PartialEq)]
pub struct Page<T> {
    /// Items of this page.
    pub items: Vec<T>,
    /// Cursor of the next page; `None` on the last one.
    pub next: Option<Cursor>,
    /// Total count, for endpoints that report it (`channels/followed`).
    pub total: Option<u64>,
}

/// Items with a stable Helix id, used to drop duplicates across pages.
pub trait HasId {
    /// Stable id of the item.
    fn id(&self) -> &str;
}

/// Follows cursors until the list ends or `limit` unique items are
/// collected. Duplicates (same [`HasId::id`]) are dropped, and a cursor seen
/// twice or an empty page ends the walk, so a misbehaving API cannot loop us.
pub async fn collect_pages<T, F, Fut>(limit: usize, mut fetch: F) -> Result<Vec<T>>
where
    T: HasId,
    F: FnMut(Option<Cursor>) -> Fut,
    Fut: Future<Output = Result<Page<T>>>,
{
    let mut out = Vec::new();
    let mut seen_ids = HashSet::new();
    let mut seen_cursors = HashSet::new();
    let mut cursor = None;
    while out.len() < limit {
        let page = fetch(cursor.take()).await?;
        let empty = page.items.is_empty();
        for item in page.items {
            if out.len() >= limit {
                break;
            }
            if seen_ids.insert(item.id().to_owned()) {
                out.push(item);
            }
        }
        match page.next {
            Some(next) if !empty && seen_cursors.insert(next.clone()) => cursor = Some(next),
            _ => break,
        }
    }
    Ok(out)
}
