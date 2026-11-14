// SPDX-License-Identifier: MPL-2.0
import 'package:flutter/foundation.dart';

import '../engine/player_engine.dart';
import 'languages.dart';

/// Which subtitle languages the user reads, best first (ISO 639-2/B after
/// [normalizeLanguage]), and whether the system asks for captions.
@immutable
class SubtitlePreferences {
  SubtitlePreferences({
    List<String> languages = const [],
    this.captionsOn = false,
  }) : languages = List.unmodifiable(<String>{
         for (final l in languages) ?normalizeLanguage(l),
       });

  /// Normalized, without duplicates.
  final List<String> languages;

  /// The system caption setting is on (Android captioning, Apple
  /// "Closed Captions + SDH"): show full subtitles, SDH first.
  final bool captionsOn;
}

/// A subtitle choice that can be applied to another file: the language and
/// kind of the track, not its id. Kept per series (PLAY-007) so the next
/// episode gets the same subtitles.
@immutable
class SubtitleChoice {
  const SubtitleChoice({
    this.off = false,
    this.language,
    this.forced = false,
    this.hearingImpaired = false,
    this.external = false,
  });

  /// Subtitles switched off.
  static const SubtitleChoice none = SubtitleChoice(off: true);

  final bool off;
  final String? language;
  final bool forced;
  final bool hearingImpaired;
  final bool external;

  /// The choice that [track] stands for (null or "off": [none]).
  factory SubtitleChoice.of(MediaTrack? track) {
    if (track == null || track.isOff) return none;
    return SubtitleChoice(
      language: normalizeLanguage(track.language),
      forced: track.forced,
      hearingImpaired: track.hearingImpaired,
      external: track.external,
    );
  }

  Map<String, Object?> toJson() => off
      ? {'off': true}
      : {
          if (language != null) 'lang': language,
          if (forced) 'forced': true,
          if (hearingImpaired) 'sdh': true,
          if (external) 'external': true,
        };

  factory SubtitleChoice.fromJson(Object? json) {
    if (json is! Map) return none;
    if (json['off'] == true) return none;
    final lang = json['lang'];
    return SubtitleChoice(
      language: lang is String ? normalizeLanguage(lang) : null,
      forced: json['forced'] == true,
      hearingImpaired: json['sdh'] == true,
      external: json['external'] == true,
    );
  }

  @override
  bool operator ==(Object other) =>
      other is SubtitleChoice &&
      other.off == off &&
      other.language == language &&
      other.forced == forced &&
      other.hearingImpaired == hearingImpaired &&
      other.external == external;

  @override
  int get hashCode =>
      Object.hash(off, language, forced, hearingImpaired, external);

  @override
  String toString() => off
      ? 'SubtitleChoice(off)'
      : 'SubtitleChoice($language${forced ? ', forced' : ''}'
            '${hearingImpaired ? ', sdh' : ''}${external ? ', external' : ''})';
}

/// What the player remembers for a series: the subtitle choice and the
/// subtitle delay.
@immutable
class SubtitleMemory {
  const SubtitleMemory({this.choice, this.delay = Duration.zero});

  /// Null: no choice made yet (the policy picks).
  final SubtitleChoice? choice;

  /// Positive: subtitles later (mpv `sub-delay`).
  final Duration delay;

  SubtitleMemory copyWith({SubtitleChoice? choice, Duration? delay}) =>
      SubtitleMemory(choice: choice ?? this.choice, delay: delay ?? this.delay);

  Map<String, Object?> toJson() => {
    if (choice != null) 'choice': choice!.toJson(),
    if (delay != Duration.zero) 'delayMs': delay.inMilliseconds,
  };

  factory SubtitleMemory.fromJson(Object? json) {
    if (json is! Map) return const SubtitleMemory();
    final delay = json['delayMs'];
    return SubtitleMemory(
      choice: json.containsKey('choice')
          ? SubtitleChoice.fromJson(json['choice'])
          : null,
      delay: delay is int
          ? Duration(milliseconds: delay.clamp(-maxDelayMs, maxDelayMs))
          : Duration.zero,
    );
  }

  /// Largest delay the controls allow, either way.
  static const int maxDelayMs = 600000;

  @override
  bool operator ==(Object other) =>
      other is SubtitleMemory && other.choice == choice && other.delay == delay;

