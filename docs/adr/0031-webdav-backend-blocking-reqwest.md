# ADR-0031: WebDAV-бэкенд — блокирующий reqwest, свои редиректы и Digest, pinning сертификата

- **Статус:** Предложен
- **Дата:** 2026-11-14
- **Решают:** владелец проекта (мейнтейнер); участники обсуждения — по PR
- **Связано:** NET-004, NET-011, CI-012; [ADR-0013](0013-pa-vfs-blocking-positional-reads.md) (синхронный `pa-vfs`), [ADR-0018](0018-metadata-providers-blocking-http.md) (ureq в `pa-metadata`), [ADR-0029](0029-server-tls-pinning-port.md) (pinning нашего сервера); `docs/research/02` §4

## Контекст и проблема

NET-004 добавляет первый сетевой бэкенд `pa-vfs`: WebDAV (Nextcloud, ownCloud, NAS, Apache, nginx, `rclone serve`). Трейт `Vfs` синхронный (ADR-0013). Нужно выбрать HTTP-клиент, понять, где живёт бэкенд, как обходить различия серверов (редиректы на коллекциях, Digest у Apache, сервер без `Range`) и как работать с самоподписанным сертификатом NAS.

## Факторы решения

- Синхронный трейт, чтение из нескольких потоков одного файла, пул соединений.
- Свой верификатор сертификата (TOFU) — уже есть `PinnedVerifier` в `pa-discovery` (SRV-004).
- Без OpenSSL и aws-lc (`deny.toml`), один TLS-стек на всех платформах.
- `PROPFIND` при редиректе должен остаться `PROPFIND`.

## Рассмотренные варианты

1. `reqwest::blocking` (rustls + ring, `tls_backend_preconfigured`) — как задумано в ROADMAP.
2. `ureq` 3, как в `pa-metadata` (ADR-0018).
3. Async `reqwest` + `block_on` на каждый вызов.

## Решение

Выбран вариант **1**.

- Крейт `core/crates/pa-vfs-webdav` (workstream NET): `pa-vfs` остаётся без зависимостей, бэкенд подключают только те, кому нужен WebDAV.
- `reqwest::blocking` с `rustls-no-provider` и провайдером ring. Для pinning — готовый `rustls::ClientConfig` с `pa_discovery::PinnedVerifier`. `ureq` 3 не даёт подставить свой верификатор сертификата без собственного транспорта; reqwest уже есть в дереве (`pa-twitch`).
- Редиректы обрабатываются вручную (`Policy::none`): до 10 переходов, метод и тело сохраняются, учётные данные уходят только на origin источника. Причина: заголовок Digest считается от URI запроса, и на каждом переходе его нужно пересчитать (и при необходимости ответить на новый `401`), а встроенный обход редиректов reqwest повторяет старый заголовок. Hrefs ответа после редиректа сопоставляются с новым корнем.
- Авторизация: первый запрос без учётных данных, по `401` выбирается Digest SHA-256 → Digest MD5 → Basic (RFC 7616/7617, `-sess`, `qop=auth`), дальше заголовок отправляется сразу; `stale=true` — повтор с новым nonce. MD5 нужен только для Digest (протокол), это не криптографическая защита данных.
- Чтение: ranged `GET`, read-ahead блок (по умолчанию 1 МиБ) и до 4 блоков на файл; сервер без `Range` поддерживается пропуском префикса. Размер без `getcontentlength` — через `HEAD`.
- TLS: по умолчанию системные корни (как в `pa-twitch`). Если сертификат не принят, ошибка помечена `UntrustedCertificate`; приложение показывает отпечаток из `fetch_fingerprint` (рукопожатие без HTTP-запроса и без пароля) и после подтверждения хранит `TlsTrust::Pinned(sha256)` вместе с источником (NET-011).
- Тесты: контракт-сьют против встроенного тестового сервера в каждом `cargo test` (Basic, Digest со `stale`, редиректы, без `Range`, без размеров, TLS) и против nginx, Apache, rclone и Nextcloud в Docker (`tests/servers/run.sh`, фича `server-tests`).

Не входит: запись (PUT/MKCOL), NTLM/Kerberos, клиентские сертификаты, prefetch в фоне (появится с NET-003 как общий слой, если понадобится), подключение Docker-тестов к CI (CI-012).

### Последствия

- Хорошо: один бэкенд проходит контракт на четырёх реальных серверах; pinning общий с нашим сервером.
- Плохо: `reqwest::blocking` держит свой поток с tokio-рантаймом на клиента и падает, если его создать или уничтожить внутри async-контекста — вызывать из рабочих потоков или `spawn_blocking` (как и требует ADR-0013).
- Плохо: на Android системный верификатор (`rustls-platform-verifier`) требует инициализации через JNI — та же задача, что у `pa-twitch`; до неё на Android работает режим pinning.
- Нейтрально: в дереве два HTTP-клиента (ureq в `pa-metadata`, reqwest в сетевых бэкендах и `pa-twitch`).

## Ссылки

- RFC 4918 (WebDAV), RFC 7616 (Digest), RFC 7617 (Basic), RFC 7233 (Range)
- `core/crates/pa-vfs-webdav/` (rustdoc), `core/crates/pa-vfs-webdav/tests/servers/README.md`
