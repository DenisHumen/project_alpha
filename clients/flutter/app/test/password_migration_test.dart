// SPDX-License-Identifier: MPL-2.0
//
// LIB-018: SMB passwords that older versions kept in flutter_secure_storage
// move into the core's secret store once, then the old copy is deleted.

import 'package:flutter_test/flutter_test.dart';
import 'package:kinolune/fakes/fake_library.dart';
import 'package:kinolune/fakes/fake_source_manager.dart';
import 'package:kinolune/services/app_services.dart';
import 'package:kinolune/services/secure_store.dart';
import 'package:kinolune/sources/source_manager.dart';

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();

  test('an old SMB password moves to the core once', () async {
    final library = FakeLibrary(scanStepDelay: Duration.zero);
    final manager = FakeSourceManager(library);
    final bare = await manager.create(
      const SourceDraft(
        type: SourceType.smb,
        name: 'NAS',
        address: 'smb://nas/media',
        username: 'anna',
      ),
    );
    final withPwd = await manager.create(
      const SourceDraft(
        type: SourceType.smb,
        name: 'Backup',
        address: 'smb://backup/media',
        username: 'anna',
        password: 'core-pass',
      ),
    );
    final store = MemorySecureStore()
      ..values[SecureStore.smbPassword(bare.id)] = 'old-pass'
      ..values[SecureStore.smbPassword(withPwd.id)] = 'stale-pass'
      ..values[SecureStore.tmdbKey] = 'k';
    manager.drafts.clear();

    final services = AppServices(
      library: library,
      secureStore: store,
      sourceManager: manager,
    );
    await services.init();

    final moved = manager.drafts.single;
    expect(moved.name, 'NAS');
    expect(moved.password, 'old-pass');
    expect(moved.address, 'smb://nas/media');
    expect(moved.username, 'anna');
    expect(services.managedSource(bare.id)!.hasPassword, isTrue);
    expect(store.values.containsKey(SecureStore.smbPassword(bare.id)), isFalse);
    expect(
      store.values.containsKey(SecureStore.smbPassword(withPwd.id)),
      isFalse,
    );
    expect(store.values[SecureStore.tmdbKey], 'k');

    // Second start: nothing left to move.
    manager.drafts.clear();
    await services.migrateLegacySmbPasswords();
    expect(manager.drafts, isEmpty);
  });

  test('a failed move keeps the old copy for the next start', () async {
    final library = FakeLibrary(scanStepDelay: Duration.zero);
    final manager = FakeSourceManager(library);
    final nas = await manager.create(
      const SourceDraft(
        type: SourceType.smb,
        name: 'NAS',
        address: 'smb://nas/media',
      ),
    );
    final store = MemorySecureStore()
      ..values[SecureStore.smbPassword(nas.id)] = 'old-pass';
    manager.nextError = const SourceException(SourceErrorCode.secretStore);
    final services = AppServices(
      library: library,
      secureStore: store,
      sourceManager: manager,
    );
    await services.init();
    expect(store.values[SecureStore.smbPassword(nas.id)], 'old-pass');
    expect(services.managedSource(nas.id)!.hasPassword, isFalse);

    await services.migrateLegacySmbPasswords();
    expect(store.values.containsKey(SecureStore.smbPassword(nas.id)), isFalse);
    expect(services.managedSource(nas.id)!.hasPassword, isTrue);
  });

  test('the reduced manager leaves old passwords alone', () async {
    final library = FakeLibrary(scanStepDelay: Duration.zero);
    final store = MemorySecureStore()
      ..values[SecureStore.smbPassword('1')] = 'old-pass';
    final services = AppServices(library: library, secureStore: store);
    await services.init();
    expect(store.values[SecureStore.smbPassword('1')], 'old-pass');
  });
}
