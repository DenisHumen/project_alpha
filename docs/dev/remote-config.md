# Удалённый конфиг: ключ minisign и подпись `config/*.json`

> Задача CORE-014. Связано: [ADR-0004](../adr/0004-no-project-servers-local-sync.md) (своих серверов нет), [ADR-0039](../adr/0039-remote-config-minisign.md) (формат и правила доверия), [secrets.md](secrets.md), исследование 06 §2.6–2.7, крейт [`pa-remote-config`](../../core/crates/pa-remote-config).

Приложение раз в несколько часов скачивает из этого репозитория `config/remote-config.json` и `config/remote-config.json.minisig`, проверяет подпись встроенным публичным ключом и только после этого читает флаги. Подписывает файл **только владелец, офлайн**. Секретный ключ никогда не попадает в git, CI, облако или на сервер сборки.

## Как это работает в приложении

| Шаг | Что происходит |
|---|---|
| Старт | Берётся встроенная копия (`core/crates/pa-remote-config/builtin/remote-config.json`), поверх — последняя валидная копия с диска, если её подпись снова проходит проверку и `serial` не меньше встроенного. Сеть не трогается. |
| Обновление | Раз в `refresh.interval_secs` (15 мин … 7 дней, по умолчанию 6 ч): `GET` с `If-None-Match`. `304` — ничего не меняется. `200` — скачивается `.minisig`, проверяется подпись, потом JSON разбирается. |
| Ошибка | Нет сети, неверная подпись, старый `serial`, новая `schema` — текущие флаги остаются, кэш на диске не трогается, повтор через `refresh.retry_secs`. |
| Зеркала | Сначала `raw.githubusercontent.com/…/main/config/`, затем GitHub Pages `…github.io/project_alpha/config/` (если включены). |
| Ключ-заглушка | Пока `PRODUCTION_PUBLIC_KEY` — заглушка, приложение работает **только на встроенной копии** (и в debug, и в release), кэш игнорирует, в сеть не ходит. |

Подпись должна быть «prehashed» (`ED`, по умолчанию в minisign ≥ 0.6), а её доверенный комментарий — содержать `file:<имя файла>` (minisign пишет его сам). Это не даёт выдать один подписанный файл за другой.

## Формат документа

```json
{
  "schema": 1,
  "serial": 2,
  "issued_at": "2026-12-01T00:00:00Z",
  "refresh": { "interval_secs": 21600, "retry_secs": 1800 },
  "flags": {
    "twitch.gql_playback": true,
    "twitch.chapters_gql": true,
    "twitch.chat_replay": false,
    "twitch.archive": false
  },
  "kill_switches": [
    { "feature": "twitch.gql_playback", "max_app_version": "0.3.1", "platforms": ["ios"], "reason": "integrity token" },
    { "feature": "twitch.*", "min_app_version": "0.4.0-alpha.1", "max_app_version": "0.4.0-alpha.3" }
  ],
  "twitch": {
    "gql_client_id": "<публичный client-id>",
    "persisted_queries": { "VideoPlayer_ChapterSelectButtonVideo": "<sha256>" },
    "player": "native"
  }
}
```

