<!-- SPDX-License-Identifier: MPL-2.0 -->

# Безопасность / Security Policy

*English version below.*

## Как сообщить об уязвимости

**Не публикуйте уязвимости в открытых issue, PR или Discussions.**

Сообщайте приватно через GitHub Private Vulnerability Reporting: вкладка **Security → Report a vulnerability** этого репозитория (прямая ссылка: <https://github.com/DenisHumen/project_alpha/security/advisories/new>). Отчёт видят только мейнтейнеры.

Что указать:

- компонент и версию (приложение и платформа, медиасервер, NAS-ОС, коммит);
- описание и влияние: что может сделать атакующий;
- шаги воспроизведения или PoC;
- известные обходные пути.

Не прикладывайте реальные пароли, токены и личные медиафайлы. Если нужен пример, обезличьте его.

## Сроки

| Этап | Срок |
|---|---|
| Подтверждение получения | до 7 дней |
| Первичная оценка (подтверждена ли уязвимость, серьёзность) | до 14 дней |
| Исправление критичных и высоких | цель — до 30 дней после подтверждения |
| Публичное раскрытие | после выпуска исправления, но не позже 90 дней с момента отчёта, если не договорились иначе |

Мы держим автора в курсе через тот же приватный advisory, согласуем дату раскрытия и по желанию указываем автора в advisory и changelog. Для подтверждённых уязвимостей запрашиваем CVE через GitHub Security Advisories.

## Что входит в область

- Код этого репозитория: Rust-ядро, клиенты (Flutter, Android, Apple, desktop), медиасервер, NAS-ОС (`distro/`), инструменты и workflow GitHub Actions.
- Сборки, опубликованные в GitHub Releases этого репозитория и в сторах.

Не входит: уязвимости сторонних библиотек (FFmpeg, mpv и др.) без специфики нашего использования — о них лучше сообщать upstream (мы обновим зависимость); сервисы третьих сторон (TMDB, Twitch и т. п.); атаки, требующие физического доступа к разблокированному устройству.

У проекта нет своих серверов и облака (ADR-0004), поэтому «серверных» уязвимостей проекта нет. Медиасервер и NAS-ОС работают у пользователя, и уязвимости в них входят в область.

## Поддерживаемые версии

До выхода 1.0 исправления выпускаются только для последней версии каждого компонента (`app`, `server`, `distro`). После 1.0 политика будет уточнена здесь.

## Секреты в репозитории

Если вы нашли в репозитории или в истории коммитов ключ, токен или пароль, сообщите тем же приватным способом. Политика секретов: [docs/dev/secrets.md](docs/dev/secrets.md).

---

## English

**Do not report security vulnerabilities in public issues, pull requests or discussions.**

Report privately via GitHub Private Vulnerability Reporting: **Security → Report a vulnerability** in this repository (<https://github.com/DenisHumen/project_alpha/security/advisories/new>). Please include the affected component and version, impact, reproduction steps or a PoC, and any known workaround. Do not attach real credentials or personal media.

Timelines: acknowledgement within 7 days; initial assessment within 14 days; fixes for critical and high severity issues targeted within 30 days of confirmation; coordinated public disclosure after a fix is released and no later than 90 days after the report unless agreed otherwise. We credit reporters on request and request CVEs through GitHub Security Advisories.

Scope: all code in this repository and builds published from it (GitHub Releases, app stores). Out of scope: third-party libraries without project-specific impact (please report upstream), third-party services, and attacks that require physical access to an unlocked device. The project runs no servers of its own.

Supported versions: until 1.0, only the latest release of each component receives fixes.
