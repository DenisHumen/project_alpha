// SPDX-License-Identifier: MPL-2.0

import 'package:pa_library/src/rust_library.dart';
import 'package:test/test.dart';

void main() {
  test('stream URLs encode every path segment (LIB-018)', () {
    expect(
      RustLibrary.streamUrl(
        'http://127.0.0.1:4000/tok',
        3,
        'Фильмы/Dune (2021)/Dune #1?.mkv',
      ),
      'http://127.0.0.1:4000/tok/3/'
      '%D0%A4%D0%B8%D0%BB%D1%8C%D0%BC%D1%8B/Dune%20(2021)/Dune%20%231%3F.mkv',
    );
    expect(RustLibrary.streamUrl('http://h/t', 1, 'a.mkv'), 'http://h/t/1/a.mkv');
  });
}
