# Политика секретов

> Задача REPO-009. Связано: [SECURITY.md](../../SECURITY.md), [ADR-0004](../adr/0004-no-project-servers-local-sync.md) (своих серверов нет), [ADR-0005](../adr/0005-license-mpl2-dco.md), [условия провайдеров](../legal/providers.md), ROADMAP §0.3 п.10.

Репозиторий публичный. Любая строка, попавшая в git, считается опубликованной навсегда: удаление коммита не помогает, ключ нужно отзывать.

## Правила

1. **В git нет секретов.** Ни ключей, ни токенов, ни паролей, ни приватных сертификатов, ни личных путей и email. Исключение — публичные идентификаторы клиентов (Twitch client_id, Trakt client_id), которые по условиям провайдера не секретны.
2. **Локально** секреты лежат в `.env.local` (в `.gitignore`). Шаблон со всеми переменными — [`.env.example`](../../.env.example); добавили переменную в код — добавьте её туда с заглушкой `changeme`.
3. **В CI** секреты лежат только в GitHub Secrets **окружений** (`release`, `store`), а не в секретах репозитория. Окружения требуют подтверждения владельца (REPO-005), поэтому job из PR до них не доберётся.
4. **OIDC вместо долгоживущих ключей** везде, где провайдер умеет: GitHub Actions получает короткий токен на один запуск.
5. **Workflow для PR** запускаются только на `pull_request` (не `pull_request_target`), с `permissions: contents: read` и без секретов (CI-002). Секреты не печатаются в лог и не передаются в шаги со сторонним кодом без нужды.
6. **Ключи провайдеров в сборках** подставляются при сборке из Secrets. Их всё равно можно извлечь из бинаря, поэтому у каждого провайдера есть поле «свой ключ» (см. [providers.md](../legal/providers.md)). Ключи, которые по условиям нельзя раздавать (TheTVDB, MAL), в сборки не попадают вовсе — только ключ пользователя.

## Где какие секреты живут

| Секрет | Где хранится | Кто использует | Примечание |
|---|---|---|---|
| Личные ключи разработчика к TMDB, Fanart.tv, OpenSubtitles, AcoustID | `.env.local` разработчика | локальные сборки и тесты | имена переменных — в `.env.example` |
| Ключи проекта к TMDB, Fanart.tv, OpenSubtitles, AcoustID | Secrets окружений `release` и `store` | релизные сборки (PKG) | регистрирует владелец (LEGAL-002) |
| Twitch client_id, Trakt client_id, LRCLIB App-Id, AniDB client | можно в открытом виде (конфиг сборки) | клиенты | **не секреты**; Twitch client secret не заводим никогда |
| Android upload keystore и пароли | Secrets окружения `store` | сборка AAB для Google Play | Play App Signing: ключ подписи приложения хранит Google |
| Google Play publishing | OIDC → Workload Identity Federation (GCP) | загрузка в Play | без JSON-ключа сервисного аккаунта, если возможно |
| App Store Connect API key (`.p8`, key id, issuer id) | Secrets окружения `store` | TestFlight, App Store, нотаризация macOS | ключ с минимальной ролью |
| Сертификаты подписи Apple (`.p12` и пароль) | Secrets окружения `store` / `release` | подпись iOS, tvOS, macOS | |
| Подпись Windows (Azure Trusted Signing) | OIDC → federated credential в Azure | MSIX, установщики | без секретов в GitHub |
| Ключ cosign для образов NAS-ОС | Secret окружения `release` (или KMS) | DIST-002 | создаёт владелец; публичный ключ — в `policy.json` образа |
| Ключ minisign для `config/` (удалённые конфиги) | Secret отдельного окружения публикации конфигов | workflow публикации конфигов | публичный ключ встроен в клиенты (CORE-014) |
| `GITHUB_TOKEN` | выдаётся GitHub на запуск | все workflow | по умолчанию только чтение; права повышаются в конкретном job |

Окружения, Secrets и OIDC-доверие настраивает владелец (REPO-005 и задачи PKG/DIST). Агенты и контрибьюторы секреты не видят и не заводят.

