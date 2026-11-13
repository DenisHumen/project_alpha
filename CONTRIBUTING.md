<!-- SPDX-License-Identifier: MPL-2.0 -->

# Как участвовать / Contributing

Спасибо за интерес к project_alpha! Мы принимаем pull request'ы от всех. *English summary below.*

Участвуя в проекте, вы соглашаетесь соблюдать [Кодекс поведения](CODE_OF_CONDUCT.md). Об уязвимостях сообщайте приватно — см. [SECURITY.md](SECURITY.md).

## Перед началом

1. Посмотрите [docs/ROADMAP.md](docs/ROADMAP.md): у каждой задачи есть ID (например `CORE-012`), зависимости (`deps`) и критерии готовности (`done when`). Полные правила работы с задачами — в [§0.3 ROADMAP](docs/ROADMAP.md#03-правила-для-агентов-и-контрибьюторов), владение каталогами — в §0.4, правила правки общих («горячих») файлов — в §0.5.
2. Возьмите задачу, у которой все зависимости отмечены `[x]` и нет открытого PR с её ID:
   ```bash
   gh pr list --search "[CORE-012] in:title" --state open
   ```
   Задачи с пометкой `(owner)` делает только владелец. Для крупной работы сначала откройте issue или обсуждение, чтобы не делать двойную работу.
3. Архитектура и решения: [docs/STRATEGY.md](docs/STRATEGY.md), [docs/research/](docs/research/), ADR в [docs/adr/](docs/adr/).

Нет подходящей задачи, но есть идея или ошибка — откройте issue по шаблону (ошибка, проблема воспроизведения, неверное распознавание, предложение) или тему в [Discussions](https://github.com/DenisHumen/project_alpha/discussions).

## Процесс: одна задача — одна ветка — один worktree

```bash
git fetch origin
git worktree add ../pa-CORE-012 -b feat/CORE-012-hlc-clock origin/main
cd ../pa-CORE-012
# … работа, тесты …
git commit -s -m "feat(sync): add HLC clock"
git push -u origin feat/CORE-012-hlc-clock
gh pr create --draft --title "feat(sync): add HLC clock and LWW merge [CORE-012]"
```

- **Ветка:** `<type>/<ID>-<slug>`, где type ∈ `feat|fix|docs|ci|build|chore|test|refactor|perf`. Без ID — `fix/<slug>`.
- **Сразу откройте draft PR** — так задача считается занятой. Статус «в работе» в ROADMAP не ведётся.
- **Коммиты** — [Conventional Commits](https://www.conventionalcommits.org/): `feat(core): add SMB2 directory listing`, `fix(play-and): …`. Язык кода, идентификаторов, коммитов и заголовков PR — английский; документация — на русском (пользовательская — на русском и английском).
- **Заголовок PR:** `<type>(<scope>): <summary> [<ID>]`. Его проверяет CI (`dco.yml`): ID задачи обязателен для `feat`, `perf`, `refactor`, `test`, `ci`, `build`; для `fix`, `chore`, `docs`, `revert` и scope `deps` (Dependabot) — по желанию. Scope — имя workstream в нижнем регистре (`core`, `net`, `lib`, `meta`, `play`, `play-and`, `play-apl`, `play-dsk`, `ui`, `tv`, `sync`, `srv`, `mus`, `twi`, `dist`, `dsn`, `pkg`, `qa`, `docs`, `legal`, `site`, `comm`, `repo`, `ci`).
- **ROADMAP:** в том же PR отметьте чекбокс своей задачи (`- [ ]` → `- [x]`). Меняйте только строку своей задачи; новую работу добавляйте новой задачей в конец своего раздела.
- **Свои каталоги:** меняйте каталоги своего workstream (§0.4). Нужна правка в чужом — минимальная и с пометкой в PR, либо отдельная задача.
- **Небольшие PR:** ориентир — до ~400 строк рукописного кода. Перед мержем — rebase на `main`, зелёный CI. PR сливаются через squash merge после ревью владельца (CODEOWNERS).

## Developer Certificate of Origin (DCO)

Мы используем [DCO](https://developercertificate.org/) вместо CLA. Каждый коммит должен содержать строку

```
Signed-off-by: Ваше Имя <email@example.com>
```

Её добавляет `git commit -s`. Этим вы подтверждаете, что имеете право отправить код под лицензией проекта. Проверка DCO обязательна для мержа. Забыли подпись — `git rebase --signoff origin/main` и `git push --force-with-lease`.

## Лицензионные заголовки (REUSE)

В каждом новом исходнике первой строкой (после shebang) — SPDX-заголовок в синтаксисе языка:

```rust
// SPDX-License-Identifier: MPL-2.0
```

```sh
# SPDX-License-Identifier: MPL-2.0
```

Copyright по умолчанию — «project_alpha contributors» (задан в [REUSE.toml](REUSE.toml)). Файлы без комментариев (JSON, картинки, lock-файлы), сгенерированный и сторонний код описываются в `REUSE.toml`, тексты лицензий лежат в `LICENSES/`. Проверка: `reuse lint` (локально: `pipx install reuse` или `pip install --user reuse`).

## Секреты

Никаких ключей, токенов, паролей и личных путей в коммитах. Локальные значения — в `.env.local` (он в `.gitignore`), шаблон — [.env.example](.env.example). Включите локальную проверку gitleaks:

```bash
pipx install pre-commit   # или pip install --user pre-commit
pre-commit install
```

Подробнее, где какие секреты живут: [docs/dev/secrets.md](docs/dev/secrets.md).

## Требования к PR

- Код собирается, тесты и линтеры проходят (CI запускается автоматически; для первых PR от новых участников запуск подтверждает мейнтейнер).
- Новая логика покрыта тестами (стратегия, обязательное в PR, тестовые медиа и ручные чек-листы — [docs/dev/testing.md](docs/dev/testing.md)): Rust — `cargo nextest`, Flutter — unit, widget и golden, Android — JUnit и instrumented, Apple — XCTest, Go — `go test`. Если в `done when` указаны платформы, перечислите в PR, где проверено.
- Не добавляйте GPL-зависимости в клиенты (см. STRATEGY §5, ADR-0005). Новые зависимости — только с совместимой лицензией (MIT, Apache-2.0, BSD, ISC, MPL-2.0, LGPL с динамической линковкой). FFmpeg и mpv — только LGPL-сборки.
- Никаких секретов, ключей и личных путей в коммитах.
- Не используйте бренды, логотипы и ассеты Infuse/Firecore, Fedora, Twitch (кроме разрешённого брендбуком), Dolby/DTS, Apple SF вне Apple-платформ.
- Телеметрия — только opt-in. Блокировку рекламы Twitch не принимаем.

## Метки

Метки описаны кодом в [.github/labels.yml](.github/labels.yml) и синхронизируются автоматически: `ws:*` — workstream, `M0`…`M10` — веха, `size:*`, `flavor:store|full`, `good first issue`, `help wanted`. Новую метку добавляйте PR-ом в этот файл, а не через интерфейс GitHub.

## AI-агенты

Код от AI-агентов принимается на тех же условиях: человек, отправляющий PR, отвечает за него и подписывает DCO; агента можно указать трейлером `Co-Authored-By`. Инструкции для агентов — в [AGENTS.md](AGENTS.md) (общие) и [CLAUDE.md](CLAUDE.md) (правила владельца и заметки для Claude Code).

## Лицензия

Отправляя вклад, вы соглашаетесь, что он распространяется под [MPL-2.0](LICENSE).

---

### English summary

By participating you agree to the [Code of Conduct](CODE_OF_CONDUCT.md); report security issues privately as described in [SECURITY.md](SECURITY.md).

1. Pick a task from [docs/ROADMAP.md](docs/ROADMAP.md) whose `deps` are all ticked and that has no open PR with its ID (full rules: ROADMAP §0.3; directory ownership §0.4; hot files §0.5). `(owner)` tasks are reserved for the maintainer.
2. One task = one branch = one worktree: `git worktree add ../pa-<ID> -b feat/<ID>-<slug> origin/main`. Open a draft PR right away to claim the task.
3. Use [Conventional Commits](https://www.conventionalcommits.org/); PR title `<type>(<scope>): <summary> [<ID>]`. Code, identifiers and commits are in English; docs are in Russian (user-facing docs in Russian and English).
4. Sign off every commit (`git commit -s`, [DCO](https://developercertificate.org/)). No CLA.
5. Every new source file starts with `SPDX-License-Identifier: MPL-2.0`; `reuse lint` must pass.
6. No secrets: use `.env.local` (git-ignored) based on `.env.example`; enable the gitleaks hook with `pre-commit install`.
7. Keep PRs small, with tests (see [docs/dev/testing.md](docs/dev/testing.md)); tick your task's checkbox in the roadmap in the same PR. PRs are squash-merged after maintainer review.
8. No GPL dependencies in clients, no third-party trademarks or assets, telemetry only opt-in.

AI agents: see [AGENTS.md](AGENTS.md). Contributions are licensed under [MPL-2.0](LICENSE).
