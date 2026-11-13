// SPDX-License-Identifier: MPL-2.0
import 'dart:async';
import 'dart:io' show File, Platform;

import 'package:flutter/foundation.dart';
import 'package:flutter/widgets.dart';
import 'package:media_kit/media_kit.dart' as mk;
import 'package:media_kit_video/media_kit_video.dart' as mkv;

import 'audio_profile.dart';
import 'player_engine.dart';

/// Initialises libmpv bindings. Call once in `main()` before `runApp`.
void ensurePaPlayerInitialized() =>
    mk.MediaKit.ensureInitialized(libmpv: bundledLibmpvPath());

/// The LGPL libmpv shipped inside the Linux bundle (`lib/libmpv.so.2`, put
/// there by tools/mpv/bundle.sh, PLAY-002), or null to let media_kit search
/// (LIBMPV_LIBRARY_PATH, then the system). Opening it by path keeps a system
/// libmpv from being loaded next to the bundled one.
String? bundledLibmpvPath() {
  if (kIsWeb || !Platform.isLinux) return null;
  if (Platform.environment['LIBMPV_LIBRARY_PATH'] != null) return null;
  final path =
      '${File(Platform.resolvedExecutable).parent.path}/lib/libmpv.so.2';
  return File(path).existsSync() ? path : null;
}

/// [PlayerEngine] on media_kit / libmpv (ADR-0012).
class MediaKitEngine implements PlayerEngine {
  MediaKitEngine({bool video = true, this.mpvOptions = const {}})
    : _player = mk.Player(
        configuration: const mk.PlayerConfiguration(
          title: 'Kinolune',
          logLevel: mk.MPVLogLevel.warn,
        ),
      ) {
    if (video) _controller = mkv.VideoController(_player);
    _subscribe();
    _ready = _configure(video: video);
  }

  /// Factory matching [PlayerEngineFactory].
  static PlayerEngine create({bool video = true}) =>
      MediaKitEngine(video: video, mpvOptions: defaultMpvOptions);

  /// Extra mpv properties applied to every engine made by [create]
  /// (diagnostics, e.g. `{'ao': 'null'}` on a headless machine).
  static Map<String, String> defaultMpvOptions = const {};

  /// Extra mpv properties for this engine.
  final Map<String, String> mpvOptions;

  final mk.Player _player;
  mkv.VideoController? _controller;
  final ValueNotifier<EngineState> _state = ValueNotifier(const EngineState());
  final List<StreamSubscription<Object?>> _subs = [];
  bool _disposed = false;
  late final Future<void> _ready;

  /// Direct access for advanced use (diagnostics, app-specific mpv options).
  mk.Player get player => _player;

  @override
  ValueListenable<EngineState> get state => _state;

  void _set(EngineState s) {
    if (!_disposed) _state.value = s;
  }

  Future<void> _configure({required bool video}) async {
    final p = _player.platform;
    if (p is! mk.NativePlayer) return;
    try {
      // Gapless playback for music queues (mpv: "weak" by default).
      await p.setProperty('gapless-audio', 'yes');
      await p.setProperty('prefetch-playlist', 'yes');
      // External subtitles next to the file are picked up automatically.
      await p.setProperty('sub-auto', 'fuzzy');
      if (!video) await p.setProperty('vid', 'no');
      for (final e in mpvOptions.entries) {
        await p.setProperty(e.key, e.value);
      }
    } catch (e) {
      debugPrint('pa_player: mpv option failed: $e');
    }
  }