## Секреты пользователя на устройстве (`pa-secrets`)

Всё выше — секреты проекта и сборок. Секреты пользователя (пароли SMB/WebDAV, токены Twitch и Trakt, свой ключ TMDB) живут только на его устройстве, в системном защищённом хранилище, через крейт [`pa-secrets`](../../core/crates/pa-secrets) (CORE-007, исследование 04 §5.1). В настройки (`pa-runtime`, экспортируются в JSON), в SQLite библиотеки и в логи они не попадают.

| Платформа | Хранилище | Как подключено |
|---|---|---|
| macOS | Keychain | `keyring-core` + `apple-native-keyring-store` |
| Windows | Credential Manager (DPAPI) | `keyring-core` + `windows-native-keyring-store` |
| Linux, BSD | Secret Service (GNOME Keyring, KWallet, KeePassXC) | `keyring-core` + `zbus-secret-service-keyring-store` (чистый Rust, без OpenSSL) |
| Android | Android Keystore (AES-256) + AES-GCM, шифртексты в приватном SharedPreferences | `KeystoreSecretStore` ([`clients/android/secrets`](../../clients/android/secrets)); Flutter — плагин [`pa_platform_secrets`](../../clients/flutter/packages/pa_platform_secrets) → `secrets_open_platform` (FRB); Kotlin + UniFFI — `CorePlatformSecretStore` → `secrets_open` |
| iOS, iPadOS, tvOS | Keychain, `kSecAttrAccessibleAfterFirstUnlock`, без iCloud | `KeychainSecretStore` в SwiftPM-пакете `PACore` (UniFFI `PlatformSecretStore` → `secretsOpen(platform:)`) |

- **Ключи** — `[A-Za-z0-9._:@-]`, до 128 байт: `source.password:<id источника>`, `twitch.tokens:<user id>`, `trakt.tokens:<id>`, `provider.api_key:tmdb`. Ключ хранится открыто (имя элемента в Keychain), поэтому в нём только внутренние id — без email и логинов.
- **Значения** — до 1280 байт (предел Credential Manager); в памяти `SecretString` обнуляется при освобождении, `Debug` печатает `***`.
- **Порядок выбора** (`pa_secrets::open`, в FFI — `secrets_open`): колбэк платформы → системное хранилище → зашифрованный файл. На мобильных без колбэка — ошибка `Unavailable`, файла нет.
- **Запасной вариант** — только десктоп без хранилища (headless Linux, оконный менеджер без Secret Service): `secrets.enc` в каталоге данных ядра, XChaCha20-Poly1305, ключ — HKDF-SHA256 от случайного `secrets.key` (0600) и `/etc/machine-id`. Это защита от случайного чтения, бэкапов и копирования каталога на другую машину, но **не** от программ того же пользователя. Ядро сообщает `fallback_reason`, приложение показывает предупреждение. Когда хранилище появится, следующий `open` переносит секреты в него и удаляет файл. Повреждённый файл откладывается в `secrets.enc.corrupt`, пароли вводятся заново.
- **Мобильные (CORE-015, [ADR-NNNN](../adr/NNNN-platform-secret-stores.md)).** Android: ключ AES-256 в Keystore без требования аутентификации пользователя (фоновые задачи читают секреты, как `AfterFirstUnlock`), каждое значение — AES-GCM со случайным IV, ключ секрета — associated data; не расшифровывается (ключ Keystore потерян после переноса данных) — запись удаляется и считается отсутствующей. Tink не нужен: он делает то же поверх Keystore и добавляет ~1,5 МБ. Flutter-приложение после открытия библиотеки на Rust-ядре вызывает `openPlatformSecrets(dataDir:)`: ядро (FRB) получает Dart-колбэк, Dart пересылает вызовы по каналу `pa_platform_secrets` в Kotlin; поток ядра ждёт ответ не дольше 30 с. Функции секретов FRB (`secretGet/Set/Delete`, `secretsOpenPlatform`, `secretsOpenDefault`) — не `sync`. `CoreSources.open` берёт уже открытое хранилище. iOS-раннера у Flutter-приложения пока нет; Swift-хранилище готово в `PACore`.
- **Маскирование в логах:** `pa_secrets::redact` (в FFI — `redact_secrets`) заменяет на `***` все значения, прошедшие через хранилище в этом процессе, поля вида `password=`, `"access_token": "…"`, `api_key=`, заголовки `Bearer …`/`Basic …` и пароль в URL (`smb://user:***@host`). Логгер (CORE-008) прогоняет через неё каждую строку.

