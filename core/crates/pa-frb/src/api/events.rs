// SPDX-License-Identifier: MPL-2.0
//! Test stream: checks that Rust → Dart streams work end to end.
//!
//! Real event streams (scan progress, library changes) follow the same
//! pattern: an async function that takes a [`StreamSink`] and returns when
//! the stream is complete; returning closes the Dart stream.

use crate::frb_generated::StreamSink;

/// Emits `count` ticks `0, 1, …, count - 1` into `sink`, then closes the stream.
///
/// Stops early if Dart has cancelled the subscription.
pub async fn tick_stream(count: u32, sink: StreamSink<u32>) {
    for tick in ticks(count) {
        if sink.add(tick).is_err() {
            break;
        }
    }
}

fn ticks(count: u32) -> impl Iterator<Item = u32> {
    0..count
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ticks_count_up_from_zero() {
        assert_eq!(ticks(3).collect::<Vec<_>>(), vec![0, 1, 2]);
        assert_eq!(ticks(0).count(), 0);
    }
}