  void _subscribe() {
    final s = _player.stream;
    _subs
      ..add(s.position.listen((v) => _set(_state.value.copyWith(position: v))))
      ..add(s.duration.listen((v) => _set(_state.value.copyWith(duration: v))))
      ..add(s.buffer.listen((v) => _set(_state.value.copyWith(buffer: v))))
      ..add(s.playing.listen((v) => _set(_state.value.copyWith(playing: v))))
      ..add(
        s.buffering.listen((v) => _set(_state.value.copyWith(buffering: v))),
      )
      ..add(
        s.completed.listen((v) => _set(_state.value.copyWith(completed: v))),
      )
      ..add(s.rate.listen((v) => _set(_state.value.copyWith(rate: v))))
      ..add(s.volume.listen((v) => _set(_state.value.copyWith(volume: v))))
      ..add(
        s.playlist.listen(
          (v) => _set(_state.value.copyWith(playlistIndex: v.index)),
        ),
      )
      ..add(s.width.listen((v) => _set(_state.value.copyWith(videoWidth: v))))
      ..add(s.height.listen((v) => _set(_state.value.copyWith(videoHeight: v))))
      ..add(s.tracks.listen(_onTracks))
      ..add(s.track.listen(_onTrack))
      ..add(s.videoParams.listen((_) => _refreshInfo()))
      ..add(s.audioParams.listen((_) => _refreshInfo()))
      ..add(
        s.error.listen((e) {
          debugPrint('pa_player: mpv error: $e');
          _set(_state.value.copyWith(error: e));
        }),
      );
  }

  static MediaTrack _audio(mk.AudioTrack t) => MediaTrack(
    id: t.id,
    title: t.title,
    language: t.language,
    codec: t.codec,
    external: t.uri,
  );

  static MediaTrack _sub(mk.SubtitleTrack t) => MediaTrack(
    id: t.id,
    title: t.title,
    language: t.language,
    codec: t.codec,
    external: t.uri || t.data,
  );

  static bool _real(String id) => id != 'auto' && id != 'no';

  void _onTracks(mk.Tracks t) {
    _set(
      _state.value.copyWith(
        audioTracks: t.audio.where((a) => _real(a.id)).map(_audio).toList(),
        subtitleTracks: t.subtitle.where((a) => _real(a.id)).map(_sub).toList(),
      ),
    );
    _refreshInfo();
  }

  void _onTrack(mk.Track t) {
    _set(
      _state.value.copyWith(
        audioTrack: _audio(t.audio),
        subtitleTrack: _real(t.subtitle.id) ? _sub(t.subtitle) : MediaTrack.off,
      ),
    );
    _refreshInfo();
  }

  void _refreshInfo() {
    final st = _player.state;
    final v = st.videoParams;
    final a = st.audioParams;
    final vt = st.track.video;
    final at = st.track.audio;
    final videoTrack = st.tracks.video.where((t) => t.id == vt.id).firstOrNull;
    final audioTrack = st.tracks.audio.where((t) => t.id == at.id).firstOrNull;
    final info = <String, String>{
      if (st.playlist.medias.isNotEmpty &&
          st.playlist.index < st.playlist.medias.length)
        'File': Uri.decodeFull(
          st.playlist.medias[st.playlist.index].uri.split('/').last,
        ),
      if (v.w != null && v.h != null) 'Resolution': '${v.w}×${v.h}',
      if (videoTrack?.codec != null) 'Video codec': videoTrack!.codec!,
      if (videoTrack?.fps != null)
        'Frame rate': '${videoTrack!.fps!.toStringAsFixed(3)} fps',
      if (v.pixelformat != null) 'Pixel format': v.pixelformat!,
      if (v.hwPixelformat != null) 'HW format': v.hwPixelformat!,
      if (v.primaries != null) 'Primaries': v.primaries!,
      if (v.gamma != null) 'Transfer': v.gamma!,
      if (audioTrack?.codec != null) 'Audio codec': audioTrack!.codec!,
      if (a.sampleRate != null) 'Sample rate': '${a.sampleRate} Hz',
      if (a.hrChannels != null || a.channels != null)
        'Channels': (a.hrChannels ?? a.channels)!,
      'Engine': 'media_kit / libmpv',
    };
    _set(_state.value.copyWith(info: info));
  }

