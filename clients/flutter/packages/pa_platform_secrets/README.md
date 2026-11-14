<!-- SPDX-License-Identifier: MPL-2.0 -->

# pa_platform_secrets

Платформенное хранилище секретов ядра на мобильных (CORE-015, [docs/dev/secrets.md](../../../../docs/dev/secrets.md)).

- **Android:** ключ AES-256 в Android Keystore + AES-GCM поверх приватного SharedPreferences — `KeystoreSecretStore` из [`clients/android/secrets`](../../../android/secrets) (там же instrumented-тесты). Плагин отдаёт его по каналу `pa_platform_secrets`.
- **Ядро:** `openPlatformSecrets()` вызывает `secretsOpenPlatform` из `pa_core` (flutter_rust_bridge) с Dart-колбэком; дальше `CoreSources.open` и `secretGet/Set/Delete` ходят в Keystore.
- **iOS:** у Flutter-приложения пока нет iOS-раннера; Swift-реализация (`KeychainSecretStore`, `kSecAttrAccessibleAfterFirstUnlock`) уже лежит в `tools/bindings/PACore` и подключится сюда так же, когда раннер появится.
- **Десктоп:** не нужен — ядро берёт системное хранилище (Keychain, Credential Manager, Secret Service).

```bash
flutter pub get && flutter analyze && flutter test
```
