// SPDX-License-Identifier: MPL-2.0
//! Deterministic [`PlayerEngine`] for UI and core tests.
//!
//! Time does not pass by itself: [`MockEngine::advance`] moves a virtual
//! clock. Media are registered by URI ([`MockEngine::with_media`]); an
//! unknown URI fails to open with [`EngineError::Source`]. Everything else
//! follows the contract exactly, so tests of track policy (PLAY-004) and
//! playback sessions (PLAY-005) can run on it.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};

use crate::caps::EngineKind;
use crate::engine::{
    EngineError, EngineEvent, EngineListener, EngineState, MAX_VOLUME, MediaSource, OpenRequest,
    PlaybackStats, PlayerEngine, RepeatMode, Track, TrackKind,
};

/// How the mock buffers ahead of the position.
const READ_AHEAD_MS: u64 = 5_000;

/// A fake media file.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MockMedia {
    pub duration_ms: u64,
    /// `None` for audio-only media.
    pub video_size: Option<(u32, u32)>,
    pub video_codec: Option<String>,
    pub audio_tracks: Vec<Track>,
    pub subtitle_tracks: Vec<Track>,
}

#[derive(Default)]
struct Inner {
    media: HashMap<String, MockMedia>,
    state: EngineState,
    items: Vec<MediaSource>,
    /// Open item, `None` before a successful open.
    current: Option<MockMedia>,
    listener: Option<Arc<dyn EngineListener>>,
    /// Events produced under the lock, sent after it is released.
    pending: Vec<EngineEvent>,
    next_external: u32,
}

impl Inner {
    fn emit(&mut self, event: EngineEvent) {
        self.state.apply(&event);
        self.pending.push(event);
    }

    fn check_open(&self) -> Result<&MockMedia, EngineError> {
        if self.state.closed {
            return Err(EngineError::Closed);
        }
        self.current
            .as_ref()
            .ok_or_else(|| EngineError::state("no item is open"))
    }

    fn check_alive(&self) -> Result<(), EngineError> {
        if self.state.closed {
            Err(EngineError::Closed)
        } else {
            Ok(())
        }
    }

    fn position(&mut self, position_ms: u64) {
        let duration = self.state.duration_ms;
        let position_ms = position_ms.min(duration);
        self.emit(EngineEvent::Position {
            position_ms,
            buffered_ms: (position_ms + READ_AHEAD_MS).min(duration),
        });
    }

    fn set_playing(&mut self, playing: bool) {
        if self.state.playing != playing {
            self.emit(EngineEvent::PlayingChanged { playing });
        }
    }

    /// Opens `items[index]`; returns whether it loaded.
    fn load(&mut self, index: u32, start_ms: u64, play: bool) -> bool {
        self.emit(EngineEvent::Loading { index });
        let uri = &self.items[index as usize].uri;
        let Some(media) = self.media.get(uri).cloned() else {
            let error = EngineError::Source {
                details: format!("cannot open {uri}"),
            };
            self.current = None;
            self.emit(EngineEvent::Error { error });
            return false;
        };
        self.emit(EngineEvent::Loaded {
            duration_ms: media.duration_ms,
            video_width: media.video_size.map(|s| s.0),
            video_height: media.video_size.map(|s| s.1),
        });
        self.emit(EngineEvent::TracksChanged {
            audio: media.audio_tracks.clone(),
            subtitles: media.subtitle_tracks.clone(),
        });
        let audio = pick_default(&media.audio_tracks, true);
        let subtitle = pick_default(&media.subtitle_tracks, false);
        self.emit(EngineEvent::TrackSelected {
            kind: TrackKind::Audio,
            id: audio,
        });
        self.emit(EngineEvent::TrackSelected {
            kind: TrackKind::Subtitle,
            id: subtitle,
        });
        self.current = Some(media);
        self.position(start_ms);
        self.set_playing(play);
        true
    }

