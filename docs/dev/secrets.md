# Политика секретов

> Задача REPO-009. Связано: [SECURITY.md](../../SECURITY.md), [ADR-0004](../adr/0004-no-project-servers-local-sync.md) (своих серверов нет), [ADR-0005](../adr/0005-license-mpl2-dco.md), [условия провайдеров](../legal/providers.md), ROADMAP §0.3 п.10.

Репозиторий публичный. Любая строка, попавшая в git, считается опубликованной навсегда: удаление коммита не помогает, ключ нужно отзывать.

## Правила

1. **В git нет секретов.** Ни ключей, ни токенов, ни паролей, ни приватных сертификатов, ни личных путей и email. Исключение — публичные идентификаторы клиентов (Twitch client_id, Trakt client_id), которые по условиям провайдера не секретны.
2. **Локально** секреты лежат в `.env.local` (в `.gitignore`). Шаблон со всеми переменными — [`.env.example`](../../.env.example); добавили переменную в код — добавьте её туда с заглушкой `changeme`.
3. **В CI** секреты лежат только в GitHub Secrets **окружений** (`release`, `store`), а не в секретах репозитория. Окружения требуют подтверждения владельца (REPO-005), поэтому job из PR до них не доберётся.
4. **OIDC вместо долгоживущих ключей** везде, где провайдер умеет: GitHub Actions получает короткий токен на один запуск.
5. **Workflow для PR** запускаются только на `pull_request` (не `pull_request_target`), с `permissions: contents: read` и без секретов (CI-002). Секреты не печатаются в лог и не передаются в шаги со сторонним кодом без нужды.
6. **Ключи провайдеров в сборках** подставляются при сборке из Secrets. Их всё равно можно извлечь из бинаря, поэтому у каждого провайдера есть поле «свой ключ» (см. [providers.md](../legal/providers.md)). Ключи, которые по условиям нельзя раздавать (TheTVDB, MAL), в сборки не попадают вовсе — только ключ пользователя.

## Где какие секреты живут

| Секрет | Где хранится | Кто использует | Примечание |
|---|---|---|---|
| Личные ключи разработчика к TMDB, Fanart.tv, OpenSubtitles, AcoustID | `.env.local` разработчика | локальные сборки и тесты | имена переменных — в `.env.example` |
| Ключи проекта к TMDB, Fanart.tv, OpenSubtitles, AcoustID | Secrets окружений `release` и `store` | релизные сборки (PKG) | регистрирует владелец (LEGAL-002) |
| Twitch client_id, Trakt client_id, LRCLIB App-Id, AniDB client | можно в открытом виде (конфиг сборки) | клиенты | **не секреты**; Twitch client secret не заводим никогда |
| Android upload keystore и пароли | Secrets окружения `store` | сборка AAB для Google Play | Play App Signing: ключ подписи приложения хранит Google |
| Google Play publishing | OIDC → Workload Identity Federation (GCP) | загрузка в Play | без JSON-ключа сервисного аккаунта, если возможно |
| App Store Connect API key (`.p8`, key id, issuer id) | Secrets окружения `store` | TestFlight, App Store, нотаризация macOS | ключ с минимальной ролью |
| Сертификаты подписи Apple (`.p12` и пароль) | Secrets окружения `store` / `release` | подпись iOS, tvOS, macOS | |
| Подпись Windows (Azure Trusted Signing) | OIDC → federated credential в Azure | MSIX, установщики | без секретов в GitHub |
| Подпись образов NAS-ОС и релизных артефактов | cosign keyless (OIDC, Sigstore) | DIST, release | приватного ключа нет |
| Ключ minisign для `config/` (удалённые конфиги) | Secret отдельного окружения публикации конфигов | workflow публикации конфигов | публичный ключ встроен в клиенты (CORE-014) |
| `GITHUB_TOKEN` | выдаётся GitHub на запуск | все workflow | по умолчанию только чтение; права повышаются в конкретном job |

Окружения, Secrets и OIDC-доверие настраивает владелец (REPO-005 и задачи PKG/DIST). Агенты и контрибьюторы секреты не видят и не заводят.

## Проверки

- **Локально:** хук gitleaks из [`.pre-commit-config.yaml`](../../.pre-commit-config.yaml) проверяет индексированные изменения перед коммитом и блокирует коммит с ключом. Включается один раз на клон:
  ```bash
  pipx install pre-commit   # или pip install --user pre-commit
  pre-commit install
  ```
  Ручная проверка всего дерева: `pre-commit run --all-files`.
- **CI:** job `gitleaks` в [`hygiene.yml`](../../.github/workflows/hygiene.yml) сканирует все коммиты PR, а на `main` — всю историю. Конфигурация и allowlist — [`.gitleaks.toml`](../../.gitleaks.toml) (заглушки вида `changeme` разрешены).
- **GitHub:** secret scanning и push protection в настройках репозитория (Settings → Code security) — включает владелец; push protection отклоняет `git push` с ключом известного провайдера ещё до попадания в репозиторий.

Ложное срабатывание: замените значение на заглушку или, если это действительно не секрет (тестовый вектор, публичный ключ), добавьте узкое правило в allowlist `.gitleaks.toml` с комментарием в PR. Комментарий `gitleaks:allow` в строке тоже работает, но его увидит ревьюер.

## Если секрет утёк

1. **Сразу отзовите или смените ключ** у провайдера — это главное, история git вторична.
2. Сообщите владельцу приватно (Security → Report a vulnerability, см. [SECURITY.md](../../SECURITY.md)).
3. Замените ключ в Secrets окружения или в `.env.local`.
4. Переписывать историю `main` (`git filter-repo`) имеет смысл только вместе с отзывом ключа и только по решению владельца.
