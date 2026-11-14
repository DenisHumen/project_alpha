// SPDX-License-Identifier: MPL-2.0
import 'package:flutter/foundation.dart';
import 'package:flutter/widgets.dart';

import '../play_request.dart';
import 'audio_profile.dart';

/// Repeat mode for a playlist.
enum QueueRepeat { off, all, one }

/// An audio or subtitle track as exposed to the UI.
@immutable
class MediaTrack {
  const MediaTrack({
    required this.id,
    this.title,
    this.language,
    this.codec,
    this.external = false,
    this.forced = false,
    this.hearingImpaired = false,
    this.isDefault = false,
  });

  /// Engine-specific id. `'no'` means "off" (subtitles).
  final String id;
  final String? title;
  final String? language;
  final String? codec;

  /// Loaded from a separate file.
  final bool external;

  /// Subtitles: forced (only the foreign-language lines).
  final bool forced;

  /// Subtitles: SDH / hearing impaired.
  final bool hearingImpaired;

  /// Marked as the default track (container flag or file name).
  final bool isDefault;

  static const off = MediaTrack(id: 'no', title: 'Off');

  bool get isOff => id == 'no';

  /// Human-readable label: "English · ac3", "Track 2", "Off".
  String get label {
    if (isOff) return 'Off';
    final parts = <String>[
      if (title != null && title!.isNotEmpty) title!,
      if (language != null && language!.isNotEmpty) language!,
    ];
    final head = parts.isEmpty ? 'Track $id' : parts.join(' · ');
    return codec == null ? head : '$head ($codec)';
  }

  @override
  bool operator ==(Object other) => other is MediaTrack && other.id == id;

  @override
  int get hashCode => id.hashCode;
}

/// Immutable snapshot of the player.
@immutable
class EngineState {
  const EngineState({
    this.position = Duration.zero,
    this.duration = Duration.zero,
    this.buffer = Duration.zero,
    this.playing = false,
    this.buffering = false,
    this.completed = false,
    this.rate = 1.0,
    this.volume = 100,
    this.playlistIndex = 0,
    this.audioTracks = const [],
    this.subtitleTracks = const [],
    this.audioTrack,
    this.subtitleTrack,
    this.videoWidth,
    this.videoHeight,
    this.info = const {},
    this.error,
    this.subtitleLines = const [],
    this.subtitleDelay = Duration.zero,
  });

  final Duration position;
  final Duration duration;
  final Duration buffer;
  final bool playing;
  final bool buffering;

  /// End of the media (or of the whole playlist) was reached.
  final bool completed;
  final double rate;

  /// 0..100.
  final double volume;

  /// Index of the current entry in the engine playlist.
  final int playlistIndex;
  final List<MediaTrack> audioTracks;

  /// Real subtitle tracks (without the synthetic "Off" entry).
  final List<MediaTrack> subtitleTracks;
  final MediaTrack? audioTrack;

  /// `null` or [MediaTrack.off] when subtitles are disabled.
  final MediaTrack? subtitleTrack;
  final int? videoWidth;
  final int? videoHeight;

  /// Technical summary for the info overlay (codec, resolution, ...).
  final Map<String, String> info;
  final String? error;

  /// Text of the subtitle shown now, one entry per line; drawn by the
  /// player's own overlay with the user's style (PLAY-007).
  final List<String> subtitleLines;

  /// Subtitle delay: positive shows subtitles later, negative earlier.
  final Duration subtitleDelay;

  EngineState copyWith({
    Duration? position,
    Duration? duration,
    Duration? buffer,
    bool? playing,
    bool? buffering,
    bool? completed,
    double? rate,
    double? volume,
    int? playlistIndex,
    List<MediaTrack>? audioTracks,
    List<MediaTrack>? subtitleTracks,
    MediaTrack? audioTrack,
    MediaTrack? subtitleTrack,
    int? videoWidth,
    int? videoHeight,
    Map<String, String>? info,
    String? error,
    List<String>? subtitleLines,
    Duration? subtitleDelay,
  }) {
    return EngineState(
      position: position ?? this.position,
      duration: duration ?? this.duration,
      buffer: buffer ?? this.buffer,
      playing: playing ?? this.playing,
      buffering: buffering ?? this.buffering,
      completed: completed ?? this.completed,
      rate: rate ?? this.rate,
      volume: volume ?? this.volume,
      playlistIndex: playlistIndex ?? this.playlistIndex,
      audioTracks: audioTracks ?? this.audioTracks,
      subtitleTracks: subtitleTracks ?? this.subtitleTracks,
      audioTrack: audioTrack ?? this.audioTrack,
      subtitleTrack: subtitleTrack ?? this.subtitleTrack,
      videoWidth: videoWidth ?? this.videoWidth,
      videoHeight: videoHeight ?? this.videoHeight,
      info: info ?? this.info,
      error: error ?? this.error,
      subtitleLines: subtitleLines ?? this.subtitleLines,
      subtitleDelay: subtitleDelay ?? this.subtitleDelay,
    );
  }
}

/// Playback engine behind `pa_player` screens. Production code uses
/// [MediaKitEngine]; tests use a fake so they don't need libmpv.
abstract class PlayerEngine {
  /// Current state; listeners fire on every change (incl. position ticks).
  ValueListenable<EngineState> get state;

  /// Replaces the playlist with [uris] and starts at [index] / [start].
  ///
  /// [autoLoadSubtitles]: let the engine pick up subtitle files next to a
  /// local file by itself. Off when the caller passes the sidecars from
  /// the library ([addSubtitle]), so they are not loaded twice.
  Future<void> open(
    List<String> uris, {
    int index = 0,
    Duration start = Duration.zero,
    bool play = true,
    bool autoLoadSubtitles = true,
  });

  Future<void> play();
  Future<void> pause();
  Future<void> seek(Duration position);
  Future<void> setRate(double rate);
  Future<void> setVolume(double volume);

  /// Jumps to playlist entry [index].
  Future<void> jump(int index);
  Future<void> setRepeat(QueueRepeat mode);

  /// Music audio mode: gapless and ReplayGain (MUS-002). Can be called at any
  /// time; changes apply to the playing track.
  Future<void> setAudioProfile(AudioProfile profile);
  Future<void> setAudioTrack(MediaTrack track);

  /// [MediaTrack.off] disables subtitles.
  Future<void> setSubtitleTrack(MediaTrack track);

  /// Loads and selects an external subtitle file.
  Future<void> addExternalSubtitle(String uri, {String? title});

  /// Loads an external subtitle of the current file without selecting it
  /// (unless [select]); it shows up in [EngineState.subtitleTracks] titled
  /// [ExternalSubtitle.title], with its language and flags.
  Future<void> addSubtitle(ExternalSubtitle subtitle, {bool select = false});

  /// Shifts subtitles: positive shows them later, negative earlier.
  Future<void> setSubtitleDelay(Duration delay);

  /// The video surface. Audio-only engines may return an empty box.
  Widget buildVideo(BuildContext context, {BoxFit fit = BoxFit.contain});

  Future<void> dispose();
}

/// Creates the engine used by a screen/controller.
typedef PlayerEngineFactory = PlayerEngine Function({bool video});
