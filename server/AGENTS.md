<!-- SPDX-License-Identifier: MPL-2.0 -->

# AGENTS.md — `server/` (медиасервер)

Общие правила — в корневом [AGENTS.md](../AGENTS.md).

- **Что здесь:** медиасервер пользователя на axum и tokio (SRV) и веб-админка `server/web/`. Сервер работает у пользователя (на ПК или в NAS-ОС), облака проекта нет (ADR-0004). Использует те же крейты ядра, что и клиенты (`pa-library` и др.).
- **Workspace:** крейт `pa-server` (бинарь `alpha-media`, как юнит NAS-ОС по ADR-0010) входит в общий Cargo workspace (`"server"` в `members` корневого `Cargo.toml`).
- **Устройство:** `pa-runtime` даёт tokio-runtime, каталоги и настройки; `pa-library` — БД медиатеки `library.sqlite3` (SQLite WAL, та же схема и миграции, что в клиенте). Сервер: конфиг (`src/config.rs`), axum-роутер (`src/http.rs`), структурные логи `tracing` (text или JSON, `src/logging.rs`), graceful shutdown по Ctrl-C/SIGTERM с checkpoint WAL. Хаб синхронизации (`src/sync/`, SYNC-008): WebSocket `/api/v1/sync` по протоколу `docs/api/sync-protocol.md` (§8 — детали хаба), журнал в `sync.sqlite3` рядом с медиатекой, «сейчас играет» — только в памяти.
- **Конфиг:** TOML-файл (`--config` или `ALPHA_MEDIA_CONFIG`), поверх — `ALPHA_MEDIA_LISTEN`, `ALPHA_MEDIA_DATA_DIR`, `ALPHA_MEDIA_LOG`, `ALPHA_MEDIA_LOG_FORMAT` (`alpha-media --help`); секция `[sync]` — `playback_ttl_secs`, `playback_max_entries`, `max_connections`. По умолчанию слушает только `127.0.0.1:8096`: наружу — явной настройкой, пока нет auth (SRV-003) и TLS (SRV-004).
- **TLS** — только rustls (OpenSSL запрещён в `deny.toml`).
- Контракт HTTP API — `docs/api/openapi.json` (владелец SRV).

## Сборка и тесты (из корня репозитория)

```bash
cargo check -p pa-server
cargo clippy -p pa-server --all-targets -- -D warnings
cargo nextest run -p pa-server
cargo run -p pa-server -- --help
just server run          # состояние в target/alpha-media
curl -s localhost:8096/health
```

## Правила области

- В каждом `.rs` первой строкой `// SPDX-License-Identifier: MPL-2.0`.
- Секреты сервера (пароли пользователей, токены) — только в его конфиге и хранилище у пользователя, никогда в репозитории и логах.
- Никаких обращений к серверам проекта и телеметрии без opt-in; запросы к TMDB и другим провайдерам идут напрямую.