  @override
  int get hashCode => Object.hash(choice, delay);

  @override
  String toString() => 'SubtitleMemory($choice, ${delay.inMilliseconds} ms)';
}

/// Picks the subtitle track to show when a file opens.
///
/// 1. A [remembered] choice of the series wins: "off" stays off; otherwise
///    the track of the same language closest to it (forced, SDH, external).
/// 2. With captions on in the system: full subtitles in the first preferred
///    language that has them, SDH first.
/// 3. Audio in a language the user reads (or of unknown language): forced
///    subtitles in a preferred language — they translate only the foreign
///    lines. Audio in another language: full subtitles in a preferred
///    language, else forced ones.
///
/// Returns [MediaTrack.off] to switch subtitles off, or null to leave the
/// engine's own choice (nothing in the user's languages, audio of unknown
/// language without forced subtitles).
MediaTrack? pickSubtitle(
  List<MediaTrack> tracks, {
  required SubtitlePreferences preferences,
  SubtitleChoice? remembered,
  String? audioLanguage,
}) {
  final real = [
    for (final t in tracks)
      if (!t.isOff) t,
  ];
  String? lang(MediaTrack t) => normalizeLanguage(t.language);

  if (remembered != null) {
    if (remembered.off) return MediaTrack.off;
    final same = [
      for (final t in real)
        if (lang(t) == remembered.language) t,
    ];
    if (same.isNotEmpty) {
      int score(MediaTrack t) =>
          (t.forced == remembered.forced ? 4 : 0) +
          (t.hearingImpaired == remembered.hearingImpaired ? 2 : 0) +
          (t.external == remembered.external ? 1 : 0);
      return _best(same, score);
    }
  }

  final prefs = preferences.languages;
  if (prefs.isEmpty || real.isEmpty) return null;
  final full = [
    for (final t in real)
      if (!t.forced) t,
  ];
  final forced = [
    for (final t in real)
      if (t.forced) t,
  ];

  MediaTrack? inPreferred(List<MediaTrack> list, int Function(MediaTrack) s) {
    for (final l in prefs) {
      final match = [
        for (final t in list)
          if (lang(t) == l) t,
      ];
      if (match.isNotEmpty) return _best(match, s);
    }
    return null;
  }

  int plain(MediaTrack t) =>
      (t.hearingImpaired ? 0 : 2) + (t.isDefault ? 1 : 0);
  int sdhFirst(MediaTrack t) =>
      (t.hearingImpaired ? 2 : 0) + (t.isDefault ? 1 : 0);
  int byDefault(MediaTrack t) => t.isDefault ? 1 : 0;

  if (preferences.captionsOn) {
    final pick = inPreferred(full, sdhFirst) ?? inPreferred(forced, byDefault);
    if (pick != null) return pick;
  }

  final audio = normalizeLanguage(audioLanguage);
  final foreignAudio = audio != null && !prefs.contains(audio);
  if (foreignAudio) {
    return inPreferred(full, plain) ?? inPreferred(forced, byDefault);
  }
  // Audio in the user's language: only the foreign lines.
  final forcedPick = inPreferred(forced, byDefault);
  if (forcedPick != null) return forcedPick;
  return audio == null ? null : MediaTrack.off;
}

/// The track with the highest [score]; the first one on a tie.
MediaTrack _best(List<MediaTrack> list, int Function(MediaTrack) score) {
  var best = list.first;
  var bestScore = score(best);
  for (final t in list.skip(1)) {
    final s = score(t);
    if (s > bestScore) {
      best = t;
      bestScore = s;
    }
  }
  return best;
}

/// Forced / SDH flags of an embedded track from its title, when the
/// container has no flags for them ("Forced", "Надписи", "SDH", "CC").
({bool forced, bool hearingImpaired}) flagsFromTitle(String? title) {
  final t = (title ?? '').toLowerCase();
  final words = t.split(RegExp(r'[^\p{L}\p{N}]+', unicode: true));
  final forced =
      words.contains('forced') ||
      words.contains('foreign') ||
      t.contains('надписи') ||
      t.contains('форсир');
  final sdh =
      words.contains('sdh') ||
      words.contains('cc') ||
      words.contains('hi') ||
      t.contains('hearing impaired') ||
      t.contains('для слабослышащих');
  return (forced: forced, hearingImpaired: sdh);
}
