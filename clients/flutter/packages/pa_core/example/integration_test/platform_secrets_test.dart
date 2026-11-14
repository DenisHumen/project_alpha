// SPDX-License-Identifier: MPL-2.0
// CORE-015: the core's secret store on Android goes Rust -> Dart callback ->
// platform channel -> Kotlin KeystoreSecretStore (Android Keystore + AES-GCM).
// Run on an emulator: flutter test integration_test/platform_secrets_test.dart -d <emulator>
// Skipped on platforms without a store in pa_platform_secrets (desktop).
import 'package:flutter_test/flutter_test.dart';
import 'package:integration_test/integration_test.dart';
import 'package:pa_core/pa_core.dart';
import 'package:pa_platform_secrets/pa_platform_secrets.dart';

void main() {
  IntegrationTestWidgetsFlutterBinding.ensureInitialized();

  setUpAll(() async => RustLib.init());

  testWidgets(
    'write, read, overwrite and delete through the core',
    (tester) async {
      final info = await openPlatformSecrets();
      expect(info, isNotNull);
      expect(info!.backend, 'platform');
      expect(info.secure, isTrue);
      expect((await secretsInfo())?.backend, 'platform');

      const key = 'test.platform_secrets:1';
      await secretDelete(key: key);
      expect(await secretGet(key: key), isNull);

      await secretSet(key: key, value: 'через-ядро');
      expect(await secretGet(key: key), 'через-ядро');
      await secretSet(key: key, value: 'second');
      expect(await secretGet(key: key), 'second');

      // Reopen: the value lives in the Keystore-encrypted file, not in the
      // core's memory.
      await secretsClose();
      await openPlatformSecrets();
      expect(await secretGet(key: key), 'second');

      expect(await secretDelete(key: key), isTrue);
      expect(await secretDelete(key: key), isFalse);
      expect(await secretGet(key: key), isNull);

      await expectLater(
        secretGet(key: 'bad key!'),
        throwsA(
          isA<SecretsError>().having((e) => e.code, 'code', 'invalid_key'),
        ),
      );
      await secretsClose();
    },
    skip: !platformSecretsSupported,
  );
}
