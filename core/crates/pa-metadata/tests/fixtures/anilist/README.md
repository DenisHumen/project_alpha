<!-- SPDX-License-Identifier: MPL-2.0 -->
# Кассета AniList для тестов

`cassette.jsonl` — HTTP-кассета для `ReplayTransport` (формат — в rustdoc `pa_metadata::replay`). AniList — GraphQL поверх `POST`, поэтому запрос в кассете записан не путём и query, а операцией и переменными: `{"path":"/","graphql":{"operation":"Search","variables":{...}}}`. Текст GraphQL-запроса в ключ не входит, так что правка форматирования запроса кассету не ломает; новые поля в запросе — повод сверить форму ответа с живым API.

**Данные синтетические.** Структура повторяет ответы AniList (`data.Page.media`, `data.Media`, `relations.edges`, `errors[].status`, заголовки `X-RateLimit-*`), но все тайтлы, ID (`7000xx` AniList, `6000xx` MAL) и даты выдуманы. Настоящие ответы AniList в git не кладём: условия запрещают использовать API как хранилище данных и массово собирать данные ([docs/legal/providers.md](../../../../../../docs/legal/providers.md), AniList §6–7).

Сценарии: поиск с 18+ тайтлом (фильтруется), граф связей (сиквел, побочная история, манга, 18+ спин-офф), `Media` по `id` и по `idMal`, `404` с `errors`, лимит 90 в заголовке (адаптация), `X-RateLimit-Remaining: 0` с `X-RateLimit-Reset`, `429` с `Retry-After: 60` → `200`, постоянный `429`, `500` → `200`, `400` с ошибкой валидации, `200` с `errors` внутри.

Сверка формы с живым API (локально, без коммита записи): обернуть `UreqTransport` в `RecordingTransport` и прогнать те же запросы через `AniListClient`. Лимит AniList сейчас 30 запросов в минуту — не больше пары десятков запросов за раз.
