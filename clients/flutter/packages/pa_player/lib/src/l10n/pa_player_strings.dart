// SPDX-License-Identifier: MPL-2.0
import 'package:flutter/foundation.dart';
import 'package:flutter/widgets.dart';

import '../engine/player_engine.dart';

/// User-facing strings of the player (OSD, side panels, mini player, Now
/// Playing). The package carries its own English and Russian texts so it does
/// not depend on the app's gen-l10n.
///
/// The app passes its locale by adding [PaPlayerStrings.delegate] to
/// `MaterialApp.localizationsDelegates`. Without the delegate, [of] still
/// picks the texts from the ambient [Localizations] locale, and falls back to
/// English for unsupported locales.
@immutable
class PaPlayerStrings {
  const PaPlayerStrings._({
    required this.languageCode,
    required this.back,
    required this.back10,
    required this.forward10,
    required this.play,
    required this.pause,
    required this.audio,
    required this.subtitles,
    required this.subtitleFile,
    required this.speed,
    required this.playbackSpeed,
    required this.info,
    required this.fullscreen,
    required this.close,
    required this.noAudioTracks,
    required this.off,
    required this.externalFile,
    required this.loadFromFile,
    required this.normalSpeed,
    required this.infoTitle,
    required this.infoPosition,
    required this.cannotOpenFolder,
    required this.noSubtitleFiles,
    required this.next,
    required this.previous,
    required this.nowPlaying,
    required this.nothingPlaying,
    required this.shuffleOn,
    required this.shuffleOff,
    required this.repeatOff,
    required this.repeatAll,
    required this.repeatOne,
    required this.upNext,
    required this.endOfQueue,
    required this.playNow,
    required this.cancel,
    required this.subtitleDelay,
    required this.subtitleDelayNone,
    required this.subtitlesEarlier,
    required this.subtitlesLater,
    required this.resetDelay,
    required this.forcedTrack,
    required this.sdhTrack,
    required String Function(String seconds) delayLater,
    required String Function(String seconds) delayEarlier,
    required String Function(String id) track,
    required String Function(String remaining, String time) remainingEndsAt,
    required String Function(int seconds) upNextIn,
    required Map<String, String> engineInfoKeys,
  }) : _track = track,
       _remainingEndsAt = remainingEndsAt,
       _upNextIn = upNextIn,
       _delayLater = delayLater,
       _delayEarlier = delayEarlier,
       _engineInfoKeys = engineInfoKeys;

  /// `en` or `ru`.
  final String languageCode;

  final String back;
  final String back10;
  final String forward10;
  final String play;
  final String pause;
  final String audio;
  final String subtitles;
  final String subtitleFile;

  /// Tooltip prefix of the speed button: "Speed (1.5×)".
  final String speed;
  final String playbackSpeed;
  final String info;
  final String fullscreen;
  final String close;
  final String noAudioTracks;
  final String off;
  final String externalFile;
  final String loadFromFile;
  final String normalSpeed;
  final String infoTitle;
  final String infoPosition;
  final String cannotOpenFolder;
  final String noSubtitleFiles;
  final String next;
  final String previous;
  final String nowPlaying;
  final String nothingPlaying;
  final String shuffleOn;
  final String shuffleOff;
  final String repeatOff;
  final String repeatAll;
  final String repeatOne;
  final String upNext;
  final String endOfQueue;

  /// Up Next card: play the next item without waiting.
  final String playNow;

  /// Up Next card: stop the countdown.
  final String cancel;

  /// Subtitles panel: heading of the delay control (PLAY-007).
  final String subtitleDelay;

  /// Delay value when subtitles are not shifted.
  final String subtitleDelayNone;

  /// Tooltip of the button that shows subtitles 0.1 s earlier.
  final String subtitlesEarlier;

  /// Tooltip of the button that shows subtitles 0.1 s later.
  final String subtitlesLater;
  final String resetDelay;

  /// Tag of a forced subtitle track.
  final String forcedTrack;

  /// Tag of an SDH subtitle track.
  final String sdhTrack;
  final String Function(String seconds) _delayLater;
  final String Function(String seconds) _delayEarlier;

