# Текущее состояние работ

> Файл для передачи работы между компьютерами и сессиями. Обновляется мейнтейнером после каждой волны/слияния.
> Источник истины по задачам — [ROADMAP.md](ROADMAP.md) (галочки, `just roadmap next`) и открытые PR. Решения — [adr/](adr/).

Обновлено: 2026-11-13, ~19:45 UTC

## Приоритет

**Релиз 0.1 — Kinolune** (ADR-0011, контракт [dev/release-0.1-contract.md](dev/release-0.1-contract.md)): Linux + Android, локальные папки (SMB на Linux — бета), распознавание, медиатека, плеер (media_kit, ADR-0012), «Продолжить просмотр», музыка. Сборка в CI → GitHub Releases по тегу `app-v*`. NAS-дистрибутив — последним.

## Готово и в main

- Инфраструктура: монорепо, `just`, `just roadmap next|lint`, bootstrap/Dev Container, REUSE, gitleaks, cargo-deny/audit, Dependabot, DCO + линт заголовков PR, метки.
- Ядро (Rust): `pa-model`, `pa-ffi` (UniFFI), `pa-frb` + Dart-пакет `pa_core` (flutter_rust_bridge, Native Assets), `pa-runtime`, `pa-vfs`, `pa-library` (схема SQLite v1), `pa-recognition` (парсер имён, META-005), `pa-anime-mappings`, `pa-eval` + формат корпуса.
- Клиент 0.1: `pa_library` (Dart: сканер, парсер v0, TMDB, SQLite, прогресс), `pa_player` (media_kit: видео-OSD, музыка), `pa_design` (токены, иконки Lucide, шрифт Inter), бренд Kinolune (иконки, баннер TV, заглушки).
- Сборки: LGPL FFmpeg для 11 таргетов (CI), CI выпуска приложения (AppImage/tar.gz, APK), CI образа NAS-ОС с подписью, определение дисков установщика.
- Документы: стратегия, 16 ADR, архитектура, тест-стратегия, генератор тестовых медиа, юридические таблицы, политика конфиденциальности, скриншоты прогресса.

## Открытые PR (работа в процессе)

| PR | Задача | Состояние |
|---|---|---|
| #28 | R01-APP: приложение Kinolune (экраны, навигация, скриншоты) | черновик, агент на паузе; нужно подключить реальные `pa_library`/`pa_player`/`pa_design`/бренд и снять скриншоты |
| #33 | PLAY-002: LGPL libmpv | черновик; Android собирается, Linux/macOS/Windows — чинить (на сервере сборки) |
| #37 | META-002: корпус распознавания v0 | черновик; gitleaks ложные срабатывания на хэшах |

## Политика CI (важно)

CI на PR запускается только при открытии / пометке «ready», не на каждый пуш. Тяжёлые сборки (FFmpeg, libmpv, образ ОС) — вручную (`gh workflow run <file> --ref <branch>`) или по тегу. Проверки — локально или на сервере сборки перед пометкой PR готовым.

## Как продолжить с другого ПК

1. `git clone https://github.com/DenisHumen/project_alpha && cd project_alpha && just bootstrap check`
2. Прочитать `AGENTS.md`/`CLAUDE.md`, этот файл, `docs/adr/0011`, `docs/dev/release-0.1-contract.md`.
3. `gh pr list` — доделать черновики выше; `just roadmap next` — следующие задачи.
4. Одна задача — одна ветка `feat/<ID>-slug`, `git commit -s`, PR с `[<ID>]`.

## Нужно от владельца

- Ключ TMDB (постеры/описания) — пока приложение работает без него по распознанным названиям.
- Ключ подписи Android (иначе APK подписаны debug-ключом) и cosign-ключ для образа ОС — по готовности к релизу.