    fn select(&mut self, kind: TrackKind, id: Option<String>) -> Result<(), EngineError> {
        self.check_open()?;
        let tracks = match kind {
            TrackKind::Audio => &self.state.audio_tracks,
            TrackKind::Subtitle => &self.state.subtitle_tracks,
        };
        if let Some(id) = &id
            && !tracks.iter().any(|t| &t.id == id)
        {
            return Err(EngineError::argument(format!(
                "unknown {kind:?} track {id}"
            )));
        }
        self.emit(EngineEvent::TrackSelected { kind, id });
        Ok(())
    }

    /// End of the current item.
    fn on_item_end(&mut self) {
        let index = self.state.playlist_index;
        let last = index as usize + 1 >= self.items.len();
        match (self.state.repeat, last) {
            (RepeatMode::One, _) => self.position(0),
            (RepeatMode::Off, true) => {
                let end = self.state.duration_ms;
                self.position(end);
                self.emit(EngineEvent::Ended);
            }
            (RepeatMode::All, true) => {
                self.load(0, 0, true);
            }
            (_, false) => {
                self.load(index + 1, 0, true);
            }
        }
    }
}

/// Default selection: the container default, else the first audio track;
/// subtitles only if marked default or forced.
fn pick_default(tracks: &[Track], first_if_none: bool) -> Option<String> {
    tracks
        .iter()
        .find(|t| t.default || t.forced)
        .or(if first_if_none { tracks.first() } else { None })
        .map(|t| t.id.clone())
}

/// The mock engine. Cheap to share: clone the `Arc`.
#[derive(Default)]
pub struct MockEngine {
    inner: Mutex<Inner>,
}

impl std::fmt::Debug for MockEngine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MockEngine")
            .field("state", &self.lock().state)
            .finish_non_exhaustive()
    }
}

impl MockEngine {
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers a fake file under `uri`.
    pub fn with_media(self, uri: impl Into<String>, media: MockMedia) -> Self {
        self.lock().media.insert(uri.into(), media);
        self
    }

    /// Lets `ms` of wall time pass: the position moves by `ms × rate` while
    /// playing; reaching the end ends the item.
    pub fn advance(&self, ms: u64) {
        self.run(|inner| {
            if inner.current.is_none() || !inner.state.playing || inner.state.closed {
                return Ok(());
            }
            let step = (ms as f64 * inner.state.rate).round() as u64;
            let target = inner.state.position_ms.saturating_add(step);
            if target >= inner.state.duration_ms {
                inner.on_item_end();
            } else {
                inner.position(target);
            }
            Ok(())
        })
        .expect("advance never fails");
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Runs `f` under the lock, then sends the events it produced.
    fn run<T>(
        &self,
        f: impl FnOnce(&mut Inner) -> Result<T, EngineError>,
    ) -> Result<T, EngineError> {
        let (result, events, listener) = {
            let mut inner = self.lock();
            let result = f(&mut inner);
            (
                result,
                std::mem::take(&mut inner.pending),
                inner.listener.clone(),
            )
        };
        if let Some(listener) = listener {
            for event in events {
                listener.on_event(event);
            }
        }
        result
    }
}

impl PlayerEngine for MockEngine {
    fn kind(&self) -> EngineKind {
        EngineKind::Mock
    }

    fn set_listener(&self, listener: Option<Arc<dyn EngineListener>>) {
        self.lock().listener = listener;
    }

    fn state(&self) -> EngineState {
        self.lock().state.clone()
    }

    fn open(&self, request: OpenRequest) -> Result<(), EngineError> {
        self.run(|inner| {
            inner.check_alive()?;
            if request.items.get(request.index as usize).is_none() {
                return Err(EngineError::argument("index outside the playlist"));
            }
            inner.items = request.items;
            inner.load(request.index, request.start_position_ms, request.play);
            Ok(())
        })
    }

    fn play(&self) -> Result<(), EngineError> {
        self.run(|inner| {
            inner.check_open()?;
            if inner.state.completed {
                inner.position(0);
            }
            inner.set_playing(true);
            Ok(())
        })
    }

