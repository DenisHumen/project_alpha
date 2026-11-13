<!-- SPDX-License-Identifier: MPL-2.0 -->

# AGENTS.md — `server/` (медиасервер)

Общие правила — в корневом [AGENTS.md](../AGENTS.md).

- **Что здесь:** медиасервер пользователя на axum и tokio (SRV) и веб-админка `server/web/`. Сервер работает у пользователя (на ПК или в NAS-ОС), облака проекта нет (ADR-0004). Использует те же крейты ядра, что и клиенты (`pa-library` и др.).
- **Workspace:** крейт сервера входит в общий Cargo workspace. Задача, создающая `server/Cargo.toml` (SRV-001), добавляет одну строку `"server"` в `members` корневого `Cargo.toml`. До этого каталог — заглушка (`just server info`).
- Контракт HTTP API — `docs/api/openapi.json` (владелец SRV).

## Сборка и тесты (из корня репозитория)

```bash
cargo check -p <server-crate>
cargo clippy -p <server-crate> --all-targets -- -D warnings
cargo nextest run -p <server-crate>
cargo run -p <server-crate> -- --help
```

`<server-crate>` — имя пакета из `server/Cargo.toml` (появится в SRV-001; обновите этот файл вместе с ним).

## Правила области

- В каждом `.rs` первой строкой `// SPDX-License-Identifier: MPL-2.0`.
- Секреты сервера (пароли пользователей, токены) — только в его конфиге и хранилище у пользователя, никогда в репозитории и логах.
- Никаких обращений к серверам проекта и телеметрии без opt-in; запросы к TMDB и другим провайдерам идут напрямую.
