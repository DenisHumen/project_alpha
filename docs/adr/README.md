# Architecture Decision Records (ADR)

Здесь записаны архитектурные и организационные решения проекта: что решили, почему и какой ценой. Формат — в духе [MADR](https://adr.github.io/madr/), шаблон — [`0000-template.md`](0000-template.md).

## Индекс

| № | Решение | Статус | Задачи |
|---|---|---|---|
| [0001](0001-rust-core-uniffi-frb.md) | Общее ядро на Rust с привязками UniFFI и flutter_rust_bridge | Принят | CORE-001…010 |
| [0002](0002-flutter-ui-swiftui-tvos.md) | UI на Flutter для шести платформ, SwiftUI для tvOS | Принят | UI-001, TV-* |
| [0003](0003-native-player-surfaces-engines.md) | Нативная поверхность плеера и гибридные движки воспроизведения | Принят | PLAY-*, PLAY-AND-*, PLAY-APL-*, PLAY-DSK-* |
| [0004](0004-no-project-servers-local-sync.md) | Без серверов проекта; синхронизация через устройства и хранилища пользователя | Принят | SYNC-*, SRV-*, CORE-014 |
| [0005](0005-license-mpl2-dco.md) | Лицензия кода MPL-2.0 и DCO вместо CLA | Принят (DEC-002) | REPO-002 |
| [0006](0006-public-monorepo-layout-ownership.md) | Один публичный монорепо, раскладка каталогов и владение ими | Принят (DEC-001) | REPO-001, REPO-011, META-001 |
| [0007](0007-store-full-build-flavors.md) | Два флейвора сборки — `store` и `full` | Принят; донаты в store — предложен (DEC-003) | CORE-013, PKG-014, PKG-015 |
| [0008](0008-android-player-embedding.md) | Встраивание плеера в Android: Activity + Flutter OSD, запасной — нативные контролы | Предложен (нужны замеры на реальном Android TV) | PLAY-AND-001 |
| [0009](0009-desktop-player-embedding.md) | Встраивание libmpv во Flutter-десктоп: texture по умолчанию, нативное окно для HDR | Предложен (нужны замеры Windows/macOS) | PLAY-DSK-001, PLAY-DSK-009 |
| [0010](0010-nas-os-phase0.md) | NAS-ОС, фаза 0: bootc-основа, Go для установщика и демонов, медиасервер в образе | Принят | DIST-001 |
| [0011](0011-maintainer-decisions-release-0-1.md) | Решения мейнтейнера: релиз 0.1 первым, Kinolune, Twitch store/full, TMDB, Penpot | Принят | DEC-003…008 |
| [0012](0012-release-0-1-player-media-kit.md) | media_kit как плеер релиза 0.1 | Принят | R01-PLAYER |
| [0013](0013-pa-vfs-blocking-positional-reads.md) | `pa-vfs`: синхронные объектно-безопасные трейты и позиционное чтение | Принят | NET-001 |
| [0014](0014-core-runtime-settings-rusqlite.md) | Процессный runtime ядра, настройки в SQLite через rusqlite | Принят | CORE-006 |
| [0015](0015-frb-native-assets-committed-bindings.md) | pa_core через Native Assets, привязки FRB в git | Принят | CORE-002 |
| [0016](0016-library-sqlite-schema-refinery.md) | Медиатека в SQLite: одна схема, миграции refinery | Принят | LIB-001 |
| [0017](0017-player-engine-contract.md) | Контракт `PlayerEngine` в ядре: события как источник состояния, выбор движка по capability-матрице | Принят | PLAY-003 |
| [0018](0018-metadata-providers-blocking-http.md) | Провайдеры метаданных: трейт `Provider` в ядре, блокирующий HTTPS в `pa-metadata` | Принят | META-009 |
| [0019](0019-lan-discovery-mdns.md) | Обнаружение в LAN: `mdns-sd` в ядре, NWBrowser на Apple, свои типы DNS-SD | Принят | NET-009, NET-020 |
| [0020](0020-music-tags-lofty.md) | Музыкальные теги в ядре через `lofty`, альбомы по ключу группировки | Предложен | MUS-001 |
| [0021](0021-anime-providers-graphql-post.md) | Аниме-провайдеры AniList и Shikimori: GraphQL `POST` в общем транспорте, общий конвейер запросов | Предложен | META-010 |
| [0022](0022-local-title-index.md) | Локальный индекс названий: свой индекс в памяти в `pa-recognition`, синонимы anime-offline-database в бандле | Предложен | META-007 |
| [0023](0023-tvos-xcodegen-project.md) | Xcode-проект tvOS-приложения генерируется XcodeGen из `project.yml` | Предложен | TV-001 |
| [0024](0024-music-audio-mode-libmpv.md) | Музыка v1: libmpv в аудиорежиме, ReplayGain из тегов файла, проверка gapless по сэмплам | Предложен | MUS-002 |
| [0025](0025-library-search-fts5.md) | Поиск по медиатеке: SQLite FTS5 с ключами из Rust и очередью на триггерах | Предложен | LIB-008 |
| [0026](0026-device-family-pairing.md) | Семья устройств: ключи Ed25519/X25519, pairing через SPAKE2, подписанный trust store | Предложен | SYNC-005 |
| [0027](0027-player-io-adapters-fn-pointers.md) | Адаптеры ByteSource для FFmpeg и mpv не линкуют библиотеки, а получают функции указателями | Предложен | NET-008 |
| [0028](0028-pa-jobs-persistent-queue.md) | Очередь задач `pa-jobs`: таблица `jobs` в базе медиатеки, блокирующие обработчики на потоках | Предложен | CORE-011 |
| [0029](0029-server-tls-pinning-port.md) | Медиасервер: HTTPS с самоподписанным сертификатом, pinning по `fp` из mDNS, порт 8487 | Предложен | SRV-004 |
| [0030](0030-lan-sync-transport.md) | LAN-синхронизация: WebSocket поверх TLS 1.3 с привязкой к ключам сопряжения, хаб по классу устройства | Предложен | SYNC-006 |
| [0031](0031-webdav-backend-blocking-reqwest.md) | WebDAV-бэкенд: блокирующий reqwest, свои редиректы и Digest, pinning самоподписанного сертификата | Предложен | NET-004 |
| [0032](0032-app-library-on-rust-core.md) | Медиатека приложения на Rust-ядре: крейт `pa-catalog`, адаптер `RustLibrary`, отпечаток OSHash | Предложен | UI-024 |
| [0033](0033-libsmb2-runtime-loading.md) | SMB через libsmb2: загрузка в рантайме, пул контекстов, read-ahead с параллельным prefetch | Предложен | NET-002, NET-003 |
| [0034](0034-pa-probe-libavformat-runtime.md) | `pa-probe`: libavformat в рантайме, зеркало префиксов структур, окно чтения «голова + хвост» | Предложен | LIB-006 |
| [0035](0035-apple-engines-foreign-player-engine-mpvkit.md) | Apple-движки — Swift-реализации `PlayerEngine` через UniFFI foreign trait; MPVKit только LGPL с точной версией | Предложен | PLAY-APL-001 |
| [0036](0036-image-cache-in-core.md) | Кэш изображений в ядре (`pa-imagecache`): размеры через URL провайдера, LRU по mtime, декодирование во Flutter | Предложен | LIB-013 |
| [0037](0037-platform-secret-stores.md) | Платформенные хранилища секретов: Keystore + AES-GCM без Tink, Keychain `AfterFirstUnlock`, Dart-колбэк FRB для Flutter | Предложен | CORE-015 |
| [0038](0038-network-playback-loopback-proxy.md) | Сетевые источники: скан через `pa-vfs` (`Connector` в `pa-catalog`), воспроизведение через loopback HTTP Range-прокси `pa-proxy` | Предложен | LIB-018, NET-007 |
| [NNNN](NNNN-remote-config-minisign.md) | Удалённый конфиг: подписанный minisign JSON из публичного репо, `minisign-verify`, anti-rollback по `serial`, только встроенная копия до появления ключа | Предложен | CORE-014 |

## Запланированные ADR

Решения владельца, которые ещё не приняты, оформляются ADR в статусе «Предложен» с рекомендацией и получают следующий свободный номер (сейчас 0039) в момент написания: DEC-003 (донаты в сторах), DEC-004 (название и бренд), DEC-005 (кодеки и состав FFmpeg на сервере), DEC-006 (Twitch без логина), DEC-007 (аккаунты и юрисдикция), DEC-008 (ключ TMDB в открытом коде). Рекомендации по DEC-003, DEC-006 и DEC-008 уже записаны в разделах «Открытые вопросы» ADR-0007 и ADR-0004.

## Статусы

- **Предложен** — написан, ждёт решения владельца.
- **Принят** — действует; код и задачи должны ему соответствовать.
- **Отклонён** — рассмотрен и не принят (файл остаётся для истории).
- **Устарел** — больше не актуален, замены нет.
- **Заменён ADR-NNNN** — действует новое решение.

## Как добавить или изменить решение

1. Скопируйте `0000-template.md` в `NNNN-kebab-slug.md` со следующим свободным номером (номера не переиспользуются).
2. Заполните разделы, статус — «Предложен». Добавьте строку в индекс выше.
3. Откройте PR по правилам [CONTRIBUTING.md](../../CONTRIBUTING.md); если ADR закрывает задачу ROADMAP — её ID в заголовке.
4. Решение принимает владелец. При мерже статус меняется на «Принят».
5. Принятый ADR не переписывают. Чтобы поменять решение, пишут новый ADR, а у старого меняют только статус на «Заменён ADR-NNNN».

Язык ADR — русский; имена крейтов, команд, флагов и идентификаторы — как в коде.
