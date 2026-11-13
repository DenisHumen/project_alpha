<!-- SPDX-License-Identifier: MPL-2.0 -->

# AGENTS.md — инструкции для AI-агентов

Общие правила для любого агента (Claude Code, Codex, Cursor, Copilot и др.), который работает в этом репозитории. Правила владельца и заметки для Claude Code — в [CLAUDE.md](CLAUDE.md); для людей — [CONTRIBUTING.md](CONTRIBUTING.md). В каталогах областей лежат свои короткие `AGENTS.md` с командами сборки и тестов: [core/](core/AGENTS.md), [clients/flutter/](clients/flutter/AGENTS.md), [clients/apple/](clients/apple/AGENTS.md), [server/](server/AGENTS.md), [distro/](distro/AGENTS.md). Ближайший к изменяемому файлу `AGENTS.md` уточняет этот.

## Проект за минуту

Свободный медиаплеер для всех платформ (MPL-2.0): Rust-ядро (`core/`), Flutter-приложение (`clients/flutter/`), нативные плееры (`clients/{android,apple,desktop}/`), медиасервер (`server/`), NAS-ОС (`distro/`). Своих серверов и облака у проекта нет: всё работает у пользователя локально. Решения — [docs/STRATEGY.md](docs/STRATEGY.md) и [docs/adr/](docs/adr/), backlog — [docs/ROADMAP.md](docs/ROADMAP.md).

## Как взять задачу

1. `git fetch origin`. Найдите в [docs/ROADMAP.md](docs/ROADMAP.md) задачу, у которой все `deps` отмечены `[x]` в `main` и нет открытого PR:
   ```bash
   gh pr list --search "[CORE-012] in:title" --state open
   ```
   Задачи `(owner)` и `(blocked: DEC-…)` не брать.
2. Прочитайте «Что», «done when» и `ref:` задачи, ADR по теме и правила §0.3–0.5 ROADMAP.
3. Отдельный worktree и ветка от свежего `main`:
   ```bash
   git worktree add ../pa-CORE-012 -b feat/CORE-012-hlc-clock origin/main
   ```
   Ветка `<type>/<ID>-<slug>`, type ∈ `feat|fix|docs|ci|build|chore|test|refactor|perf`. Несколько задач в одном PR — ветка по первому ID, все ID в заголовке.
4. Сразу откройте draft PR (он «занимает» задачу): заголовок `<type>(<scope>): <summary> [<ID>]`, тело — по [шаблону](.github/pull_request_template.md).
5. Меняйте только каталоги своего workstream (ROADMAP §0.4). Горячие файлы (§0.5: корневые `Cargo.toml`, `justfile`, `pubspec.yaml`, роутер, `.arb`, `ROADMAP.md`) — только минимальные добавляющие правки по правилам таблицы. Нужна правка в чужом каталоге — минимальная и с пометкой в PR.
6. В том же PR отметьте `[x]` только у своей задачи. Новая работа — новая задача в конце своего раздела отдельным коммитом `docs(roadmap): add <ID>`.
7. Перед готовностью: rebase на `origin/main`, все проверки ниже зелёные, в описании PR честно указано, что проверено и на каких платформах, а что нет.

## Коммиты

- [Conventional Commits](https://www.conventionalcommits.org/) на английском; scope — workstream в нижнем регистре (`core`, `ui`, `play-and`, `repo`, `ci`…).
- Всегда `git commit -s` (DCO). `Signed-off-by` — человек, который запустил агента и отвечает за вклад. Агент указывается трейлером `Co-Authored-By: <агент> <email>`.
- Не переписывать чужие коммиты и `main`, не делать force-push в `main`, не мержить свои PR, не запускать релизные workflow (окружения `release` и `store` требуют подтверждения владельца).

## Команды проверки

Инструменты: Rust (версия закреплена в `rust-toolchain.toml`), [`just`](https://github.com/casey/just), Python 3 для `reuse` и `pre-commit`. Остальное — по области (см. `AGENTS.md` области).

| Что | Команда |
|---|---|
| Список рецептов всех областей | `just --list --list-submodules` |
| Rust: формат | `cargo fmt --all --check` |
| Rust: линтер | `cargo clippy --workspace --all-targets -- -D warnings` |
| Rust: тесты | `cargo nextest run --workspace` (нет nextest — `cargo test --workspace`) |
| Лицензии и SPDX | `reuse lint` |
| Секреты и SPDX в изменённых файлах | `pre-commit run --all-files` |

На общей машине ограничивайте параллелизм: `nice cargo … -j4`.

## Обязательно

- Каждый новый исходник начинается с SPDX-заголовка в синтаксисе языка: `// SPDX-License-Identifier: MPL-2.0` (`#` для shell, Python, YAML, TOML; `<!-- -->` для XML и Markdown). Файлы без комментариев и сгенерированный код — запись в [REUSE.toml](REUSE.toml).
- Тесты из `done when` есть и проходят: Rust — `cargo nextest`; Flutter — unit, widget и golden; Android — JUnit и instrumented; Apple — XCTest; Go — `go test`.
- Документация — на русском (пользовательская — на русском и английском); код, идентификаторы, коммиты и заголовки PR — на английском.
- Новые зависимости — только с совместимой лицензией: MIT, Apache-2.0, BSD, ISC, MPL-2.0, LGPL (только динамическая линковка).

## Запрещено (ROADMAP §0.3 п.10)

- GPL-код в клиентах. FFmpeg и mpv — только LGPL-сборки, динамически.
- Ассеты, тексты и бренд Infuse/Firecore, Fedora, Dolby/DTS; Apple SF вне Apple-платформ; логотипы сервисов вне их brand guidelines.
- Секреты в git: ключи, токены, пароли, сертификаты. Только `.env.local` (шаблон `.env.example`) и GitHub Secrets — см. [docs/dev/secrets.md](docs/dev/secrets.md).
- Личные данные: имена, email, абсолютные пути домашних каталогов (`/home/<user>`, `C:\Users\<user>`) — в коде, логах, фикстурах и документах.
- Блокировка рекламы Twitch; телеметрия без opt-in; облачные сервисы проекта (ADR-0004).

## Если что-то непонятно

Не угадывайте решения, которые относятся к владельцу (DEC-…, `(owner)`): опишите вопрос в PR в разделе «Решения владельца» и продолжайте с безопасным вариантом по умолчанию.
