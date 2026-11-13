# Как участвовать / Contributing

Спасибо за интерес к project_alpha! Мы принимаем pull request'ы от всех. *English summary below.*

## Перед началом

1. Посмотрите [docs/ROADMAP.md](docs/ROADMAP.md): у каждой задачи есть ID (например `CORE-012`), зависимости и критерии готовности.
2. Возьмите задачу, у которой закрыты все зависимости. Для крупной работы сначала откройте issue или обсуждение, чтобы не делать двойную работу.
3. Архитектура и решения: [docs/STRATEGY.md](docs/STRATEGY.md), [docs/research/](docs/research/), ADR в [docs/adr/](docs/adr/).

## Ветки и коммиты

- Ветка: `feat/<ID>-краткое-описание` или `fix/<ID>-...` (без ID — просто `fix/описание`).
- Коммиты — [Conventional Commits](https://www.conventionalcommits.org/): `feat(core): add SMB2 directory listing`, `fix(android): ...`. Язык коммитов и кода — английский.
- Заголовок PR: `<type>(<scope>): описание [<ID>]`. В том же PR отметьте задачу в `docs/ROADMAP.md` как выполненную.
- Небольшие PR: одна задача — один PR. PR сливаются через squash merge.

## Developer Certificate of Origin (DCO)

Мы используем [DCO](https://developercertificate.org/) вместо CLA. Каждый коммит должен содержать строку

```
Signed-off-by: Ваше Имя <email@example.com>
```

Её добавляет `git commit -s`. Этим вы подтверждаете, что имеете право отправить код под лицензией проекта.

## Требования к PR

- Код собирается, тесты и линтеры проходят (CI запускается автоматически; для первых PR от новых участников запуск подтверждает мейнтейнер).
- Новая логика покрыта тестами.
- Не добавляйте GPL-зависимости в код, который попадает в сборки для App Store (см. STRATEGY §5). Новые зависимости — только с совместимой лицензией (MIT, Apache-2.0, BSD, MPL-2.0, LGPL с динамической линковкой).
- Никаких секретов, ключей и личных путей в коммитах.
- Не используйте бренды, логотипы и ассеты Infuse/Firecore, Fedora, Twitch (кроме разрешённого брендбуком), Dolby/DTS.

## AI-агенты

Код от AI-агентов принимается на тех же условиях: человек, отправляющий PR, отвечает за него и подписывает DCO. Правила для агентов — в [CLAUDE.md](CLAUDE.md).

## Лицензия

Отправляя вклад, вы соглашаетесь, что он распространяется под [MPL-2.0](LICENSE).

---

### English summary

Pick a task from `docs/ROADMAP.md` whose dependencies are done, branch as `feat/<ID>-slug`, use Conventional Commits, sign off every commit (`git commit -s`, DCO), keep PRs small with tests, tick the task in the roadmap in the same PR. No GPL dependencies in App Store code paths, no secrets, no third-party trademarks. Contributions are licensed under MPL-2.0.