- `serial` — увеличивать при **каждой** публикации. Документ с меньшим `serial`, чем уже есть у приложения, отвергается: старый подписанный файл не может выключить kill-switch обратно.
- `flags` — нет флага — значит выключен. `kill_switches` выключают функцию по шаблону имени (`twitch.*`, `*`), диапазону версий приложения (включительно, semver) и платформам; если версию приложения не удаётся разобрать, ограниченный по версиям kill-switch срабатывает.
- `twitch.player`: при выключенном или убитом `twitch.gql_playback` приложение всегда берёт `embed`. Неизвестное значение тоже означает `embed`.
- Нативное воспроизведение (TWI-004, `pa_twitch::playback`) требует `twitch.gql_playback: true`, `gql_client_id`, `player: "native"` и хэш операции `PlaybackAccessToken` в `persisted_queries`; без любого из них ответ — embed, запросов к Twitch нет. Работает только в full-сборке (возможность `twitch-native` крейта `pa-flavor`, `cfg(pa_twitch_native)`, CORE-013 — см. [flavors.md](flavors.md)); store-сборка всегда берёт embed (ADR-0011 §5). Ошибки GQL/usher имеют коды `gql_hash_invalid`, `integrity_required`, `usher_403` — по ним видно, что пора обновить хэш или выключить флаг.
- Неизвестные поля игнорируются (поле `_comment` в примере — это допускает). Ломающее изменение формата — только новая `schema` вместе с релизом приложения.
- Встроенная копия в крейте — копия последнего подписанного документа на момент релиза. Перед релизом её обновляют, иначе приложение начнёт с более старых флагов (кэш и сеть всё равно победят, если `serial` больше).

## Ключ: создать один раз (владелец, офлайн)

Нужен [minisign](https://jedisct1.github.io/minisign/) (пакет `minisign` в Fedora/Debian/Homebrew, `scoop install minisign`). Лучше — на машине без сети или с live-USB.

```bash
# 1. Пара ключей, секретный — с паролем (не -W).
minisign -G -p kinolune-config.pub -s kinolune-config.key \
  -c "Kinolune remote config"

# 2. Секретный ключ и пароль — в офлайн-хранилище (менеджер паролей + копия
#    на зашифрованном носителе). В репозиторий, CI и облака не кладём.

# 3. Публичный ключ — вторая строка kinolune-config.pub:
sed -n 2p kinolune-config.pub
```

Эту строку (`RW…`, 56 символов) владелец вписывает в `PRODUCTION_PUBLIC_KEY` в [`core/crates/pa-remote-config/src/verify.rs`](../../core/crates/pa-remote-config/src/verify.rs) отдельным PR, и копирует файл в `config/minisign.pub` (для ручной проверки другими людьми). Тест `production_key_constant_is_placeholder_or_valid` проверит, что ключ разбирается; `just core remote-config-release-check` перестанет падать.

## Подписать и опубликовать

```bash
# правим config/remote-config.json, увеличиваем serial, затем:
KINOLUNE_CONFIG_KEY=/путь/к/kinolune-config.key just core sign-config
# = minisign -S -s "$KINOLUNE_CONFIG_KEY" -m <файл> для каждого config/**/*.json,
#   затем just core remote-config-verify (проверка как в приложении)

git add config/ && git commit -s -m "chore(config): <что поменяли>, serial N"
git push   # через PR или напрямую в main — решение владельца
```

Проверить чужие или свои подписи без секретного ключа:

```bash
just core remote-config-verify                      # встроенным ключом приложения
just core remote-config-verify pubkey=config/minisign.pub
minisign -Vm config/remote-config.json -p config/minisign.pub
```

Сырой файл на `raw.githubusercontent.com` обновляется с задержкой CDN (до ~5 мин), причём JSON и `.minisig` — независимо. В это окно приложение может увидеть новую пару «файл + старая подпись» и отвергнуть её; оно просто повторит позже. Ничего страшного, но срочные kill-switch'и лучше публиковать заранее проверенными.

## Если ключ скомпрометирован или потерян

1. Новый ключ (шаги выше), новый `PRODUCTION_PUBLIC_KEY`, релиз приложения.
2. Старые версии приложения продолжат верить старому ключу, пока не обновятся: держать их на безопасных значениях можно только тем же старым ключом. Поэтому секретный ключ хранится в двух офлайн-копиях.
3. Утечка ключа без релиза не лечится — отсюда правило «только офлайн».

## Релизы

`just core remote-config-release-check` падает, пока ключ — заглушка. Подключить его в релизный workflow (`app-v*`) — решение владельца: до появления ключа это заблокирует релизы, а без проверки релиз выйдет «только со встроенной копией» (безопасно, но без удалённых kill-switch'ей).
