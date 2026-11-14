// SPDX-License-Identifier: MPL-2.0
//
// PLAY-007: which subtitle track opens with a file — the user's languages,
// forced tracks, system captions and the choice remembered for a series.
import 'package:flutter_test/flutter_test.dart';
import 'package:pa_player/pa_player.dart';

const ruFull = MediaTrack(id: '1', language: 'rus', title: 'Full');
const ruForced = MediaTrack(id: '2', language: 'ru', forced: true);
const enFull = MediaTrack(id: '3', language: 'eng');
const enSdh = MediaTrack(id: '4', language: 'en', hearingImpaired: true);
const ruExternal = MediaTrack(
  id: '5',
  language: 'rus',
  title: 'Movie.ru.srt',
  external: true,
);
const jaFull = MediaTrack(id: '6', language: 'jpn');

final ru = SubtitlePreferences(languages: ['ru']);

MediaTrack? pick(
  List<MediaTrack> tracks, {
  SubtitlePreferences? prefs,
  SubtitleChoice? remembered,
  String? audio,
}) => pickSubtitle(
  tracks,
  preferences: prefs ?? ru,
  remembered: remembered,
  audioLanguage: audio,
);

void main() {
  test('preferences normalize and dedupe languages', () {
    expect(
      SubtitlePreferences(languages: ['ru-RU', 'rus', 'en', 'xx']).languages,
      ['rus', 'eng'],
    );
  });

  group('without a remembered choice', () {
    final cases =
        <
          (
            String,
            List<MediaTrack>,
            String?,
            SubtitlePreferences?,
            MediaTrack?,
          )
        >[
          (
            'audio in the user language: forced in that language',
            [ruFull, ruForced, enFull],
            'rus',
            null,
            ruForced,
          ),
          (
            'audio in the user language, no forced: off',
            [ruFull, enFull],
            'ru',
            null,
            MediaTrack.off,
          ),
          (
            'foreign audio: full subtitles in the user language',
            [enFull, ruForced, ruFull],
            'eng',
            null,
            ruFull,
          ),
          (
            'foreign audio: plain before SDH',
            [enSdh, enFull],
            'jpn',
            SubtitlePreferences(languages: ['en']),
            enFull,
          ),
          (
            'foreign audio, only forced in the user language',
            [enFull, ruForced],
            'eng',
            null,
            ruForced,
          ),
          (
            'foreign audio, second preferred language',
            [jaFull, enFull],
            'jpn',
            SubtitlePreferences(languages: ['ru', 'en']),
            enFull,
          ),
          (
            'foreign audio, nothing in the user languages: engine choice',
            [jaFull],
            'jpn',
            null,
            null,
          ),
          (
            'unknown audio: forced in the user language',
            [ruFull, ruForced],
            null,
            null,
            ruForced,
          ),
          (
            'unknown audio, no forced: engine choice',
            [ruFull],
            null,
            null,
            null,
          ),
          (
            'system captions on: full subtitles, SDH first',
            [enFull, enSdh, ruForced],
            'eng',
            SubtitlePreferences(languages: ['en'], captionsOn: true),
            enSdh,
          ),
          (
            'system captions on, audio in the user language: still shown',
            [ruForced, ruFull],
            'rus',
            SubtitlePreferences(languages: ['ru'], captionsOn: true),
            ruFull,
          ),
          (
            'external sidecar of the user language',
            [enFull, ruExternal],
            'eng',
            null,
            ruExternal,
          ),
          ('no tracks', [], 'eng', null, null),
          (
            'no preferred languages',
            [ruFull],
            'eng',
            SubtitlePreferences(),
            null,
          ),
        ];
    for (final (name, tracks, audio, prefs, expected) in cases) {
      test(name, () {
        expect(pick(tracks, prefs: prefs, audio: audio), expected);
      });
    }

    test('the default flag breaks ties', () {
      const a = MediaTrack(id: 'a', language: 'rus');
      const b = MediaTrack(id: 'b', language: 'rus', isDefault: true);
      expect(pick([a, b], audio: 'eng'), b);
    });
  });

  group('remembered for the series', () {
    test('off stays off', () {
      expect(
        pick([ruFull, ruForced], remembered: SubtitleChoice.none, audio: 'en'),
        MediaTrack.off,
      );
    });

    test('same language and kind on the next episode', () {
      // The ids differ between episodes; the language and flags do not.
      const next = [
        MediaTrack(id: '7', language: 'rus'),
        MediaTrack(id: '8', language: 'rus', forced: true),
        MediaTrack(id: '9', language: 'eng'),
      ];
      expect(
        pick(
          next,
          remembered: const SubtitleChoice(language: 'rus', forced: true),
          audio: 'eng',
        )!.id,
        '8',
      );
      expect(
        pick(
          next,
          remembered: const SubtitleChoice(language: 'eng'),
          audio: 'rus',
        )!.id,
        '9',
      );
    });

    test('external choice prefers the sidecar', () {
      expect(
        pick(
          [ruFull, ruExternal],
          remembered: const SubtitleChoice(language: 'rus', external: true),
        ),
        ruExternal,
      );
    });

    test('language missing in this episode: the policy decides', () {
      expect(
        pick(
          [ruFull, ruForced],
          remembered: const SubtitleChoice(language: 'jpn'),
          audio: 'rus',
        ),
        ruForced,
      );
    });
  });

  test('flags from embedded track titles', () {
    expect(flagsFromTitle('English (Forced)').forced, isTrue);
    expect(flagsFromTitle('Надписи').forced, isTrue);
    expect(flagsFromTitle('English SDH').hearingImpaired, isTrue);
    expect(flagsFromTitle('English [CC]').hearingImpaired, isTrue);
    expect(flagsFromTitle('Для слабослышащих').hearingImpaired, isTrue);
    final plain = flagsFromTitle('Ethics of Chinese');
    expect(plain.forced, isFalse);
    expect(plain.hearingImpaired, isFalse);
    expect(flagsFromTitle(null).forced, isFalse);
  });
}
