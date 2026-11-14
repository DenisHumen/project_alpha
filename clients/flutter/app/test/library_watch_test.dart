// SPDX-License-Identifier: MPL-2.0
//
// LIB-005 in the app: the library watcher starts when the library opens,
// restarts when the sources change and stops with the services.

import 'package:flutter_test/flutter_test.dart';
import 'package:kinolune/fakes/fake_library.dart';
import 'package:kinolune/integration/library.dart';
import 'package:kinolune/services/app_services.dart';
import 'package:kinolune/services/secure_store.dart';

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();

  test('the watcher follows the sources of the open library', () async {
    final library = FakeLibrary.demo(scanStepDelay: Duration.zero);
    final services = AppServices(
      library: library,
      secureStore: MemorySecureStore(),
    );
    await services.init();
    await pumpEventQueue();
    expect((library.watching, library.watchStarts), (true, 1));

    await services.addSource(
      const MediaSource(
        id: '',
        kind: SourceKind.localFolder,
        name: 'More',
        path: '/srv/more',
      ),
    );
    await pumpEventQueue();
    expect((library.watching, library.watchStarts), (true, 2));

    services.dispose();
    await pumpEventQueue();
    expect(library.watching, isFalse);
  });
}
