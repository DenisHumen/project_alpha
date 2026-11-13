<!-- SPDX-License-Identifier: MPL-2.0 -->

# AGENTS.md — `core/` (Rust-ядро)

Общие правила — в корневом [AGENTS.md](../AGENTS.md). Здесь только то, что относится к ядру.

- **Что здесь:** крейты `core/crates/pa-*` (ADR-0001, [docs/dev/architecture.md](../docs/dev/architecture.md)). Каталоги поделены между workstream CORE, NET, LIB, META, PLAY, SYNC, MUS, TWI — см. таблицу владения в ROADMAP §0.4.
- **Workspace:** один Cargo workspace на весь репозиторий (корневой `Cargo.toml`, `members` по глобу `core/crates/*`). Новый крейт в `core/crates/` корень не трогает. Зависимости — одна строка в `[workspace.dependencies]` (по алфавиту) и `dep.workspace = true` в крейте. `Cargo.lock` руками не правится.
- **Toolchain:** закреплён в `rust-toolchain.toml`; rustup поставит его при первом `cargo`.

## Сборка и тесты (из корня репозитория)

```bash
cargo check --workspace
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo nextest run --workspace          # или: cargo test --workspace
cargo nextest run -p pa-model          # один крейт
```

`cargo-nextest`: `cargo install cargo-nextest --locked`. На общей машине — `nice cargo … -j4`. Те же проверки запускает CI (`.github/workflows/rust.yml`). Добавили или переименовали крейт — обновите граф: [docs/dev/crate-graph.md](../docs/dev/crate-graph.md) проверяется workflow `docs.yml`.

## Правила области

- В каждом `.rs` первой строкой `// SPDX-License-Identifier: MPL-2.0`.
- `pa-ffi/src/lib.rs` — горячий файл: каждый домен экспортируется своим модулем `src/<domain>.rs`, в `lib.rs` добавляется одна строка `mod`.
- Миграции `pa-library/migrations/`: имя `YYYYMMDDHHMM_<ID>_<slug>.sql`, только expand→contract.
- Сгенерированные биндинги UniFFI не коммитятся, кроме того, что требует Xcode. Привязки FRB (`pa-frb/src/frb_generated.rs`, `pa_core/lib/src/rust/`) коммитятся и проверяются CI — ADR-0013; регенерация `just core frb`.
- Зависимости проходят `cargo deny` (CI-003): GPL, AGPL и SSPL в графе клиентских крейтов запрещены.
- Публичный API документируется rustdoc.