  @override
  Future<void> open(
    List<String> uris, {
    int index = 0,
    Duration start = Duration.zero,
    bool play = true,
  }) async {
    final medias = <mk.Media>[
      for (var i = 0; i < uris.length; i++)
        mk.Media(
          _normalise(uris[i]),
          start: i == index && start > Duration.zero ? start : null,
        ),
    ];
    _set(_state.value.copyWith(completed: false));
    await _player.open(mk.Playlist(medias, index: index), play: play);
  }

  /// Accepts plain paths as well as URIs.
  static String _normalise(String uri) {
    if (uri.startsWith('/')) return Uri.file(uri).toString();
    return uri;
  }

  @override
  Future<void> play() => _player.play();

  @override
  Future<void> pause() => _player.pause();

  @override
  Future<void> seek(Duration position) => _player.seek(position);

  @override
  Future<void> setRate(double rate) => _player.setRate(rate);

  @override
  Future<void> setVolume(double volume) => _player.setVolume(volume);

  @override
  Future<void> jump(int index) => _player.jump(index);

  @override
  Future<void> setRepeat(QueueRepeat mode) =>
      _player.setPlaylistMode(switch (mode) {
        QueueRepeat.off => mk.PlaylistMode.none,
        QueueRepeat.all => mk.PlaylistMode.loop,
        QueueRepeat.one => mk.PlaylistMode.single,
      });

  @override
  Future<void> setAudioProfile(AudioProfile profile) async {
    await _ready;
    final p = _player.platform;
    if (_disposed || p is! mk.NativePlayer) return;
    for (final e in profile.mpvProperties.entries) {
      // Diagnostics overrides (mpvOptions) win over the profile.
      if (mpvOptions.containsKey(e.key)) continue;
      try {
        await p.setProperty(e.key, e.value);
      } catch (err) {
        debugPrint('pa_player: mpv ${e.key}=${e.value} failed: $err');
      }
    }
  }

  @override
  Future<void> setAudioTrack(MediaTrack track) async {
    final t = _player.state.tracks.audio.where((a) => a.id == track.id);
    if (t.isNotEmpty) await _player.setAudioTrack(t.first);
  }

  @override
  Future<void> setSubtitleTrack(MediaTrack track) async {
    if (track.isOff) {
      await _player.setSubtitleTrack(mk.SubtitleTrack.no());
      return;
    }
    final t = _player.state.tracks.subtitle.where((a) => a.id == track.id);
    if (t.isNotEmpty) await _player.setSubtitleTrack(t.first);
  }

  @override
  Future<void> addExternalSubtitle(String uri, {String? title}) =>
      _player.setSubtitleTrack(
        mk.SubtitleTrack.uri(
          _normalise(uri),
          title: title ?? uri.split('/').last,
        ),
      );

  @override
  Widget buildVideo(BuildContext context, {BoxFit fit = BoxFit.contain}) {
    final c = _controller;
    if (c == null) return const SizedBox.expand();
    return mkv.Video(
      controller: c,
      fit: fit,
      fill: const Color(0xFF000000),
      controls: mkv.NoVideoControls,
      // Keyboard/remote handling and wakelock live in PlayerScreen.
      wakelock: true,
      subtitleViewConfiguration: const mkv.SubtitleViewConfiguration(
        style: TextStyle(
          height: 1.3,
          fontSize: 44,
          color: Color(0xFFFFFFFF),
          fontWeight: FontWeight.w600,
          shadows: [Shadow(blurRadius: 6, color: Color(0xCC000000))],
          backgroundColor: Color(0x00000000),
        ),
      ),
    );
  }

  @override
  Future<void> dispose() async {
    if (_disposed) return;
    _disposed = true;
    for (final s in _subs) {
      await s.cancel();
    }
    await _player.dispose();
    // _state is left to GC: widgets may still detach listeners after this.
  }
}
