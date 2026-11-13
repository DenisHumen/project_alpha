// SPDX-License-Identifier: MPL-2.0
import 'package:flutter/foundation.dart';

import '../play_request.dart';

/// Which ReplayGain value the engine applies (MUS-002).
enum ReplayGainMode {
  /// No loudness normalisation.
  off,

  /// Per-track gain: every track at the same loudness (shuffle, mixes).
  track,

  /// Per-album gain: keeps the loudness differences inside an album.
  album,

  /// `album` while an album plays in order, `track` otherwise
  /// (resolved by [AudioProfile.resolveFor]; engines treat it as `track`).
  auto,
}

/// Audio-mode settings for music playback (MUS-002, research 02 §5.2,
/// variant A: libmpv in audio mode). Engine-agnostic: [MediaKitEngine] maps
/// it to mpv properties with [mpvProperties]; a future Media3 or own engine
/// (MUS-008) maps the same fields.
///
/// The gain values themselves come from the files' tags (`REPLAYGAIN_*`, and
/// `R128_*` for Opus) — the same tags MUS-001 stores in `music_tracks.rg_*`.
@immutable
class AudioProfile {
  const AudioProfile({
    this.gapless = true,
    this.replayGain = ReplayGainMode.auto,
    this.preampDb = 0,
    this.preventClipping = true,
    this.fallbackDb = 0,
    this.exclusive = false,
  });

  /// Plays the next track without re-opening the audio output and decodes
  /// it ahead of time (`gapless-audio`, `prefetch-playlist`).
  final bool gapless;
  final ReplayGainMode replayGain;

  /// Added to the ReplayGain value, dB (-15..15).
  final double preampDb;

  /// Lowers the gain so that `gain × peak` does not exceed full scale.
  final bool preventClipping;

  /// Gain for files without ReplayGain tags, dB (-200..60).
  final double fallbackDb;

  /// Exclusive output (WASAPI exclusive on Windows; ignored elsewhere by mpv
  /// for now — real bit-perfect output is MUS-009).
  final bool exclusive;

  AudioProfile copyWith({
    bool? gapless,
    ReplayGainMode? replayGain,
    double? preampDb,
    bool? preventClipping,
    double? fallbackDb,
    bool? exclusive,
  }) => AudioProfile(
    gapless: gapless ?? this.gapless,
    replayGain: replayGain ?? this.replayGain,
    preampDb: preampDb ?? this.preampDb,
    preventClipping: preventClipping ?? this.preventClipping,
    fallbackDb: fallbackDb ?? this.fallbackDb,
    exclusive: exclusive ?? this.exclusive,
  );

  /// Replaces [ReplayGainMode.auto] by `album` when [order] (the play order)
  /// is one album in order — every track has the same album key
  /// ([PlayRequest.albumId], else [PlayRequest.album]) and [shuffled] is
  /// false — and by `track` otherwise.
  AudioProfile resolveFor(List<PlayRequest> order, {required bool shuffled}) {
    if (replayGain != ReplayGainMode.auto) return this;
    return copyWith(
      replayGain: !shuffled && isSingleAlbum(order)
          ? ReplayGainMode.album
          : ReplayGainMode.track,
    );
  }

  /// True when all [tracks] share one non-empty album key.
  static bool isSingleAlbum(List<PlayRequest> tracks) {
    String? key(PlayRequest t) {
      final k = t.albumId ?? t.album;
      return k == null || k.isEmpty ? null : k;
    }

    if (tracks.isEmpty) return false;
    final first = key(tracks.first);
    return first != null && tracks.every((t) => key(t) == first);
  }

  /// mpv properties (all of them can change while playing).
  Map<String, String> get mpvProperties => {
    'gapless-audio': gapless ? 'yes' : 'no',
    'prefetch-playlist': gapless ? 'yes' : 'no',
    'replaygain': switch (replayGain) {
      ReplayGainMode.off => 'no',
      ReplayGainMode.track || ReplayGainMode.auto => 'track',
      ReplayGainMode.album => 'album',
    },
    'replaygain-preamp': _db(preampDb.clamp(-15, 15)),
    // mpv: "yes" *allows* clipping; "no" lowers the gain to avoid it.
    'replaygain-clip': preventClipping ? 'no' : 'yes',
    'replaygain-fallback': _db(fallbackDb.clamp(-200, 60)),
    'audio-exclusive': exclusive ? 'yes' : 'no',
  };

  static String _db(num v) => v.toStringAsFixed(2);

  @override
  bool operator ==(Object other) =>
      other is AudioProfile &&
      other.gapless == gapless &&
      other.replayGain == replayGain &&
      other.preampDb == preampDb &&
      other.preventClipping == preventClipping &&
      other.fallbackDb == fallbackDb &&
      other.exclusive == exclusive;

  @override
  int get hashCode => Object.hash(
    gapless,
    replayGain,
    preampDb,
    preventClipping,
    fallbackDb,
    exclusive,
  );

  @override
  String toString() => 'AudioProfile($mpvProperties)';
}
