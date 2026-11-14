// SPDX-License-Identifier: MPL-2.0
//
// Playing music from the music screens (MUS-004) and counting plays: a
// track counts as played when it becomes the current track of the music
// player, however it got there (tap, next, auto-advance).

import 'package:material_ui/material_ui.dart';

import '../../integration/library.dart';
import '../../integration/player.dart';
import '../../services/app_services.dart';

/// Counts plays of library tracks played by [player] in [music].
class MusicPlayRecorder {
  MusicPlayRecorder._(this._player, this._music) {
    _player.addListener(_onChange);
  }

  static final _recorders = Expando<MusicPlayRecorder>();

  /// The recorder of [services] (one per music player).
  static MusicPlayRecorder of(AppServices services) {
    final music = libraryMusic(services.library);
    final r = _recorders[services.music];
    if (r != null && identical(r._music, music)) return r;
    r?._player.removeListener(r._onChange);
    return _recorders[services.music] = MusicPlayRecorder._(
      services.music,
      music,
    );
  }

  final MusicPlayerController _player;
  final LibraryMusic _music;

  /// Track id by playback URI.
  final Map<String, String> _ids = {};
  String? _current;

  /// Makes the tracks known by their playback URI.
  void register(Iterable<MusicTrack> tracks) {
    for (final t in tracks) {
      _ids[t.item.playbackUri] = t.id;
    }
  }

  void _onChange() {
    final uri = _player.current?.uri;
    if (uri == _current) return;
    _current = uri;
    final id = uri == null ? null : _ids[uri];
    if (id != null) _music.recordTrackPlay(id).ignore();
  }
}

/// Plays [tracks] from [index]; [shuffle] plays them in a random order.
void playMusic(
  BuildContext context,
  List<MusicTrack> tracks, {
  int index = 0,
  bool shuffle = false,
}) {
  if (tracks.isEmpty) return;
  final services = Services.read(context);
  final list = shuffle ? ([...tracks]..shuffle()) : tracks;
  MusicPlayRecorder.of(services).register(list);
  services.music.play([
    for (final t in list)
      PlayRequest(
        uri: t.item.playbackUri,
        title: t.title,
        artist: t.artist,
        album: t.item.album,
        albumId: t.albumId,
        artworkUri: t.item.posterUrl,
        durationMs: t.item.durationMs,
      ),
  ], index: shuffle ? 0 : index);
}
