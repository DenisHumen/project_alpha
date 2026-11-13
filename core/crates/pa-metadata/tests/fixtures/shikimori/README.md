<!-- SPDX-License-Identifier: MPL-2.0 -->
# Кассета Shikimori для тестов

`cassette.jsonl` — HTTP-кассета для `ReplayTransport` (формат — в rustdoc `pa_metadata::replay`): строка = `GET`-запрос (путь и query) и ответ (статус, заголовки, тело).

**Данные синтетические.** Структура повторяет REST API v1 Shikimori (`/animes`, `/animes/{id}`, `/animes/{id}/related`: `russian`, `license_name_ru`, `description` с BB-разметкой `[character=…]`, `myanimelist_id`, `rating`), но все тайтлы, описания и ID (`6000xx`) выдуманы. Официальных условий кэширования и редистрибуции данных у Shikimori нет ([docs/legal/providers.md](../../../../../../docs/legal/providers.md), Shikimori), поэтому настоящие ответы в git не кладём.

Сценарии: поиск с `censored=true`, пустое русское название, хентай (`rating: rx`, скрывается), `404`, запись с чужим `myanimelist_id`, связи с мангой (отбрасывается), `429` → `200`, постоянный `503`.

Сверка формы с живым API (локально, без коммита записи): `RecordingTransport` поверх `UreqTransport` с User-Agent зарегистрированного OAuth-приложения; лимит 5 запросов в секунду и 90 в минуту.