  final String Function(String id) _track;
  final String Function(String remaining, String time) _remainingEndsAt;
  final String Function(int seconds) _upNextIn;
  final Map<String, String> _engineInfoKeys;

  /// "-1:02:03 · ends at 21:40".
  String remainingEndsAt(String remaining, String time) =>
      _remainingEndsAt(remaining, time);

  /// Up Next countdown: "Starts in 7 s".
  String upNextIn(int seconds) => _upNextIn(seconds);

  /// Subtitle delay with an unambiguous sign: "Subtitles 0.3 s later",
  /// "Subtitles 0.3 s earlier", or [subtitleDelayNone].
  String delayLabel(Duration delay) {
    final ms = delay.inMilliseconds;
    if (ms == 0) return subtitleDelayNone;
    var secs = (ms.abs() / 1000).toStringAsFixed(ms.abs() % 100 == 0 ? 1 : 2);
    if (languageCode == 'ru') secs = secs.replaceAll('.', ',');
    return ms > 0 ? _delayLater(secs) : _delayEarlier(secs);
  }

  /// Tooltip of the speed button.
  String speedWithRate(String rate) => '$speed ($rate)';

  /// [MediaTrack.label] with the fixed words translated.
  String trackLabel(MediaTrack t) {
    if (t.isOff) return off;
    final parts = <String>[
      if (t.title != null && t.title!.isNotEmpty) t.title!,
      if (t.language != null && t.language!.isNotEmpty) t.language!,
    ];
    final head = parts.isEmpty ? _track(t.id) : parts.join(' · ');
    return t.codec == null ? head : '$head (${t.codec})';
  }

  /// Translates a key of [EngineState.info] ("Resolution", "Video codec"…);
  /// unknown keys are returned as is.
  String engineInfoKey(String key) => _engineInfoKeys[key] ?? key;

  static const en = PaPlayerStrings._(
    languageCode: 'en',
    back: 'Back',
    back10: 'Back 10 s',
    forward10: 'Forward 10 s',
    play: 'Play',
    pause: 'Pause',
    audio: 'Audio',
    subtitles: 'Subtitles',
    subtitleFile: 'Subtitle file',
    speed: 'Speed',
    playbackSpeed: 'Playback speed',
    info: 'Info',
    fullscreen: 'Fullscreen (F)',
    close: 'Close',
    noAudioTracks: 'No audio tracks',
    off: 'Off',
    externalFile: 'External file',
    loadFromFile: 'Load from file…',
    normalSpeed: 'Normal',
    infoTitle: 'Title',
    infoPosition: 'Position',
    cannotOpenFolder: 'Cannot open folder',
    noSubtitleFiles: 'No subtitle files here',
    next: 'Next',
    previous: 'Previous',
    nowPlaying: 'Now Playing',
    nothingPlaying: 'Nothing playing',
    shuffleOn: 'Shuffle on',
    shuffleOff: 'Shuffle off',
    repeatOff: 'Repeat off',
    repeatAll: 'Repeat all',
    repeatOne: 'Repeat one',
    upNext: 'Up next',
    endOfQueue: 'End of queue',
    playNow: 'Play now',
    cancel: 'Cancel',
    subtitleDelay: 'Subtitle timing',
    subtitleDelayNone: 'In sync with the video',
    subtitlesEarlier: 'Show subtitles earlier',
    subtitlesLater: 'Show subtitles later',
    resetDelay: 'Reset',
    forcedTrack: 'Forced',
    sdhTrack: 'SDH',
    delayLater: _enDelayLater,
    delayEarlier: _enDelayEarlier,
    track: _enTrack,
    remainingEndsAt: _enEndsAt,
    upNextIn: _enUpNextIn,
    engineInfoKeys: {},
  );