**Тесты.** `cargo test -p pa-secrets` проверяет выбор бэкенда, запасной файл и перенос из него. Тест `tests/native.rs` ходит в настоящее хранилище ОС под отдельным именем сервиса и всё за собой удаляет. Он запускается только с `PA_SECRETS_NATIVE_TEST=1` (иначе на машине разработчика запертая связка ключей покажет окно разблокировки и тест зависнет); CI задаёт переменную на macOS и Windows, а на Linux поднимает одноразовый gnome-keyring:

```bash
# Отдельный XDG_DATA_HOME: ваша настоящая связка ключей не трогается.
XDG_DATA_HOME="$(mktemp -d)" dbus-run-session -- sh -c \
  'echo test | gnome-keyring-daemon --unlock --components=secrets >/dev/null; \
   PA_SECRETS_NATIVE_TEST=1 cargo test -p pa-secrets --test native'
```

Платформенные хранилища (CORE-015): instrumented-тесты на эмуляторе — `just android secrets-test-device` (нужны `tools/bindings/build-aar.sh --debug x86_64` и `ANDROID_SERIAL`; запись, чтение, перезапись, удаление, шифрование на диске, привязка к ключу, потерянный ключ Keystore, путь через ядро UniFFI и перезапуск процесса — Android Test Orchestrator запускает каждый шаг в новом процессе). XCTest `KeychainSecretStoreTests` — в `tools/bindings/test-pacore.sh ios|tvos` (только macOS CI, workflow `apple-core.yml`); перезапуск процесса там — два отдельных запуска `xcodebuild test` шагов `KeychainRestartTests`.

`pa_secrets::open` на десктопе может показать системное окно разблокировки связки ключей и ждать ответа пользователя — вызывайте его не из UI-потока.

## Проверки

- **Локально:** хук gitleaks из [`.pre-commit-config.yaml`](../../.pre-commit-config.yaml) проверяет индексированные изменения перед коммитом и блокирует коммит с ключом. Включается один раз на клон:
  ```bash
  pipx install pre-commit   # или pip install --user pre-commit
  pre-commit install
  ```
  Ручная проверка всего дерева: `pre-commit run --all-files`.
- **CI:** job `gitleaks` в [`hygiene.yml`](../../.github/workflows/hygiene.yml) сканирует все коммиты PR, а на `main` — всю историю. Конфигурация и allowlist — [`.gitleaks.toml`](../../.gitleaks.toml) (заглушки вида `changeme` разрешены).
- **GitHub:** secret scanning и push protection в настройках репозитория (Settings → Code security) — включает владелец; push protection отклоняет `git push` с ключом известного провайдера ещё до попадания в репозиторий.

Ложное срабатывание: замените значение на заглушку или, если это действительно не секрет (тестовый вектор, публичный ключ), добавьте узкое правило в allowlist `.gitleaks.toml` с комментарием в PR. Комментарий `gitleaks:allow` в строке тоже работает, но его увидит ревьюер.

## Если секрет утёк

1. **Сразу отзовите или смените ключ** у провайдера — это главное, история git вторична.
2. Сообщите владельцу приватно (Security → Report a vulnerability, см. [SECURITY.md](../../SECURITY.md)).
3. Замените ключ в Secrets окружения или в `.env.local`.
4. Переписывать историю `main` (`git filter-repo`) имеет смысл только вместе с отзывом ключа и только по решению владельца.
