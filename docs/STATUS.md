# Текущее состояние работ

> Файл для передачи работы между компьютерами и сессиями. Обновляется мейнтейнером после каждой волны/слияния.
> Источник истины по задачам — [ROADMAP.md](ROADMAP.md) (галочки, `just roadmap next`) и открытые PR. Решения — [adr/](adr/).

Обновлено: 2026-11-14, ~01:45 UTC

## Приоритет

**Релиз 0.1 — Kinolune** (ADR-0011, контракт [dev/release-0.1-contract.md](dev/release-0.1-contract.md)): Linux + Android, локальные папки (SMB на Linux — бета), распознавание, медиатека, плеер (media_kit, ADR-0012), «Продолжить просмотр», музыка. Сборка в CI → GitHub Releases по тегу `app-v*`. NAS-дистрибутив — последним.

## Релизы

- **[Kinolune 0.1.0-alpha.1](https://github.com/DenisHumen/project_alpha/releases/tag/app-v0.1.0-alpha.1)** (2026-11-13, пре-релиз): Linux AppImage/tar.gz, Android APK (arm64, armv7, x86_64, universal; debug-подпись). Linux требует системный libmpv.
- **[Kinolune 0.1.0-alpha.2](https://github.com/DenisHumen/project_alpha/releases/tag/app-v0.1.0-alpha.2)** (2026-11-14): наш LGPL libmpv внутри AppImage и APK.
- **[Kinolune 0.1.0-alpha.3](https://github.com/DenisHumen/project_alpha/releases/tag/app-v0.1.0-alpha.3)** (2026-11-14): медиатека на Rust-ядре по умолчанию (импорт базы v0 с резервной копией), метаданные TMDB/AniList/Shikimori в ядре, сессия воспроизведения (resume, Up Next), структура папок (диски, части, extras).

## Готово и в main

- Инфраструктура: монорепо, `just`, `just roadmap next|lint`, bootstrap/Dev Container, REUSE, gitleaks, cargo-deny/audit, Dependabot, DCO + линт заголовков PR, метки.
- Ядро (Rust): `pa-model`, `pa-ffi` (UniFFI), `pa-frb` + Dart-пакет `pa_core` (flutter_rust_bridge, Native Assets), `pa-runtime`, `pa-vfs`, `pa-library` (схема SQLite v1), `pa-recognition` (парсер имён, META-005), `pa-anime-mappings`, `pa-eval` + формат корпуса.
- Клиент 0.1: `pa_library` (Dart: сканер, парсер v0, TMDB, SQLite, прогресс), `pa_player` (media_kit: видео-OSD, музыка), `pa_design` (токены, иконки Lucide, шрифт Inter), бренд Kinolune (иконки, баннер TV, заглушки).
- Сборки: LGPL FFmpeg для 11 таргетов (CI), CI выпуска приложения (AppImage/tar.gz, APK), CI образа NAS-ОС с подписью, определение дисков установщика.
- Приложение Kinolune (`clients/flutter/app`, R01-APP) — все экраны 0.1, скриншоты в docs/screenshots/0.1.
- LIB-016: refinery удалён, rusqlite 0.42 (закрыт CVE встроенного SQLite).
- Документы: стратегия, 16 ADR, архитектура, тест-стратегия, генератор тестовых медиа, юридические таблицы, политика конфиденциальности, скриншоты прогресса.

## В работе (агенты, ветки на GitHub)

META-013 остаток (распознавание по группам LIB-007, NFO-id, замер точности на корпусе) — на паузе до сброса 5-часового лимита. Открытые PR: `gh pr list`.

Слито 14 ноября: LIB-007 (структура папок), UI-024 (приложение на Rust-ядре), META-027 (метаданные в ядре; остаток META-013 ждёт LIB-007 и замера точности), PLAY-005 (сессия воспроизведения), NET-011 (менеджер источников `pa-sources`), LIB-006 (`pa-probe`, ADR-0034), PLAY-APL-001 (AlphaPlayerKit на MPVKit, ADR-0035; релизы Apple заблокированы до PLAY-APL-013 — FFmpeg MPVKit под LGPL-3.0), NET-002/003/004, SYNC-006, CORE-007, TV-001, PLAY-002, PLAY-013, CORE-003.

Сделано за 13–14 ноября (кроме перечисленного выше): pa-vfs, pa-runtime, pa-library (схема, прогресс, «Продолжить», Up Next), pa-scanner, pa-sync (модель, протокол), хаб синхронизации на сервере, pa-secrets, pa-discovery (mDNS), pa-playback (контракт плеера), pa-metadata (TMDB, AniList, Shikimori), парсер 0.2 + индекс тайтлов, корпус v0 (2035 групп), музыка (схема + теги), Twitch (Helix + вход), LGPL libmpv на 7 платформ, xcframework (Apple) и AAR (Android), локализация ru/en, исправления UI по скриншотам.

## Политика CI (важно)

CI на PR не запускается для черновиков; для готовых PR — при открытии / пометке «ready». Проверки перед слиянием — на сервере сборки (fmt, clippy, тесты, cargo-deny, reuse, Flutter). Тяжёлые сборки (FFmpeg, libmpv, образ ОС) — вручную (`gh workflow run <file> --ref <branch>`) или по тегу. Проверки — локально или на сервере сборки перед пометкой PR готовым.

## Как продолжить с другого ПК

1. `git clone https://github.com/DenisHumen/project_alpha && cd project_alpha && just bootstrap check`
2. Прочитать `AGENTS.md`/`CLAUDE.md`, этот файл, `docs/adr/0011`, `docs/dev/release-0.1-contract.md`.
3. `gh pr list` — доделать черновики выше; `just roadmap next` — следующие задачи.
4. Одна задача — одна ветка `feat/<ID>-slug`, `git commit -s`, PR с `[<ID>]`.

## Нужно от владельца

- Ключ TMDB (постеры/описания) — пока приложение работает без него по распознанным названиям.
- Ключ подписи Android (иначе APK подписаны debug-ключом) и cosign-ключ для образа ОС — по готовности к релизу.
