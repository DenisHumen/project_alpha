# ADR-NNNN: Платформенные хранилища секретов — Keystore + AES-GCM без Tink, Keychain `AfterFirstUnlock`, колбэк FRB для Flutter

- **Статус:** Предложен
- **Дата:** 2026-11-14
- **Решают:** владелец проекта
- **Связано:** `CORE-015`, `CORE-007`, `NET-011`, `UI-024`; ADR-0001; `docs/dev/secrets.md`; `docs/research/04` §5.1

## Контекст и проблема

`pa-secrets` (CORE-007) на мобильных и tvOS требует колбэк платформы `PlatformKeystore`, иначе `open` даёт `Unavailable`. В UniFFI (`pa-ffi`) колбэк `PlatformSecretStore` есть, но Flutter-приложение ходит в ядро через flutter_rust_bridge (`pa-frb`, свой `libpa_frb`, свои статические данные), и там колбэка не было: `CoreSources.open` на Android падал при первом пароле. Нужно решить: чем шифровать на Android, как хранить на Apple и как Kotlin/Swift-код попадает в ядро, загруженное через FRB.

## Факторы решения

- Минимум зависимостей и размера APK; только совместимые лицензии.
- Фоновые задачи (загрузки, синхронизация) должны читать секреты при заблокированном экране.
- Одна реализация хранилища на платформу для Flutter-приложения и нативных клиентов (tvOS на Swift, Kotlin-пользователи `pa-ffi`).
- Секретное значение не должно пропадать навсегда из-за потерянного ключа, но и не должно молча подменяться.

## Рассмотренные варианты

Android:
1. **`javax.crypto` AES-GCM с ключом AES-256 в Android Keystore, шифртексты в приватном SharedPreferences.**
2. Tink `AndroidKeysetManager` + `Aead` (как в research/04 §5.1).
3. `androidx.security:security-crypto` (`EncryptedSharedPreferences`).

Мост в FRB:
1. **Dart-колбэк (`DartFn`) в `secrets_open_platform`, Dart пересылает вызовы по platform channel в Kotlin/Swift.**
2. JNI/C-функции регистрации в `libpa_frb`, Kotlin/Swift вызывают их напрямую.
3. Грузить в приложение ещё и `pa-ffi` и открывать хранилище через UniFFI.

## Решение

- **Android — вариант 1.** Keystore делает AES-GCM сам; Tink в варианте 2 делает то же самое (ключ Keystore оборачивает ключ данных) и тянет Tink + protobuf-lite (около 1,5 МБ до R8) ради одного вызова AEAD; вариант 3 устарел (deprecated). Ключ без `setUserAuthenticationRequired` — аналог `AfterFirstUnlock`. Ключ секрета — associated data, шифртекст другой записи не расшифруется. Если запись не расшифровывается (ключ Keystore потерян: перенос данных на другое устройство, сброс учётных данных), она удаляется и считается отсутствующей — пароль вводят заново. Ошибки самого Keystore — ошибки, а не «нет значения».
- **Apple:** Keychain, generic password, сервис `app.kinolune.secrets`, `kSecAttrAccessibleAfterFirstUnlock`, без синхронизации в iCloud. Класс `KeychainSecretStore` в SwiftPM-пакете `PACore` реализует UniFFI-протокол `PlatformSecretStore` (tvOS-приложение, будущий iOS-раннер Flutter).
- **FRB — вариант 1.** `secrets_open_platform(data_dir, callback)` принимает Dart-функцию; поток ядра, которому нужен секрет, ждёт ответ (не дольше 30 с — на случай, если изолят Dart уже закрыт). Ответ FRB доставляет прямым FFI-вызовом, без пула потоков, поэтому ожидание не блокирует сам ответ. Пакет `pa_platform_secrets` (Flutter-плагин) передаёт вызовы в `KeystoreSecretStore` по каналу. Функции секретов в FRB не `sync`: синхронный вызов на потоке Dart ждал бы сам себя. Вариант 2 обходит Dart, но требует своего JNI/C-слоя и зависимости `jni`; вариант 3 даёт две копии ядра в одном процессе.
- Kotlin-класс хранилища лежит в `clients/android/secrets` (без зависимостей) и компилируется и в Flutter-плагин, и в модуль с UniFFI-адаптером `CorePlatformSecretStore` и instrumented-тестами.

### Последствия

- Хорошо: без новых зависимостей в APK; один Kotlin-класс и один Swift-класс на всех клиентов; `CoreSources.open` на Android работает с паролями.
- Плохо: на Android секреты доступны ядру, только пока жив Flutter-движок (вызов идёт через Dart); фоновая работа без движка потребует варианта 2. На iOS Flutter-плагина пока нет — у приложения нет iOS-раннера.
- Резервное копирование: `shared_prefs/kinolune_secrets.xml` попадёт в Auto Backup, но без ключа Keystore на другом устройстве не расшифруется и будет удалён при первом чтении. Исключать файл из бэкапа — отдельное решение для манифеста приложения.