  static const ru = PaPlayerStrings._(
    languageCode: 'ru',
    back: 'Назад',
    back10: 'Назад на 10 с',
    forward10: 'Вперёд на 10 с',
    play: 'Воспроизвести',
    pause: 'Пауза',
    audio: 'Звук',
    subtitles: 'Субтитры',
    subtitleFile: 'Файл субтитров',
    speed: 'Скорость',
    playbackSpeed: 'Скорость воспроизведения',
    info: 'Информация',
    fullscreen: 'Полный экран (F)',
    close: 'Закрыть',
    noAudioTracks: 'Нет звуковых дорожек',
    off: 'Выкл.',
    externalFile: 'Внешний файл',
    loadFromFile: 'Загрузить из файла…',
    normalSpeed: 'Обычная',
    infoTitle: 'Название',
    infoPosition: 'Позиция',
    cannotOpenFolder: 'Не удалось открыть папку',
    noSubtitleFiles: 'Здесь нет файлов субтитров',
    next: 'Следующий',
    previous: 'Предыдущий',
    nowPlaying: 'Сейчас играет',
    nothingPlaying: 'Ничего не играет',
    shuffleOn: 'Перемешивание включено',
    shuffleOff: 'Перемешивание выключено',
    repeatOff: 'Повтор выключен',
    repeatAll: 'Повтор всех',
    repeatOne: 'Повтор одного',
    upNext: 'Далее',
    endOfQueue: 'Конец очереди',
    playNow: 'Смотреть сейчас',
    cancel: 'Отмена',
    subtitleDelay: 'Синхронизация субтитров',
    subtitleDelayNone: 'Совпадают с видео',
    subtitlesEarlier: 'Показывать субтитры раньше',
    subtitlesLater: 'Показывать субтитры позже',
    resetDelay: 'Сбросить',
    forcedTrack: 'Форсированные',
    sdhTrack: 'Для слабослышащих',
    delayLater: _ruDelayLater,
    delayEarlier: _ruDelayEarlier,
    track: _ruTrack,
    remainingEndsAt: _ruEndsAt,
    upNextIn: _ruUpNextIn,
    engineInfoKeys: {
      'File': 'Файл',
      'Resolution': 'Разрешение',
      'Video codec': 'Видеокодек',
      'Frame rate': 'Частота кадров',
      'Pixel format': 'Формат пикселей',
      'HW format': 'Аппаратный формат',
      'Primaries': 'Основные цвета',
      'Transfer': 'Передаточная функция',
      'Audio codec': 'Аудиокодек',
      'Sample rate': 'Частота дискретизации',
      'Channels': 'Каналы',
      'Engine': 'Движок',
    },
  );

  static String _enDelayLater(String s) => 'Subtitles $s s later';
  static String _enDelayEarlier(String s) => 'Subtitles $s s earlier';
  static String _ruDelayLater(String s) => 'Субтитры позже на $s с';
  static String _ruDelayEarlier(String s) => 'Субтитры раньше на $s с';
  static String _enTrack(String id) => 'Track $id';
  static String _ruTrack(String id) => 'Дорожка $id';
  static String _enEndsAt(String r, String t) => '-$r · ends at $t';
  static String _ruEndsAt(String r, String t) => '-$r · закончится в $t';
  static String _enUpNextIn(int s) => 'Starts in $s s';
  static String _ruUpNextIn(int s) => 'Начнётся через $s с';

  /// Locales with their own texts.
  static const supportedLocales = [Locale('en'), Locale('ru')];

  /// Texts for [locale] (by language code); English for anything else.
  static PaPlayerStrings forLocale(Locale? locale) =>
      locale?.languageCode == 'ru' ? ru : en;

  /// Add to `MaterialApp.localizationsDelegates` to follow the app's locale.
  static const LocalizationsDelegate<PaPlayerStrings> delegate =
      _PaPlayerStringsDelegate();

  /// Texts for [context]: from [delegate] if registered, else by the ambient
  /// locale, else English.
  static PaPlayerStrings of(BuildContext context) =>
      Localizations.of<PaPlayerStrings>(context, PaPlayerStrings) ??
      forLocale(Localizations.maybeLocaleOf(context));
}

class _PaPlayerStringsDelegate extends LocalizationsDelegate<PaPlayerStrings> {
  const _PaPlayerStringsDelegate();

  // Every locale is "supported": unsupported ones get English, so adding the
  // delegate never breaks an app locale (e.g. a pseudo-locale).
  @override
  bool isSupported(Locale locale) => true;

  @override
  Future<PaPlayerStrings> load(Locale locale) =>
      SynchronousFuture(PaPlayerStrings.forLocale(locale));

  @override
  bool shouldReload(_PaPlayerStringsDelegate old) => false;
}
