// SPDX-License-Identifier: MPL-2.0
/// Android player (PLAY-AND-002, ADR-0008, ADR-0017).
///
/// Video plays in a native `PlayerActivity`: Media3/ExoPlayer renders into a
/// `SurfaceView` (HDR and frame-rate switching stay intact), and the OSD is a
/// second Flutter engine (`osdMain` by default) drawn transparently above it.
///
/// - In the app: [PaPlayerAndroid.open] starts playback and completes with
///   the final engine state when the player closes (for resume);
///   [PaPlayerAndroid.prewarmOsd] starts the OSD engine ahead of time (e.g.
///   when a detail page opens) so the player starts without a stall.
/// - In the OSD entrypoint: [PlayerOsdBridge] sends commands of the
///   `PlayerEngine` contract and receives its events and state.
library;

export 'src/osd_bridge.dart';
export 'src/player.dart';
