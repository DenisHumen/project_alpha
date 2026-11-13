// SPDX-License-Identifier: MPL-2.0

/// Kinolune player (release 0.1): video [PlayerScreen] and the music player
/// ([MusicPlayerController], [NowPlayingBar], [NowPlayingScreen]) on
/// media_kit / libmpv. Call [ensurePaPlayerInitialized] in `main()`.
library;

export 'src/engine/media_kit_engine.dart'
    show MediaKitEngine, ensurePaPlayerInitialized;
export 'src/engine/player_engine.dart';
export 'src/l10n/pa_player_strings.dart';
export 'src/music/artwork.dart' show Artwork;
export 'src/music/music_player_controller.dart';
export 'src/music/music_queue.dart';
export 'src/music/now_playing_bar.dart';
export 'src/music/now_playing_screen.dart';
export 'src/play_request.dart';
export 'src/ui/tokens.dart' show PaTokens, formatDuration;
export 'src/video/osd_controller.dart';
export 'src/video/player_screen.dart';
export 'src/video/progress_reporter.dart';