    fn pause(&self) -> Result<(), EngineError> {
        self.run(|inner| {
            inner.check_open()?;
            inner.set_playing(false);
            Ok(())
        })
    }

    fn seek(&self, position_ms: u64) -> Result<(), EngineError> {
        self.run(|inner| {
            inner.check_open()?;
            inner.position(position_ms);
            Ok(())
        })
    }

    fn set_rate(&self, rate: f64) -> Result<(), EngineError> {
        self.run(|inner| {
            inner.check_alive()?;
            if !rate.is_finite() || rate <= 0.0 {
                return Err(EngineError::argument(format!("rate {rate}")));
            }
            inner.emit(EngineEvent::RateChanged { rate });
            Ok(())
        })
    }

    fn set_volume(&self, volume: f64) -> Result<(), EngineError> {
        self.run(|inner| {
            inner.check_alive()?;
            if !volume.is_finite() || volume < 0.0 {
                return Err(EngineError::argument(format!("volume {volume}")));
            }
            inner.emit(EngineEvent::VolumeChanged {
                volume: volume.min(MAX_VOLUME),
            });
            Ok(())
        })
    }

    fn set_repeat(&self, repeat: RepeatMode) -> Result<(), EngineError> {
        self.run(|inner| {
            inner.check_alive()?;
            inner.emit(EngineEvent::RepeatChanged { repeat });
            Ok(())
        })
    }

    fn jump(&self, index: u32) -> Result<(), EngineError> {
        self.run(|inner| {
            inner.check_alive()?;
            if inner.items.is_empty() {
                return Err(EngineError::state("no playlist"));
            }
            if index as usize >= inner.items.len() {
                return Err(EngineError::argument("index outside the playlist"));
            }
            let play = inner.state.playing;
            inner.load(index, 0, play);
            Ok(())
        })
    }

    fn set_audio_track(&self, id: Option<String>) -> Result<(), EngineError> {
        self.run(|inner| inner.select(TrackKind::Audio, id))
    }

    fn set_subtitle_track(&self, id: Option<String>) -> Result<(), EngineError> {
        self.run(|inner| inner.select(TrackKind::Subtitle, id))
    }

    fn add_external_subtitle(&self, uri: String, title: Option<String>) -> Result<(), EngineError> {
        self.run(|inner| {
            inner.check_open()?;
            if uri.is_empty() {
                return Err(EngineError::argument("empty subtitle uri"));
            }
            inner.next_external += 1;
            let track = Track {
                id: format!("ext{}", inner.next_external),
                title: title.or(Some(uri)),
                external: true,
                ..Track::default()
            };
            let id = track.id.clone();
            let audio = inner.state.audio_tracks.clone();
            let mut subtitles = inner.state.subtitle_tracks.clone();
            subtitles.push(track);
            inner.emit(EngineEvent::TracksChanged { audio, subtitles });
            inner.emit(EngineEvent::TrackSelected {
                kind: TrackKind::Subtitle,
                id: Some(id),
            });
            Ok(())
        })
    }

    fn stats(&self) -> Result<PlaybackStats, EngineError> {
        let inner = self.lock();
        let media = inner.check_open()?;
        let audio_codec = inner
            .state
            .audio_track
            .as_ref()
            .and_then(|id| media.audio_tracks.iter().find(|t| &t.id == id))
            .and_then(|t| t.codec.clone());
        Ok(PlaybackStats {
            video_codec: media.video_codec.clone(),
            audio_codec,
            hw_decoding: Some(false),
            audio_passthrough: Some(false),
            dropped_frames: 0,
            bitrate_bps: None,
            buffered_ms: inner.state.buffered_ms,
        })
    }

    fn close(&self) -> Result<(), EngineError> {
        self.run(|inner| {
            if !inner.state.closed {
                inner.current = None;
                inner.emit(EngineEvent::Closed);
            }
            Ok(())
        })
    }
}
