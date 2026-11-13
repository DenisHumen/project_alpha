<!-- SPDX-License-Identifier: MPL-2.0 -->

# ADR-0018: Обнаружение в LAN — `mdns-sd` в ядре, NWBrowser на Apple, свои типы DNS-SD

- **Статус:** Принят
- **Дата:** 2026-11-14
- **Решают:** владелец проекта (мейнтейнер); участники обсуждения — по PR
- **Связано:** `NET-009`, `NET-020`, `SRV-004`, `SYNC-006`, `docs/research/04` §1.6, `docs/research/02` §4, [ADR-0004](0004-no-project-servers-local-sync.md)

## Контекст и проблема

Клиенту нужно находить в домашней сети: наш медиасервер (режим B), другие устройства для P2P-синхронизации (ADR-0004) и файловые шары (SMB, WebDAV, NFS) для режима A. Исследование 04 §1.6 предлагает DNS-SD с типом `_projectalpha._tcp` и `mdns-sd`. Нужно зафиксировать: чем браузить на каждой платформе, как называются наши типы, что лежит в TXT и как это тестировать без сети.

Ограничения: RFC 6763 §7.2 — имя сервиса не длиннее 15 символов (имя `_projectalpha-sync._tcp` из черновика SYNC-006 не проходит); на iOS и tvOS сырой multicast требует entitlement `com.apple.developer.networking.multicast`, который Apple выдаёт вручную; на Android Wi-Fi отбрасывает multicast без `MulticastLock`; зависимости — только MIT/Apache без OpenSSL (ADR-0005, `deny.toml`).

## Факторы решения

- Один код на Linux, Windows, macOS и Android.
- Работа на iOS и tvOS без multicast-entitlement.
- Тесты в CI без реальной сети.
- Совместимость с Avahi, Bonjour и NAS (Synology, QNAP, TrueNAS), которые уже анонсируют `_smb._tcp`.

## Рассмотренные варианты

1. `mdns-sd` в ядре везде, на Apple — тоже.
2. `mdns-sd` в ядре на Linux, Windows, macOS и Android; на iOS и tvOS — `NWBrowser` через колбэк в ядро.
3. Только платформенные API (Avahi D-Bus, Bonjour, `NsdManager`, DNS-SD Windows).

## Решение

Выбран вариант **2**: крейт `pa-discovery`.

- **Модель и логика — в ядре, без I/O:** `ServiceKind`, `DiscoveredService`, разбор TXT, `Registry` (дедупликация повторных резолвов, found/updated/lost), фасад `Discovery` с подписками и снапшотом. Свой сервер или своё устройство отфильтровываются по TXT `id`.
- **Бэкенд `MdnsBackend`** (фича `mdns`, по умолчанию): `mdns-sd` 0.21 без фич по умолчанию — pure Rust, MIT OR Apache-2.0, без OpenSSL и без async-рантайма. На Android приложение передаёт `MulticastLock` (колбэк над `WifiManager.MulticastLock`); ядро держит его, пока идёт хотя бы один браузинг. `Advertiser` анонсирует наши типы (сервер — `SRV-004`, пиры — `SYNC-006`) с отдельным именем хоста `pa-<id>.local.`, чтобы не отвечать за имя хоста ОС.
- **Бэкенд `PlatformBackend`:** приложение реализует `PlatformBrowser` (на iOS и tvOS — `NWBrowser`) и пишет результаты в `PlatformSink`. `NsdManager` на Android не используем: `mdns-sd` с `MulticastLock` даёт один код с десктопом и TXT без ограничений старых API.
- **Типы DNS-SD** (константы в `ServiceKind`):

| Тип | Что | TXT |
|---|---|---|
| `_projectalpha._tcp` | наш медиасервер | `txtvers=1`, `id` (UUID сервера), `v` (мажор API), `name`, `fp` (отпечаток TLS/ключа, формат задаёт SRV-004) |
| `_alpha-sync._tcp` | приложение для LAN-синхронизации | `txtvers=1`, `id` (`device_id`), `v` (мажор протокола синхронизации), `dev` (`server`, `desktop`, `tv`, `mobile` — порядок выбора хаба), `name`, `fp` (ключ из pairing) |
| `_smb._tcp`, `_webdav._tcp`, `_webdavs._tcp`, `_nfs._tcp` | чужие шары | `path` у WebDAV и NFS |

  `txtvers` больше поддерживаемого — запись отвергается, чтобы старый клиент не читал новую схему неверно. Имя `_projectalpha` остаётся, хотя продукт называется Kinolune (ADR-0011): пакеты и идентификаторы до проверки товарных знаков не переименовываются, а смена типа DNS-SD ломает обнаружение уже установленных серверов. При переименовании клиенты один релиз браузят оба типа.
- **Тесты:** юнит-тесты на модели, реестре и фасаде с подставным бэкендом; интеграционные — настоящий `mdns-sd`, но только loopback и частный UDP-порт (не 5353), поэтому ни сеть, ни системный Avahi не участвуют. Тест в реальной LAN (`lan_smoke`) помечен `#[ignore]` и запускается вручную.

Не входит: WS-Discovery, NetBIOS, Plex GDM, Jellyfin UDP (`NET-010`); TLS-pinning по `fp` (`SRV-004`); транспорт синхронизации (`SYNC-006`); код на Kotlin и Swift и `Info.plist` (`NET-020`).

### Последствия

- Хорошо: одна реализация на четыре платформы; на Apple — только `NSLocalNetworkUsageDescription` и `NSBonjourServices` (список даёт `bonjour_services()`), без entitlement.
- Хорошо: тесты детерминированы и не требуют сети и прав.
- Плохо: на Apple обнаружение зависит от кода приложения (`NET-020`); результаты `NWBrowser` без резолва содержат только имя и TXT, адрес получает соединение.
- Плохо: `mdns-sd` поднимает свой поток и сокеты рядом с системным респондером (Avahi, Bonjour); на 5353 это штатно (SO_REUSEPORT), но ответов на хосте становится два.
- Требует работы: `NET-020` — обвязка на Android и Apple; `SRV-004` — анонс сервера через `Advertiser`; `SYNC-006` — тип `_alpha-sync._tcp`.

## Ссылки

- `docs/research/04-backend-architecture-billing.md` §1.6
- `docs/research/02-client-stack-playback.md` §4 (Local Network Privacy)
- RFC 6762 (mDNS), RFC 6763 (DNS-SD), §6 (TXT), §7.2 (длина имени сервиса)
- <https://github.com/keepsimple1/mdns-sd>
