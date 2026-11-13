// SPDX-License-Identifier: MPL-2.0
import 'package:fake_async/fake_async.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:pa_player/pa_player.dart';
import 'package:pa_player/testing.dart';

void main() {
  test('OsdController: auto-hide only while playing and no panel', () {
    fakeAsync((fa) {
      final osd = OsdController(autoHide: const Duration(seconds: 4));
      expect(osd.visible, isTrue);
      fa.elapse(const Duration(seconds: 10));
      expect(osd.visible, isTrue, reason: 'paused');
      osd.playing = true;
      fa.elapse(const Duration(seconds: 3));
      osd.show(); // interaction restarts the timer
      fa.elapse(const Duration(seconds: 3));
      expect(osd.visible, isTrue);
      fa.elapse(const Duration(seconds: 2));
      expect(osd.visible, isFalse);

      osd.openPanel(OsdPanel.audio);
      expect(osd.visible, isTrue);
      fa.elapse(const Duration(seconds: 10));
      expect(osd.visible, isTrue, reason: 'panel open');
      osd.closePanel();
      fa.elapse(const Duration(seconds: 5));
      expect(osd.visible, isFalse);
      osd.dispose();
    });
  });

  test('ProgressReporter: periodic, pause, seek, close, dedupe', () {
    fakeAsync((fa) {
      final e = FakePlayerEngine(loadedDuration: const Duration(seconds: 100));
      final out = <(int, int)>[];
      final r = ProgressReporter(
        engine: e,
        onProgress: (p, d) => out.add((p, d)),
      );
      r.report();
      expect(out, isEmpty, reason: 'duration unknown yet');
      e.open(['x']);
      fa.flushMicrotasks();
      fa.elapse(const Duration(seconds: 5));
      expect(out, [(0, 100000)]);
      e.advance(const Duration(seconds: 5));
      fa.elapse(const Duration(seconds: 5));
      expect(out.last, (5000, 100000));
      e.pause();
      fa.flushMicrotasks();
      expect(out, hasLength(2), reason: 'same position is not re-sent');
      e.advance(const Duration(seconds: 1));
      fa.elapse(const Duration(seconds: 20));
      expect(out, hasLength(2), reason: 'no ticks while paused');
      r.onSeek(const Duration(seconds: 50));
      expect(out.last, (50000, 100000));
      e.seek(const Duration(seconds: 51));
      r.close();
      expect(out.last, (51000, 100000));
      fa.elapse(const Duration(seconds: 20));
      expect(out.last, (51000, 100000));
    });
  });

  test('formatDuration', () {
    expect(formatDuration(const Duration(seconds: 65)), '1:05');
    expect(
      formatDuration(const Duration(hours: 1, minutes: 2, seconds: 3)),
      '1:02:03',
    );
  });

  test('MediaTrack.label', () {
    expect(MediaTrack.off.label, 'Off');
    expect(const MediaTrack(id: '2').label, 'Track 2');
    expect(
      const MediaTrack(
        id: '1',
        title: 'Director',
        language: 'en',
        codec: 'aac',
      ).label,
      'Director · en (aac)',
    );
  });
}
