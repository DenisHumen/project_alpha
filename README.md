<!-- SPDX-License-Identifier: MPL-2.0 -->

# project_alpha — Kinolune

[![Rust](https://github.com/DenisHumen/project_alpha/actions/workflows/rust.yml/badge.svg)](https://github.com/DenisHumen/project_alpha/actions/workflows/rust.yml)
[![Hygiene](https://github.com/DenisHumen/project_alpha/actions/workflows/hygiene.yml/badge.svg)](https://github.com/DenisHumen/project_alpha/actions/workflows/hygiene.yml)
[![DCO](https://github.com/DenisHumen/project_alpha/actions/workflows/dco.yml/badge.svg)](https://github.com/DenisHumen/project_alpha/actions/workflows/dco.yml)
[![License: MPL-2.0](https://img.shields.io/badge/license-MPL--2.0-blue.svg)](LICENSE)

**Русский** · [English](README.en.md)

Свободный медиаплеер для всех платформ — Linux, macOS, Windows, iOS/iPadOS, Android/Android TV, Apple TV. Рабочее название продукта — **Kinolune** (ADR-0011), репозиторий пока называется `project_alpha`.

- библиотека фильмов, сериалов и аниме с точным распознаванием по именам файлов и папок;
- воспроизведение почти любых форматов, HDR/Dolby Vision, passthrough звука;
- источники: локальные папки, SMB, NFS, WebDAV, Plex/Jellyfin/Emby, собственный медиасервер;
- музыкальный плеер (mp3, flac, alac, opus, dsd…), gapless, тексты песен;
- удобный клиент Twitch: эфиры с чатом, записи, избранные стримеры;
- продолжение просмотра на любом устройстве — без облака, всё локально;
- собственный NAS-дистрибутив Linux с простым установщиком и RAID.

Никаких серверов и аккаунтов проекта: приложение работает у вас, запросы к TMDB и другим сервисам идут прямо с устройства, телеметрии нет. Проект живёт на добровольные пожертвования.

## Статус

🚧 **Ранняя разработка, готовых сборок пока нет.** Идёт веха M0 (каркас репозитория, CI, ядро на Rust) и подготовка **релиза 0.1** для Linux-десктопа и Android: локальные папки и SMB, распознавание, медиатека с постерами, плеер, «Продолжить просмотр», базовый музыкальный плеер ([ADR-0011](docs/adr/0011-maintainer-decisions-release-0-1.md), [контракт 0.1](docs/dev/release-0.1-contract.md)).

Текущее состояние работ — [docs/STATUS.md](docs/STATUS.md), полный план — [docs/ROADMAP.md](docs/ROADMAP.md).

Скриншоты появятся вместе с первым релизом (только собственные материалы и открытые тестовые медиа, без чужого арта).

## Скачать

Сборки будут публиковаться в [GitHub Releases](https://github.com/DenisHumen/project_alpha/releases) этого репозитория. Первый релиз — 0.1 (Linux: tar.gz и AppImage; Android: APK).

## Поддержать проект

Площадки для пожертвований ещё подключаются; когда они появятся, ссылки будут здесь и на кнопке **Sponsor** репозитория. Уже сейчас очень помогают отчёты об ошибках, примеры неверного распознавания и pull request'ы.

## Документация

- [docs/STRATEGY.md](docs/STRATEGY.md) — стратегия и ключевые решения
- [docs/ROADMAP.md](docs/ROADMAP.md) — детальный план и чек-лист задач
- [docs/adr/](docs/adr/) — архитектурные решения (ADR)
- [docs/dev/architecture.md](docs/dev/architecture.md) — архитектура для разработчиков
- [docs/research/](docs/research/) — исследования
- [docs/legal/privacy-policy.md](docs/legal/privacy-policy.md) — политика конфиденциальности

## Участие

Pull request'ы приветствуются — см. [CONTRIBUTING.md](CONTRIBUTING.md) и [кодекс поведения](CODE_OF_CONDUCT.md). AI-агентам — [AGENTS.md](AGENTS.md). Вопросы и идеи — в [Discussions](https://github.com/DenisHumen/project_alpha/discussions). Уязвимости — только приватно, см. [SECURITY.md](SECURITY.md).

## Лицензия

Код — [MPL-2.0](LICENSE). Проект соответствует спецификации [REUSE](https://reuse.software/): лицензия каждого файла указана в его заголовке или в [REUSE.toml](REUSE.toml), тексты лицензий — в [LICENSES/](LICENSES/).

Kinolune не связан с Infuse/Firecore, Plex, Jellyfin, Emby, Twitch, TMDB и другими упомянутыми сервисами; их названия принадлежат владельцам.
