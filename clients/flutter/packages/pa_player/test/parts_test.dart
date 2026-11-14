// SPDX-License-Identifier: MPL-2.0
// LIB-020: the mpv EDL URI that plays the parts of a split movie as one
// timeline. The real playback is checked in edl_libmpv_test.dart.

import 'package:flutter_test/flutter_test.dart';
import 'package:pa_player/pa_player.dart';
import 'package:pa_player/src/engine/parts.dart' show mediaDisplayName;

void main() {
  test('one part is played as it is', () {
    expect(partsUri(['/m/Arrival.mkv']), '/m/Arrival.mkv');
    expect(
      partsUri(['http://127.0.0.1:9/t/1/a.mkv']),
      'http://127.0.0.1:9/t/1/a.mkv',
    );
    expect(() => partsUri([]), throwsArgumentError);
  });

  test('two local parts make an EDL with byte lengths', () {
    expect(
      partsUri(['/m/Heat.cd1.avi', '/m/Heat.cd2.avi']),
      'edl://%15%/m/Heat.cd1.avi;%15%/m/Heat.cd2.avi',
    );
  });

  test('lengths count UTF-8 bytes; separators in names are safe', () {
    const parts = [
      '/фильмы/Чужой; часть 1,2 %20.vob', // ; , % and Cyrillic
      'http://127.0.0.1:4000/tok/3/%D0%A7/VTS_01_2.VOB',
    ];
    final uri = partsUri(parts);
    expect(uri, startsWith('edl://%48%/фильмы/'));
    expect(partsOf(uri), parts);
  });

  test('a plain URI is its own part; broken EDLs are null', () {
    expect(partsOf('/m/a.mkv'), ['/m/a.mkv']);
    expect(partsOf('edl://'), isNull);
    expect(partsOf('edl://%99%/short'), isNull);
    expect(partsOf('edl:///m/a.mkv'), isNull);
    expect(partsOf('edl://%3%abc,x'), isNull);
  });

  test('display name of a part list', () {
    expect(mediaDisplayName('/m/Heat%20one.mkv'), 'Heat one.mkv');
    expect(mediaDisplayName('/m/100%.mkv'), '100%.mkv');
    expect(
      mediaDisplayName(partsUri(['/m/Heat.cd1.avi', '/m/Heat.cd2.avi'])),
      'Heat.cd1.avi (+1)',
    );
    expect(mediaDisplayName('edl://broken'), 'EDL');
  });
}
