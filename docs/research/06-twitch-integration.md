# 06. Модуль Twitch: API, воспроизведение, чат, загрузки, риски и UX

> Исследование для project_alpha. Дата: 2026-11-13. Автор: исследователь №6 (интеграция Twitch).
> Связанные документы: [02 — клиентский стек](02-client-stack-playback.md) (Rust-ядро, Flutter + SwiftUI для tvOS, mpv/ExoPlayer/FFmpeg), [04 — бэкенд](04-backend-architecture-billing.md) (§1.8 sync позиции и ключи `twitch:vod:<id>`, §1.10 очередь `pa-jobs`, §5.1.1 OAuth-токены Twitch). Этот документ их не повторяет, а дополняет специфику Twitch.
> Пометка **[ПРОВЕРИТЬ]** означает факт, который нужно подтвердить на прототипе или у юриста: недокументированные части Twitch меняются без предупреждения.

---

## 0. TL;DR

1. **Официальный Helix API покрывает «каталог»**: пользователи и аватары, фолловы (`user:read:follows`), стримы, VOD/хайлайты/аплоады, клипы, категории, поиск, расписание, EventSub, чат (EventSub + Send Chat Message). **Он не выдаёт ссылки на воспроизведение (HLS)** и **не отдаёт главы VOD** (смену категорий по времени). Для чата VOD (replay) официального API тоже нет.
2. **Воспроизведение и главы есть только через недокументированный GQL** (`gql.twitch.tv`) с first-party Client-ID веб-плеера Twitch, `PlaybackAccessToken` и плейлисты `usher.ttvnw.net`. Так работают streamlink, yt-dlp, TwitchDownloader, Xtra, Twire и Frosty. Twitch официально называет GQL «не для третьих лиц», периодически его закручивает (client-integrity с 2022–2023 гг.) и может сломать в любой момент.
3. **Юридически это серая и красная зоны.** ToS Twitch даёт лицензию только на личный просмотр через сервис и запрещает копирование и скачивание контента. Developer Agreement запрещает хранить копии Twitch Content без разрешения. Apple Guideline 5.2.3 прямо запрещает приложениям скачивать медиа со сторонних сервисов без их разрешения. Xtra был удалён из Google Play в 2021 г. (по сообщениям, по DMCA-жалобе Twitch).
4. **Рекомендуемая стратегия — «два уровня сборок».**
   - **Store-сборки** (App Store, Play, tvOS): каталог через Helix, просмотр live/VOD нативным HLS, **рекламу показываем как есть**, чат, синхронизация позиции, загрузка клипов только через официальный endpoint. Скачивания и записи **нет**, но можно смотреть то, что уже скачал собственный сервер пользователя (это его файлы).
   - **Сервер и десктоп** (Linux/Win/mac вне сторов): скачивание VOD, запись эфиров, чат-replay. Включается пользователем явно, с предупреждением и только для личного использования, за серверным feature-flag с kill-switch.
5. **Главы и категории можно получать без GQL** для каналов, которые сервер мониторит: EventSub `channel.update` и `stream.online/offline` дают таймлайн смены категорий. Для произвольных VOD остаётся GQL `moments` (best-effort).
6. **Чат.** Без логина — анонимный IRC WebSocket (`justinfan…`), только чтение. С логином — EventSub `channel.chat.message` и `POST /helix/chat/messages`. Эмоуты: Twitch, 7TV, BTTV, FFZ. Чат скачанного VOD хранится рядом в JSON (формат совместим с TwitchDownloader) и синхронизируется с таймкодом.
7. **Конкурентное преимущество** — не в обходе ограничений Twitch, а в качестве: TV-first интерфейс, мгновенный resume между устройствами, фильтры и сортировка по категориям и главам, единая библиотека «скачанное + онлайн», нормальная работа PiP и фона, нет TikTok-ленты.

---

## 1. Официальный Twitch API (Helix)

### 1.1. Что даёт Helix: эндпоинты для нашего модуля

| Задача | Endpoint | Auth/scope | Примечания |
|---|---|---|---|
| Профиль, аватар, ник | `GET /helix/users` | app или user token | До 100 `id`/`login` за вызов; поля `display_name`, `login`, `profile_image_url`, `offline_image_url`, `broadcaster_type`, `created_at` |
| Фолловы пользователя | `GET /helix/channels/followed` | user token, **`user:read:follows`** | Пагинация, поле `followed_at` |
| Кто из фолловов сейчас в эфире | `GET /helix/streams/followed` | user token, `user:read:follows` | Главный источник для ряда «Сейчас в эфире» |
| Стримы (по логину, категории, языку) | `GET /helix/streams` | любой токен | `game_id`, `game_name`, `title`, `viewer_count`, `started_at`, `thumbnail_url` (шаблон `{width}x{height}`), `tags`, `is_mature` |
| Информация о канале | `GET /helix/channels` | любой | Текущая категория, теги, `content_classification_labels` |
| VOD/хайлайты/аплоады | `GET /helix/videos` | любой | Ровно один из `id` / `user_id` / `game_id`; `type=archive\|highlight\|upload`; `sort`, `period`; `first` ≤ 100; поля `duration` (строка `1h2m3s`), `stream_id`, `muted_segments` (только с user token), `thumbnail_url`, `view_count`, `published_at`. **Категорий и глав нет** ([docs](https://dev.twitch.tv/docs/api/reference/videos)) |
| Клипы | `GET /helix/clips` | любой | `video_id`, `vod_offset` (может быть `null` несколько минут), `game_id`, `duration`, `is_featured` |
| Скачивание клипов (официально) | `GET /helix/clips/downloads` | `editor:manage:clips` / `channel:manage:clips` | Работает **только если пользователь редактор канала**. Временные URL, 100 req/min ([docs](https://dev.twitch.tv/docs/api/reference/clips)). Для зрителя не подходит |
| Категории | `GET /helix/games`, `GET /helix/games/top`, `GET /helix/search/categories` | любой | `box_art_url` (шаблон), `igdb_id` (можно связать с IGDB для обложек и жанров) |
| Поиск каналов | `GET /helix/search/channels?live_only=` | любой | `is_live`, `game_name`, `started_at`, `thumbnail_url` (аватар) |
| Расписание | `GET /helix/schedule` | любой | Ряд «Скоро в эфире» на странице стримера |
| Эмоуты и бейджи | `GET /helix/chat/emotes[/global\|/set]`, `GET /helix/chat/badges[/global]` | любой | CDN `static-cdn.jtvnw.net/emoticons/v2/{id}/{format}/{theme}/{scale}` |
| Чат: отправка | `POST /helix/chat/messages` | `user:write:chat` | Отдельный bucket лимитов ([docs](https://dev.twitch.tv/docs/chat/)) |
| Цвет ника и настройки чата | `GET /helix/chat/color`, `GET /helix/chat/settings` | любой | Slow/sub/emote-only режимы показываем в UI |

**Чего в Helix нет:** URL воспроизведения (HLS) для live и VOD, главы VOD (moments), чат-replay VOD (старый v5 `comments` мёртв, yt-dlp в 2026.07 удалил «dead rechat subtitles» — см. [changelog](https://www.videohelp.com/software/yt-dlp/version-history)), sub-only статус VOD явным полем, скачивание VOD. Об отсутствии глав в API разработчики жалуются давно ([форум](https://discuss.dev.twitch.com/t/api-is-missing-information-about-video-chapters/21090)).

### 1.2. EventSub (уведомления)

- Транспорты: **webhook** (нужен публичный HTTPS, app access token, подписи HMAC), **WebSocket** (user token, подходит для клиента или сервера за NAT), Conduits (шардирование для больших объёмов).
- Нужные типы: `stream.online` и `stream.offline` (старт записи, пуш «X в эфире»), `channel.update` v2 (смена названия и категории, отсюда **наши собственные главы**), `channel.chat.message` и `channel.chat.notification` (чат), `channel.raid` (переход на рейд).
- **Важное ограничение WebSocket:** на пару (client_id, user) не больше 3 соединений и 300 подписок на соединение, а **`max_total_cost` = 10** ([docs](https://dev.twitch.tv/docs/eventsub/handling-websocket-events/)). Подписки на чужие каналы, не авторизовавшие наше приложение (`stream.online` для фолловов), стоят по 1, поэтому через WebSocket можно следить максимум за ~10 каналами. **Вывод:** уведомления о фолловах делаем через **webhook в нашем облаке** (app token, лимит стоимости на порядки выше, подписка на broadcaster дедуплицируется между всеми нашими пользователями) и через поллинг `GET /streams/followed` раз в 60–120 с на устройстве или сервере как запасной путь. Это согласуется с §1.10 документа 04.
- Подписки `channel.chat.message` с user token пользователя-читателя стоят 0 (авторизовал сам пользователь) **[ПРОВЕРИТЬ на прототипе]**.

### 1.3. OAuth: какие потоки на каких устройствах

Источник: [Getting OAuth Access Tokens](https://dev.twitch.tv/docs/authentication/getting-tokens-oauth/).

| Платформа | Поток | Комментарий |
|---|---|---|
| **Apple TV, Android TV, сервер/админка** | **Device Code Grant** (`POST https://id.twitch.tv/oauth2/device`, затем опрос `POST /oauth2/token` с `grant_type=urn:ietf:params:oauth:grant-type:device_code`, пользователь вводит код на `twitch.tv/activate`) | Для public-клиента секрет не нужен. На экране показываем QR с `verification_uri` и `user_code`. Пока пользователь не подтвердил, сервер отвечает `authorization_pending` |
| iOS, Android, desktop | **Device Code Grant** (то же самое, но кнопка «Открыть в браузере» вместо QR) или Authorization Code через **наше облако как OAuth-брокер** (confidential client, секрет только в облаке) | PKCE Twitch не документирует. Implicit grant устарел и небезопасен (токен во фрагменте URL, без refresh). Рекомендация: **один поток — DCF — везде**, UI-обёртка различается |
| Без логина | **App access token** (client credentials) — **только в нашем облаке** | Нужен client secret, поэтому на клиент его не кладём. Анонимные запросы (поиск, стримы, VOD) клиент делает через наш прокси **twitch-gateway** с кэшем |

Детали токенов: access token живёт около 4 ч. Refresh token у public-клиента одноразовый и **истекает после 30 дней неактивности**: после этого нужен повторный DCF, приложение должно мягко попросить войти снова. Минимальный набор scope: `user:read:follows`, `user:read:chat`, `user:write:chat` (только если пользователь включил отправку сообщений). Остальное по §5.1.1 документа 04.

> Нюанс: OAuth-токен, выданный **нашему** client_id, **не работает с GQL** (там нужен токен first-party клиента Twitch). Поэтому вход через Helix не даёт доступа к sub-only VOD и не убирает рекламу для Turbo/подписчиков при GQL-воспроизведении. См. §2.4.

### 1.4. Лимиты

- Token bucket на client_id: отдельные «вёдра» для app token и для каждой пары (client_id, user). По умолчанию запрос стоит 1 очко. Текущий лимит смотрим в заголовках `Ratelimit-Limit`, `Ratelimit-Remaining`, `Ratelimit-Reset`; на практике около 800 очков в минуту, **[ПРОВЕРИТЬ]** по заголовкам. При исчерпании возвращается 429 ([API guide](https://dev.twitch.tv/docs/api/guide/)).
- Пагинация курсорная (`after`/`before`/`first`), списки «плавающие»: возможны дубли, поэтому дедуплицируем по `id`.
- Чат: 20 сообщений за 30 с у обычного аккаунта, не чаще 1 сообщения в секунду на канал; до 100 одновременно открытых комнат на аккаунт ([docs](https://dev.twitch.tv/docs/chat/)).
- **Архитектурный вывод:** все анонимные Helix-запросы идут через облачный `twitch-gateway` с кэшем: пользователи и аватары на 24 ч, категории на 7 дней, `streams` на 30–60 с, `videos` на 5 мин. Коалесцируем одинаковые запросы, батчим `users?id=` по 100. Запросы с user token (фолловы, чат) клиент делает напрямую, лимит у каждого пользователя свой.

### 1.5. Developer Agreement и брендинг

- [Twitch Developer Services Agreement](https://legal.twitch.com/legal/developer-agreement) (последняя найденная редакция от ноября 2023 г.; страница рендерится JS, текст **[ПРОВЕРИТЬ юристом]**):
  - Twitch и авторы сохраняют все права на Twitch Content.
  - **Не хранить копии Twitch Content и Program Materials**, кроме случаев с письменным разрешением Twitch, собственных прав или кэширования на ограниченный срок. По словам сотрудника Twitch на форуме, хранить **данные пользователя** дольше можно, если пользователь дал согласие через OAuth ([форум](https://discuss.dev.twitch.com/t/storage-of-data-from-twitch-api/63884)). Это про данные, а не про видео.
  - Запрещённые действия нельзя выполнять и через третьих лиц.
- [ToS Twitch](https://legal.twitch.com/legal/terms-of-service/): у Twitch **исключительное право продавать и показывать рекламу** на сервисе ([пересказ](https://conductatlas.com/platform/twitch/twitch-terms-of-service/provision/CA-P-025003/twitch-exclusive-right-to-sell-advertisements/)). Лицензия пользователя — на личный некоммерческий доступ через сервисы Twitch. Копирование, скачивание, запись потока и архивирование контента без явного разрешения запрещены (точную формулировку **[ПРОВЕРИТЬ]**).
- **Брендинг** ([brand.twitch.tv](https://brand.twitch.tv)): нельзя выдавать себя за официальное приложение, нельзя использовать логотип Glitch как иконку приложения, нельзя использовать «Twitch» как часть названия продукта. Название-дескриптор вроде «… for Twitch» распространено (Frosty for Twitch в App Store), но рискованно. Рекомендация: модуль внутри project_alpha называется «Стримы» или «Live», упоминание вида «работает с Twitch» — только в описании. Аватары и превью — с CDN Twitch, атрибуция «Twitch» на экранах канала, кнопка «Открыть на Twitch».
- Важный факт для позиционирования: retention VOD на Twitch — **7 дней** у обычных стримеров, **14** у Affiliate, **60** у Partner, Turbo и Prime. Хайлайты и аплоады с мая 2025 г. ограничены **100 часами** на канал ([TechCrunch](https://techcrunch.com/2025/02/20/twitch-caps-streamers-storage-at-100-hours-of-highlights-and-uploads), [ArchiveTeam](https://wiki.archiveteam.org/index.php/Twitch)). Поэтому «посмотреть через неделю» без локальной копии часто невозможно, и в этом главный пользовательский аргумент за серверную запись.

---

## 2. Воспроизведение, главы и чат-replay: как это делают open-source клиенты

### 2.1. Цепочка воспроизведения (неофициальная)

Данные из исходников [yt-dlp `twitch.py`](https://github.com/yt-dlp/yt-dlp/blob/master/yt_dlp/extractor/twitch.py) и [документации streamlink](https://streamlink.github.io/cli/plugins/twitch.html):

1. `POST https://gql.twitch.tv/gql` с заголовком `Client-ID: <публичный ID веб-клиента Twitch>`, операция **`PlaybackAccessToken`**: `streamPlaybackAccessToken(channelName)` для live или `videoPlaybackAccessToken(id)` для VOD. В ответе `value` (JSON с правами) и `signature`.
2. Мастер-плейлист:
   - live: `https://usher.ttvnw.net/api/channel/hls/{login}.m3u8?sig=…&token=…&allow_source=true&supported_codecs=av1,h265,h264&playlist_include_framerate=true&p=<rand>`
   - VOD: `https://usher.ttvnw.net/vod/{video_id}.m3u8?...`
3. Варианты качества (`chunked` = source, 1080p60, 720p60… и `audio_only`) → медиаплейлист → сегменты (TS или fMP4; у Twitch есть HEVC и AV1 Enhanced Broadcasting, поэтому в `supported_codecs` передаём то, что реально декодирует устройство).
4. Live low-latency: Twitch добавляет prefetch-сегменты (`#EXT-X-TWITCH-PREFETCH`). streamlink в режиме `--twitch-low-latency` скачивает их заранее и держит live edge ≤ 2 сегмента ([docs](https://streamlink.github.io/cli/plugins/twitch.html)).
5. **Client-integrity:** с 2022 г. веб-плеер Twitch шлёт токен целостности, вычисляемый обфусцированным JS. В мае 2023 г. Twitch на неделю сделал его обязательным, и streamlink сломался; streamlink теперь умеет получать токен через headless Chromium ([docs](https://streamlink.github.io/cli/plugins/twitch.html)). Сейчас анонимный `PlaybackAccessToken` обычно работает без него, но это **решение Twitch, которое может измениться за день**. Для чат-replay через GQL TwitchDownloader уже сообщал о «закручивании» ([release 1.52.7](https://newreleases.io/project/github/lay295/TwitchDownloader/release/1.52.7)).
6. Другие GQL-операции, которыми пользуется yt-dlp: `VideoMetadata`, `VideoPlayer_ChapterSelectButtonVideo` (главы), `VideoPlayer_VODSeekbarPreviewVideo` (storyboard-превью для перемотки — полезно для scrubbing на TV), `FilterableVideoTower_Videos`, `ClipsCards__User`, `ShareClipRenderStatus` (URL файла клипа и токен).

**Как это делают клиенты:**

| Проект | Платформа | Плеер и данные | Статус |
|---|---|---|---|
| [streamlink](https://streamlink.github.io/cli/plugins/twitch.html) | CLI | GQL → usher; **фильтрует встроенную рекламу** (пауза вывода) | Активен |
| [yt-dlp](https://github.com/yt-dlp/yt-dlp) | CLI | GQL persisted queries, главы из `moments`; sub-only VOD через cookie `auth-token` | Активен |
| [TwitchDownloader](https://github.com/lay295/TwitchDownloader) | Win GUI + кроссплатформенный CLI (MIT) | VOD/клипы, чат в JSON/HTML/TXT, рендер чата с 7TV/BTTV/FFZ, FFmpeg | Активен, периодически ломается из-за GQL |
| Xtra ([F-Droid](https://f-droid.org/ru/packages/com.github.andreyasadchy.xtra/index.html)) | Android | GQL + Helix, загрузки VOD | **Удалён из Google Play в апреле 2021 г.**, живёт на GitHub и F-Droid |
| Twire | Android | Плеер и чат | В Play присутствует (по найденным данным) |
| [Frosty](https://github.com/tommyxchow/frosty) | iOS/Android (Flutter) | Helix + чат с 7TV/BTTV/FFZ; **видео через официальный web-плеер (WebView)**, загрузок нет | **В App Store** ([листинг](https://apps.apple.com/app/id1603987585)) |
| Chatterino | Desktop | Только чат (IRC + Helix), видео через streamlink | Активен |

**Ключевой вывод.** В сторах выживает модель Frosty: Helix для данных, официальный embed-плеер для видео, никаких загрузок. Модель Xtra с GQL-плеером и загрузками из Google Play выкинули.

### 2.2. Официальная альтернатива плеера: Twitch Embed

[Twitch Embed](https://dev.twitch.tv/docs/embed/) (`player.twitch.tv` и `embed.twitch.tv`) — санкционированный способ показать стрим или VOD в стороннем приложении: реклама, sub-gate, Turbo-статус залогиненного в WebView пользователя работают «как у Twitch». Минусы для нас:
- **На tvOS нет WebView** (WKWebView недоступен), поэтому на Apple TV embed невозможен в принципе.
- Нет нативного HDR, PiP через наш плеер, кастомного UI, общего плеера с библиотекой, точного контроля позиции (у embed JS API есть `seek` и `getCurrentTime`, этого хватает для синхронизации, но не для нашего UX).
- Нужен параметр `parent`: в WebView это требует загрузки с нашего HTTPS-домена-обёртки.

**Вывод:** embed — **безопасный fallback-режим** (feature flag `twitch.player=embed`) для iOS, Android и desktop на случай, если Twitch закроет GQL-плеер или ревьюер стора потребует. Основной режим — нативный HLS с показом рекламы как есть. На tvOS других вариантов нет, поэтому tvOS — самая рискованная платформа модуля.

### 2.3. Главы и несколько категорий у VOD

Три источника по убыванию «официальности»:

1. **Свой таймлайн (официально, Helix и EventSub).** Для каналов в избранном или фолловах сервер или облако подписываются на `channel.update`, `stream.online` и `stream.offline` и пишут `CategorySegment(stream_id, game_id, title, started_at)`. После эфира `GET /videos?user_id=…&type=archive` находит VOD по `stream_id`, и сегменты переводятся в смещения VOD (`offset = segment.started_at − stream.started_at`). Точность ±несколько секунд, без GQL. **Рекомендуемый путь для всего, что мы записываем или мониторим.**
2. **GQL `VideoPlayer_ChapterSelectButtonVideo` → `moments`** (`type=GAME_CHANGE`, `positionMilliseconds`, `durationMilliseconds`, `details.game{id,displayName,boxArtURL}`). Работает для произвольных VOD. Если моментов нет, берём одну главу с текущей игрой (так делает yt-dlp). Best-effort, кэшируем в БД навсегда.
3. **Fallback:** `GET /helix/videos` + текущая категория канала (`GET /helix/channels`). Только для свежих VOD и с пометкой «≈».

Хранить сразу в нормализованной модели (см. §5), чтобы фильтр «VOD, где был Elden Ring» работал по главам, а не по текущей категории канала.

### 2.4. Sub-only VOD, реклама, Turbo

- **Sub-only VOD:** usher отвечает 403 (`vod_manifest_restricted` / `unauthorized_entitlements`). Получить доступ можно только с токеном first-party веб-клиента, то есть cookie `auth-token` из браузера. Такой токен «даёт полный доступ к аккаунту» и не отзывается выходом из аккаунта ([streamlink](https://streamlink.github.io/cli/plugins/twitch.html)). **Решение:** не просим и не принимаем этот cookie в store-сборках. В desktop и серверной сборке — максимум как «экспертная настройка» с явным предупреждением, хранение в Keychain или Secret Service, по умолчанию выключено. Без него sub-only VOD показываем с замком и кнопкой «Смотреть на Twitch» (deeplink в официальное приложение или браузер).
- **Реклама:** Twitch вставляет рекламу на стороне сервера (SSAI) прямо в HLS (маркеры `EXT-X-DATERANGE` с классом `twitch-stitched-ad`, рекламные сегменты). **Мы НЕ строим блокировку рекламы:** не фильтруем сегменты, не подменяем `platform`/`player_type` ради отсутствия рекламы, не используем прокси-обходы (как TTV-LOL/Luminous и подобные). Плеер воспроизводит рекламу как обычный контент и показывает плашку «Реклама» с таймером (по `DATERANGE`), перемотку во время рекламы блокируем. Мы показываем то, что отдал Twitch, и не продаём собственную рекламу поверх (исключительное право Twitch). При записи live рекламные вставки остаются в записи; в метаданных помечаем их интервалы, чтобы обрезка не производилась автоматически. **[ПРОВЕРИТЬ юристом]**, допустима ли ручная пользовательская обрезка.
- **Turbo и подписчики без рекламы:** через Helix-токен это не работает (см. §1.3). Честно пишем в FAQ: «Для просмотра без рекламы по Turbo/подписке используйте режим встроенного плеера Twitch (embed) с входом в Twitch». Embed с web-сессией это корректно учитывает.

### 2.5. Чат-replay VOD

- Источник — GQL-операция `VideoCommentsByOffsetOrCursor` (смещение в секундах или курсор). Cursor-режим, по сообщениям, чаще упирается в integrity-проверку, offset-режим стабильнее **[ПРОВЕРИТЬ]**. Официальной альтернативы нет.
- Для **наших записей live** чат пишется в реальном времени из EventSub или IRC: это то же, что видит любой зритель, и не требует GQL. Это ещё один аргумент записывать чат вместе с эфиром на сервере.
- Формат хранения — JSON, совместимый со схемой TwitchDownloader (`streamer`, `video{start,end,chapters}`, `comments[]{content_offset_seconds, commenter{display_name, _id}, message{fragments[{text, emoticon}], user_badges, user_color}}`, `embeddedData` с эмоутами и бейджами): можно импортировать архивы пользователя, скачанные TwitchDownloader.

### 2.6. Оценка правовых и платформенных рисков

| Функция | Источник данных | Риск ToS Twitch | Риск App Store | Риск Google Play | Решение |
|---|---|---|---|---|---|
| Каталог, фолловы, поиск, аватары, категории | Helix | Низкий (официально) | Низкий | Низкий | Везде |
| Live/VOD через Twitch Embed | Официальный embed | Низкий | Низкий | Низкий | Fallback везде, кроме tvOS |
| Live/VOD нативным HLS через GQL, **с рекламой** | GQL + usher (недокументировано) | **Средний** (неразрешённый клиент, client-id Twitch) | Средний (прецедентов массовых отказов не нашёл; ревью смотрит в основном на загрузки и IP) | Средний | Основной режим, kill-switch на embed |
| Главы из `moments` | GQL | Средний | Низкий | Низкий | Best-effort, кэш |
| Чат live (IRC/EventSub) | Официально | Низкий | Низкий | Низкий | Везде |
| Чат-replay VOD онлайн | GQL | Средний | Низкий | Низкий | Feature flag |
| **Скачивание VOD и клипов (зритель)** | GQL + usher + FFmpeg | **Высокий** (прямой запрет копирования, хранения и архивирования) | **Высокий**: [Guideline 5.2.3](https://developer.apple.com/app-store/review/guidelines/#intellectual-property) запрещает сохранение и скачивание медиа со сторонних сервисов без разрешения | **Высокий**: [политика IP](https://support.google.com/googleplay/android-developer/answer/9888072); прецедент Xtra | **Только desktop и сервер** вне сторов |
| **Запись live 24/7** | usher HLS | **Высокий** | — | — | Только сервер, opt-in, личное использование |
| Sub-only VOD через cookie | GQL с first-party токеном | **Высокий** + риск безопасности аккаунта | Высокий | Высокий | Не делать в store; на desktop — скрытая экспертная опция или не делать вовсе |
| Блокировка рекламы | — | Нарушение | Нарушение | Нарушение | **Не делаем никогда** |

Дополнительные риски:
- **Технический:** GQL может перестать работать в любой день (persisted-query хэши, client-integrity, 410 Gone для `/api`). Нужны: серверный remote-config с актуальными хэшами и client-id (обновляется без релиза), kill-switch по функциям, автоматический ночной «канареечный» тест в CI (live, VOD, главы, чат-replay), плюс fallback на embed.
- **Аккаунтный:** использование first-party client-id Twitch может кончиться блокировкой нашего Helix client_id и Developer-аккаунта. Изоляция: GQL-запросы делает **только клиент или сервер пользователя напрямую**, а не наше облако. Облако работает только с Helix и EventSub под нашим client_id и не проксирует видео и GQL. Так наш облачный сервис остаётся «чистым» Helix-приложением.
- **Юрисдикция:** «копия для личного использования» — исключение, которое в разных странах трактуется по-разному и **не отменяет договорный запрет ToS**. Перед публикацией функций загрузки нужна консультация юриста. Самая безопасная формулировка продукта: «серверная медиатека умеет добавлять видео по URL с помощью yt-dlp/streamlink», то есть универсальный загрузчик (как это делают Tube Archivist, Pinchflat, Ganymede), а не «скачиватель Twitch».

### 2.7. Предлагаемый дизайн с минимизацией рисков

```
┌────────────── Store-сборки (iOS, iPadOS, tvOS, Android/TV из Play, Mac App Store) ──────────────┐
│ Helix: каталог, фолловы, поиск, категории, расписание, EventSub (через облако)                   │
│ Видео: нативный HLS (GQL→usher) с рекламой, или embed (flag); на tvOS — только нативный          │
│ Чат live: IRC/EventSub + отправка; эмоуты 7TV/BTTV/FFZ                                           │
│ Скачанное: только просмотр файлов с СВОЕГО сервера пользователя (это обычные файлы медиатеки)    │
│ Кнопки «Скачать» НЕТ; есть «Добавить в очередь сервера» — [ПРОВЕРИТЬ] с ревью, по умолчанию скрыто │
└──────────────────────────────────────────────────────────────────────────────────────────────────┘
┌────────────── Desktop (GitHub/Flathub/AUR/winget) и серверная сборка (NAS-дистрибутив) ─────────┐
│ Всё выше + модуль «Архив» (opt-in, предупреждение о ToS и личном использовании):                 │
│  • скачивание VOD/хайлайтов/клипов, запись live избранных каналов, чат-архив                     │
│  • движок: встроенный Rust-загрузчик HLS + опционально внешний yt-dlp/streamlink (пользователь    │
│    ставит сам, мы вызываем как плагин → проще обновлять при поломках GQL)                        │
│ Feature flags (remote-config): twitch.gql_playback, twitch.chapters_gql, twitch.chat_replay,      │
│  twitch.archive; kill-switch по стране/версии                                                    │
└──────────────────────────────────────────────────────────────────────────────────────────────────┘
```

Принципы:
1. **Всё инициирует пользователь**: никакого автоматического «скачать всё с канала» по умолчанию. Правила записи создаются вручную и по одному каналу.
2. **Личное использование**: нет публичного шаринга скачанных Twitch-файлов (гостевые ссылки, публичные библиотеки). Доступ только для профилей внутри аккаунта.
3. **Уважение к удалённому и muted-контенту**: muted-сегменты из Helix показываем в UI. Опция (по умолчанию включена) «удалить локальную копию, если VOD удалён автором» — **[ОБСУДИТЬ]**: аргумент за — снижает юридический риск, против — противоречит смыслу архива.
4. **Store-сборка — «чистый Helix-клиент + плеер»**. Модуль «Архив» физически не компилируется в store-бинарь (cargo feature `twitch-archive`), чтобы ревьюер не нашёл скрытый код загрузки.
5. **Мониторинг**: ночной CI-канареечный тест, телеметрия ошибок по типам (`gql_hash_invalid`, `integrity_required`, `usher_403`) и автоматическое переключение flag на embed при массовых сбоях.

---

## 3. Live-чат

### 3.1. Транспорт: IRC vs EventSub

| | Twitch IRC over WebSocket (`wss://irc-ws.chat.twitch.tv:443`) | EventSub `channel.chat.message` + `POST /helix/chat/messages` |
|---|---|---|
| Без логина | **Да**: анонимный ник `justinfan<random>`, только чтение | Нет |
| Статус у Twitch | «Исторический интерфейс», не помечен как deprecated. Незащищённый WS отключён 15.08.2025 ([lifecycle](https://dev.twitch.tv/docs/product-lifecycle/)) | **Рекомендуемый** способ ([docs](https://dev.twitch.tv/docs/chat/)) |
| Данные | Теги IRCv3 (`badges`, `emotes`, `color`, `reply-parent-*`, `msg-id`), USERNOTICE для сабов и рейдов, CLEARCHAT и CLEARMSG | Структурированный JSON: fragments (text/emote/cheermote/mention), badges, reply, `channel.chat.notification`, `channel.chat.clear_user_messages`, `channel.chat.message_delete` |
| Ограничения | 100 комнат на аккаунт, лимиты join и auth | Подписки на WS (см. §1.2), нужен `user:read:chat` |

**Решение:** абстракция `ChatSource` в Rust-ядре с двумя реализациями. Без логина и для «многоканального» режима — анонимный IRC. С логином — EventSub для чтения (+ `channel.chat.notification` для сабов, рейдов, анонсов) и Helix Send Chat Message для отправки. Если EventSub недоступен, временный fallback на IRC с `chat:read`/`chat:edit` (legacy scope). Нормализация в общую модель `ChatMessage{id, ts, user{id, login, display, color, badges[]}, fragments[], reply_to, kind}`. Та же модель используется для replay.

### 3.2. Эмоуты и бейджи

- **Twitch:** глобальные, канальные и наборы сабов пользователя (`/chat/emotes/user` с `user:read:emotes` — опционально, чтобы показать «мои эмоуты» в пикере). Анимированные — формат `animated` (GIF) на CDN.
- **Сторонние:** 7TV (`7tv.io/v3/users/twitch/{id}` + глобальные; есть WebSocket EventAPI для live-обновлений набора), BTTV (`api.betterttv.net/3/cached/users/twitch/{id}`, `/3/cached/emotes/global`), FFZ (`api.frankerfacez.com/v1/room/id/{id}`, глобальные; также `api.betterttv.net/3/cached/frankerfacez/users/twitch/{id}`). Включены по умолчанию (официальное мобильное приложение их не показывает, и это главная фича Frosty). Учитываем zero-width (overlay) эмоуты 7TV. **[ПРОВЕРИТЬ]** актуальные версии API 7TV (идёт переход на v4/GQL).
- **Бейджи:** `/chat/badges/global` и `/chat/badges?broadcaster_id=` + сторонние (FFZ, 7TV-косметика опционально).
- **Кэш:** emote-map на канал в SQLite (TTL 1 ч) + дисковый кэш изображений (WebP/AVIF/GIF). Декодирование анимаций — в ядре или общей текстуре: одна анимация на эмоут, а не на каждое сообщение.

### 3.3. Модерация и базовые функции

- Отображать: удалённые сообщения (`<сообщение удалено>`), таймауты и баны (CLEARCHAT), slow/sub/emote/followers-only режим (плашка над полем ввода), закреплённое сообщение, анонсы.
- Пользовательские функции: блок-лист пользователей (Helix `user:manage:blocked_users`, или локально без логина), фильтр слов и подсветка упоминаний, «скрыть бейджи / эмоуты / ботов» (Nightbot, StreamElements и т. п. — список ботов локальный), реплаи, @-упоминания с автодополнением.
- **Модераторские действия** (бан, таймаут, удаление через Helix `moderator:manage:*`) — **не в MVP**, это аудитория Chatterino.
- Производительность: на крупных каналах 50–100+ сообщений в секунду. Нужен кольцевой буфер (500–1000 сообщений), батчинг рендера раз в 100–200 мс, режим «пауза при прокрутке вверх», на TV — «прореживание» (показывать не больше N в секунду с индикатором «+N»).

### 3.4. Рендеринг чата по форм-факторам

- **Desktop:** чат справа (ресайз, ширина 300–420 px) или «театр» с полупрозрачным чатом поверх видео. Отдельное окно чата (pop-out) удобно для второго монитора.
- **Телефон:** портрет — видео 16:9 сверху, чат снизу (как везде). Ландшафт — видео на весь экран, чат выезжает справа по свайпу (30–40% ширины) с регулируемой прозрачностью.
- **Планшет:** split, как на desktop.
- **TV (Apple TV/Android TV):** чат только для чтения. Колонка справа 25–30% ширины, видео ужимается (не перекрывается) — режим «Театр», переключение кнопкой Play/Pause-долгим или кнопкой меню плеера. Крупный шрифт (≥ 28 pt на 1080p), ограничение частоты. Отправка сообщений с TV — через «телефон как пульт»: на телефоне в том же аккаунте приходит карточка «Вы смотрите X на Apple TV — написать в чат», это наш sync-hub из документа 04.
- **Replay для VOD и скачанных файлов:** индекс `content_offset_seconds` → бинарный поиск по позиции плеера. При перемотке перерисовываем окно последних N сообщений до текущей позиции. При ускоренном воспроизведении чат идёт с той же скоростью. Опция «задержка чата ±N секунд» для точной подстройки (у записи live смещение между видео и чатом ≈ задержка доставки HLS, её калибруем при записи по `program-date-time`).

---

## 4. Пайплайн загрузки и записи

Оркестрация (очередь, квоты, retention, атомарные перемещения) описана в §1.10 документа 04. Здесь — специфика Twitch. Всё в этом разделе входит только в desktop и серверные сборки (§2.7).

### 4.1. Скачивание VOD (сегментированный HLS)

1. **Резолв:** Helix `videos?id=` (метаданные) + GQL `PlaybackAccessToken` → usher → мастер-плейлист → выбор варианта: `source` (`chunked`) по умолчанию, иначе предпочтение пользователя (1080p60 / 720p / audio_only) с учётом кодека (H.264 надёжнее для старых TV; AV1/HEVC — если все устройства пользователя декодируют). Токен истекает (около 20 ч по `expires` в `value`), для длинных загрузок перезапрашиваем.
2. **Загрузка сегментов:** параллельно 4–8 запросов, ретраи с backoff. Каждый сегмент пишется в `tmp/<job>/seg_<n>.ts` и отмечается в манифесте job (bitmap готовых сегментов), поэтому **докачка** после перезапуска стартует с недостающих. Проверяем размер и `Content-Length`. Muted-сегменты у Twitch называются `*-muted.ts`: качаем как есть (звук вырезан самим Twitch) и помечаем интервалы.
3. **Диапазон (trim):** пользователь может скачать часть VOD (например, «только главу Elden Ring») — берём сегменты, покрывающие интервал, и точно режем при ремуксе.
4. **Ремукс без перекодирования:** склейка через concat demuxer FFmpeg (или прямое чтение плейлиста как локального m3u8) → **MKV по умолчанию** (главы, вложения, устойчивость к обрывам, теги) или MP4 (`-c copy -movflags +faststart`, `-bsf:a aac_adtstoasc`) для совместимости с AVPlayer. На Apple наш плеер читает MKV через FFmpeg-демуксер (документ 02), поэтому MKV — безопасный дефолт.
5. **Метаданные внутрь файла** (`ffmetadata`):
   - Глобальные теги: `title`, `artist`/`ARTIST`=display_name стримера, `date`=дата начала эфира, `comment`=URL VOD, кастомные `TWITCH_VIDEO_ID`, `TWITCH_STREAM_ID`, `TWITCH_USER_ID`, `TWITCH_LOGIN`, `TWITCH_CATEGORIES`.
   - **Главы** из `CategorySegment`: `[CHAPTER] TIMEBASE=1/1000 START= END= title=Elden Ring`.
   - Вложения MKV: `cover.jpg` (превью VOD), `avatar.jpg`; в MP4 — `attached_pic`.
   - Это позволяет пересканировать библиотеку без БД (если базу потеряли) и импортировать файлы, скачанные TwitchDownloader или yt-dlp: их `video_id` извлекается из имени и тегов.
6. **Сайдкары:** `*.chat.json` (§2.5, можно gzip), `*.info.json` (полный снимок метаданных: Helix + главы + muted), `*-thumb.jpg`, `*.trickplay/` (спрайты превью для перемотки генерируем сами FFmpeg-ом по I-кадрам — тот же механизм, что для фильмов библиотеки).
7. **Проверка:** `ffprobe` длительности против `duration` из Helix (допуск 1–2 %), затем атомарный `rename` в библиотеку и событие `library.item_added`.

### 4.2. Запись live

- Триггер: `stream.online` (облачный webhook → сервер пользователя по WS или iroh) или поллинг раз в 60 с (сервер без облака).
- Захват: тот же загрузчик, режим live. Опрос медиаплейлиста каждые ~`TARGETDURATION/2`, дедупликация по `EXT-X-MEDIA-SEQUENCE`, запись **сегментами в файлы по 1–2 часа** (или один MKV с периодическим fsync), чтобы обрыв не портил всё. Учитываем разрывы (`EXT-X-DISCONTINUITY`: смена разрешения, реклама) — при ремуксе такие куски разделяются или нормализуются.
- Реклама в записи остаётся (§2.4); интервалы помечаются в `info.json`.
- Параллельно пишется чат (EventSub/IRC) с `received_at` и `program-date-time` текущего сегмента для точной синхронизации.
- Сохраняются `channel.update` (смены категорий и названий) → главы.
- После `stream.offline`: финализация (склейка частей, главы, метаданные) и опциональная задача «через 30 мин сравнить с официальным VOD»: если VOD доступен и полон, можно **заменить** запись (без потерь при наших сетевых сбоях) или оставить запись, если VOD muted или удалён. Связь `stream_id ↔ video_id` сохраняется для ключей sync (документ 04, §1.8).
- **Нагрузка:** 1080p60 source ≈ 6–8 Мбит/с ≈ 3–3,5 ГБ/ч. 5 каналов по 6 ч в день ≈ 100 ГБ/день. Квоты и retention по каналу обязательны (§1.10 документа 04). Предлагаем дефолт «хранить 14 дней или последние 10 эфиров» и выбор качества записи (720p60 ≈ 1,5–2 ГБ/ч).
- **Клиент vs сервер:** запись live только на сервере или десктопе (мобильные ОС убивают фон). Скачивание VOD — на сервере (рекомендуется) или desktop-клиенте. На телефоне — офлайн-копия **из своего сервера** (обычная функция медиатеки), а не с Twitch.

### 4.3. Раскладка хранилища и именование

```
<storage:twitch>/
  <login> [<user_id>]/                         # папка по стримеру; user_id в имени — переживает смену ника
    _channel/avatar.jpg, banner.jpg, channel.json
    2026/
      2026-11-12 Elden Ring, Hades II — Title here [v2290012345].mkv
      2026-11-12 Elden Ring, Hades II — Title here [v2290012345].chat.json.gz
      2026-11-12 ... [v2290012345].info.json / -thumb.jpg
      2026-11-13 LIVE Just Chatting — Title [s51234567890].mkv   # наша запись (s = stream_id)
    clips/
      2026-11-12 Clip title [c-SlugName].mp4
    highlights/
```

- Шаблон имени настраиваемый (переменные `{date}`, `{login}`, `{categories}`, `{title}`, `{id}`), с санацией для Windows и SMB (запрещённые символы, длина ≤ 180, без конечных точек и пробелов).
- **Источник истины — БД, а не имя файла.** Переименование и перемещение в UI меняют путь в БД и на диске атомарно. Ручной перенос файла вне приложения подхватывается сканером по тегу `TWITCH_VIDEO_ID` или по `[v…]` в имени.
- **Дедупликация:** уникальный ключ `(video_id)` для VOD, `(stream_id)` для записей, `(clip_slug)` для клипов. Перед постановкой в очередь проверяем, нет ли уже файла или активной задачи (с любого устройства). Запись live + официальный VOD одного эфира связываются через `stream_id` и показываются как одна карточка с двумя «версиями» (как версии фильма 4K/1080p).
- Управление файлами в UI: переименовать, переместить между storage-targets, удалить (в корзину сервера на 7 дней), «оставить навсегда» (исключение из retention), пересобрать в MP4, перекачать в другом качестве, обрезать.

---

## 5. Модель данных и интеграция с библиотекой

### 5.1. Сущности

```
Streamer          (user_id PK, login, display_name, avatar_url, avatar_local, offline_banner_url,
                   broadcaster_type, description, last_seen_live_at, updated_at)
Category          (game_id PK, name, box_art_url, igdb_id)
Favourite         (profile_id, user_id, source: twitch_follow|local, notify: bool,
                   record_rule_id?, added_at)                     -- фолловы Twitch + локальное избранное
LiveSession       (stream_id PK, user_id, started_at, ended_at?, title_last, peak_viewers,
                   video_id?)                                     -- эфир; связь с VOD
Video             (video_id PK, user_id, stream_id?, type: archive|highlight|upload,
                   title, published_at, created_at, duration_s, view_count, thumbnail_url,
                   muted_segments_json, is_sub_only?, availability: online|deleted|unknown,
                   last_checked_at)
CategorySegment   (id, owner: video_id|stream_id, game_id, title_at_time?, start_s, end_s,
                   source: eventsub|gql_moments|fallback)        -- главы
Clip              (slug PK, broadcaster_id, creator_name, video_id?, vod_offset_s?, game_id,
                   title, created_at, duration_s, thumbnail_url, view_count)
MediaFile         (общая сущность медиатеки из док. 04: id, storage_target, path, size, container,
                   codecs, duration, fingerprint)
TwitchAsset       (media_file_id FK, kind: vod|recording|clip, video_id?, stream_id?, clip_slug?,
                   quality, has_ads_marked, chat_log_id?, origin: server|desktop|import)
ChatLog           (id, owner: video_id|stream_id|clip_slug, path, format: tdl_json_v1,
                   message_count, first_offset_s, last_offset_s, emote_snapshot: bool)
DownloadTask      (= jobs из док. 04 §1.10; params: video_id|login, quality, range, with_chat,
                   container; result: media_file_id)
RecordRule        (id, profile_id, user_id, quality, with_chat, retention_days?, keep_last_n?,
                   categories_filter?, replace_with_vod: bool, enabled)
PlaybackProgress  (= progress из док. 04 §1.8: (account, profile, media_key) → pos, watched, hlc)
```

### 5.2. Ключи sync и связка «онлайн ↔ файл»

- `media_key` для VOD = `twitch:vod:<video_id>` **и для онлайн-просмотра, и для скачанного файла** (как зафиксировано в документе 04). Для записи — `twitch:rec:<stream_id>` + маппинг `offset_rec_to_vod` после появления VOD. Тогда позиция переносится в обе стороны: начал запись на ТВ, продолжил официальный VOD на телефоне в дороге.
- Клипы: `twitch:clip:<slug>`, прогресс не нужен (короткие), только «просмотрено».
- Live: без позиции. Синхронизируется только «сейчас смотрю канал X» → кнопка «Продолжить на этом устройстве» и «догнать эфир» (на другом устройстве открывается live на live edge).
- **Выбор источника при открытии VOD:** локальный файл на устройстве → файл на своём сервере в LAN → файл на сервере удалённо (iroh) → онлайн с Twitch (если VOD ещё жив). Пользователь видит одну карточку с пометкой «Есть на сервере» и не думает об источниках.
- Избранное и фолловы синхронизируются как данные (`Favourite`), токены — нет (документ 04, §5.1.1).

### 5.3. Встраивание в общую медиатеку

- Twitch — отдельная **секция библиотеки** («Стримы»), такая же, как «Фильмы», «Сериалы», «Музыка». Внутри стример ≈ «сериал», VOD ≈ «эпизод», сортировка по дате. Это переиспользует готовые UI-компоненты (полки, карточки, страница сериала, continue watching).
- Ряд «Продолжить просмотр» на главной общий для всего: VOD Twitch идут вперемешку с сериалами.
- Поиск по библиотеке индексирует названия VOD, стримеров и категории (FTS5 в SQLite).
- Фильтры и сортировка: стример, категория (по `CategorySegment`, а не по одной текущей), дата, длительность, «есть локально / только онлайн / истекает скоро» (дата удаления на Twitch считается по типу канала: 7/14/60 дней), «не досмотрено», язык.

---

## 6. UX по форм-факторам

### 6.1. Боли официального приложения (что исправляем)

Собрано из отзывов и прессы:
- **Редизайн 2024 г. с TikTok-лентой (Discovery Feed)** на главной: пользователи массово просили откат, Twitch раскатал его всё равно ([techissuestoday](https://techissuestoday.com/twitch-mobile-apps-ui-redesign-sparks-user-outrage-prompting-calls-for-rollback), [Sportskeeda](https://sportskeeda.com/us/streamers/news-remove-tiktok-aspects-users-flood-twitch-forums-requests-roll-back-tiktok-like-features-mobile-app), [Spilled](https://spilled.gg/twitchs-tiktok-makeover-sparks-user-revolt)). Фолловы ушли с первого экрана.
- **VOD сбрасывается на начало**, если погас экран или приложение ушло в фон — давний баг ([Clarity](https://www.onclarity.com/leaderboard/insight/twitch)). Позиция VOD не синхронизируется надёжно между устройствами.
- **Приложение для Apple TV:** жалобы на обновление, скрывающее небольшие каналы, и на слабую навигацию ([PiunikaWeb](https://piunikaweb.com/blog/latest-twitch-app-update-on-apple-tv-allegedly-awful)). На TV нет нормального чата рядом с видео, медленный старт, нет фильтров VOD.
- Нет эмоутов 7TV, BTTV и FFZ в мобильных и TV-приложениях ([Frosty README](https://github.com/tommyxchow/frosty)).
- Невозможно отфильтровать VOD канала по игре или главе, сортировать по длительности; нет «что я не досмотрел».
- VOD исчезают через 7/14/60 дней, офлайн-просмотра нет вообще.
- Жалобы стримеров, что мобильный сайт и TV «не работают нормально» ([Sportskeeda — Squeex](https://sportskeeda.com/us/streamers/news-squeex-blasts-twitch-ceo-collaborating-streamers-accusing-website-working-mobile-tv)); слабый PiP и фон на Android; навязчивые промо, бусты и вертикальные клипы.

### 6.2. Главная модуля «Стримы» (все устройства, разная плотность)

Ряды сверху вниз:
1. **Сейчас в эфире** (фолловы + локальное избранное): большая карточка с живым превью (обновляется раз в 60 с), аватар, ник, категория (иконка box art), время в эфире, зрители. Порядок: «любимые» (закреплённые) → по зрителям или по времени начала (настройка).
2. **Продолжить просмотр** (VOD/записи с прогрессом) — общий с медиатекой.
3. **Новые VOD от избранных** (с бейджем «Удалится через 2 дня» и «На сервере»).
4. **Скачано и записано** (с сервера пользователя), группировка по стримеру.
5. **Скоро в эфире** (расписание фолловов).
6. **Категории ваших стримеров** (быстрые фильтры: «Just Chatting», «Elden Ring»…).
7. (Опционально) **Популярное сейчас** по выбранным языкам — без автоплея и без вертикальной ленты.

Без логина ряды 1–6 строятся по локальному избранному (поиск → «★ В избранное»). Предлагаем импорт фолловов разово через DCF, после чего можно выйти: избранное останется локальным.

### 6.3. Страница стримера

Шапка: баннер, аватар, ник, статус (LIVE/офлайн, последний эфир N ч назад), кнопки «Смотреть», «★», «Уведомлять», «Записывать» (desktop и сервер), «Открыть на Twitch». Вкладки: **VOD** (сетка с полосой глав-категорий на каждой карточке — цветная лента с иконками игр по таймлайну; фильтр по категории, длительности, «не смотрел»), **Хайлайты**, **Клипы** (сортировка по просмотрам и дате, период), **Скачанное**, **Расписание**, **О канале**.

### 6.4. Плеер

- **Таймлайн с главами-категориями** (сегменты с box art при наведении или фокусе), переход «следующая глава» отдельной кнопкой. Storyboard-превью при перемотке (GQL seekbar preview или наш trickplay для файлов).
- **Мгновенный resume** (документ 04, §1.8.1): тост «Продолжено с 1:41:11 (iPhone, 10 мин назад)».
- **Скорость 0,5–3×** для VOD и пропуск ±10/30/60 с (на TV — клики по тачпаду и D-pad).
- **Low-latency режим** для live (prefetch-сегменты, малый буфер; переключатель «Низкая задержка / Стабильно»), индикатор отставания и кнопка «К эфиру». Для live доступна DVR-перемотка назад в пределах окна плейлиста. «Смотреть с начала эфира» = открыть VOD текущего стрима (Twitch создаёт его во время эфира), с автопереходом на live при догоне.
- **PiP:** iOS (`AVPictureInPictureController` с `AVSampleBufferDisplayLayer`), Android (`enterPictureInPictureMode`, автоматический PiP при уходе с экрана), desktop (окно «поверх всех», мини-плеер). **Фоновое аудио** («только звук» = вариант `audio_only` для экономии трафика) — особенно ценно для подкаст-подобных стримов, интегрируется с нашим музыкальным мини-плеером и системными media controls.
- **Театр с чатом** (§3.4); multi-view (2–4 стрима) на desktop и TV — v2.
- Реклама: плашка «Реклама · 0:15», без перемотки; по окончании — автоматический возврат.
- Рейды: при `channel.raid` предлагаем «Перейти к X» (тост 10 с), не перебрасываем автоматически.

### 6.5. TV (Apple TV / Android TV) — фокус-навигация

- Верхние вкладки: Главная · Стримы · Категории · Поиск · Скачанное. Горизонтальные ряды, крупные фокусируемые карточки с параллаксом (tvOS `.card` button style), живое превью при фокусе > 1 с (muted).
- Длинное нажатие на карточке открывает контекстное меню: «В избранное», «Записывать», «Отметить просмотренным», «Открыть канал».
- **Логин:** QR-код + `user_code` (DCF). Плюс «Войти с телефона»: TV присылает код на телефон того же аккаунта project_alpha через sync-hub, и телефон открывает `twitch.tv/activate` с уже подставленным кодом. Сам токен Twitch между устройствами не передаётся (документ 04, §5.1.1).
- Поиск — диктовка Siri или Google Assistant + экранная клавиатура с подсказками каналов и категорий.
- Свайп вниз в плеере открывает инфо-панель: главы-категории, качество, задержка, чат вкл/выкл, «Показать на телефоне».
- Чат — только чтение. «Ответить» отправляет карточку на телефон.

### 6.6. Телефон

- Нижние вкладки: Главная · Подписки · Обзор · Загрузки (на телефоне — офлайн-копии с сервера) · Профиль.
- Свайп вниз по плееру сворачивает его в мини-плеер (как YouTube) с продолжением звука; PiP автоматически.
- Жесты: двойной тап ±10 с (VOD), вертикальный свайп — яркость и громкость, свайп влево — чат в ландшафте.
- Уведомления «X в эфире» (через облачный EventSub webhook → APNs/FCM), группировка, тихие часы, выбор каналов.
- Виджеты (iOS и Android): «Сейчас в эфире» из избранного.

### 6.7. Desktop

- Боковая панель с избранным (аватары с красной точкой LIVE, как в web Twitch), основное окно — сетка.
- Горячие клавиши (Space, J/K/L, F, T — театр, C — чат, `[` `]` — главы), pop-out чата и плеера, multi-view.
- Менеджер загрузок и записей: очередь, скорость, место, правила записи, retention; drag & drop файлов между storage-targets.
- Импорт существующих архивов (папки с файлами yt-dlp, streamlink, TwitchDownloader) с автоматическим сопоставлением по `video_id` и подтягиванием метаданных и глав из Helix и GQL.

### 6.8. «Почему мы лучше официального приложения» — короткий список

1. Фолловы и избранное на первом экране, без ленты рекомендаций.
2. Resume VOD между любыми устройствами до секунды, нет сброса при блокировке экрана.
3. Главы-категории на таймлайне, фильтр VOD по игре, сортировка по длительности и дате.
4. Единая библиотека: онлайн-VOD, свои записи и скачанное в одной карточке, офлайн (desktop и сервер).
5. Чат с 7TV/BTTV/FFZ на всех устройствах, чат рядом с видео на TV, replay чата у записей.
6. Нормальные PiP, фон, «только звук», мини-плеер.
7. Работает без аккаунта (локальное избранное).
8. Единый стиль с медиаплеером (tvOS-эстетика), быстрые Rust-ядро и нативный плеер.

---

## 7. План внедрения (предложение)

| Этап | Содержимое | Платформы |
|---|---|---|
| **T0 — прототип (2–3 нед.)** | Rust-crate `pa-twitch`: Helix-клиент + DCF + кэш; GQL playback → mpv на desktop; анонимный IRC-чат; проверка всех [ПРОВЕРИТЬ] | Linux desktop |
| **T1 — MVP «смотреть»** | Главная, страница стримера, поиск, категории, live/VOD нативно с рекламой, чат + эмоуты, sync позиции, локальное избранное, облачный `twitch-gateway` + EventSub-webhook для уведомлений; remote-config и kill-switch; embed-fallback | Desktop, Android, Android TV, iOS, tvOS |
| **T2 — главы** | Свой таймлайн категорий (EventSub `channel.update`) + GQL moments; фильтры по категориям | Все |
| **T3 — Архив** | Скачивание VOD и клипов, запись live, чат-архив, метаданные и главы в MKV, хранилище, retention, импорт архивов | Сервер + desktop (вне сторов) |
| **T4 — полировка** | Multi-view, «телефон как пульт» для чата, виджеты, PiP-доработки, модераторские функции (опц.) | Все |

Перед T1 в сторах: юридическая консультация по ToS Twitch (воспроизведение через GQL) и пред-ревью Apple (TestFlight External Review) с минимальной сборкой.

---

## 8. Источники

- Twitch API Reference: [Videos](https://dev.twitch.tv/docs/api/reference/videos), [Clips](https://dev.twitch.tv/docs/api/reference/clips), [индекс](https://dev.twitch.tv/docs/api/reference/), [API Guide (лимиты, пагинация)](https://dev.twitch.tv/docs/api/guide/)
- [Twitch: Getting OAuth Access Tokens (DCF)](https://dev.twitch.tv/docs/authentication/getting-tokens-oauth/), [Refreshing tokens](https://dev.twitch.tv/docs/authentication/refresh-tokens/)
- [EventSub WebSocket: лимиты](https://dev.twitch.tv/docs/eventsub/handling-websocket-events/), [Twitch Chat docs](https://dev.twitch.tv/docs/chat/), [Product lifecycle (PubSub, IRC non-TLS)](https://dev.twitch.tv/docs/product-lifecycle/), [Changelog](https://dev.twitch.tv/docs/change-log/)
- [Twitch Embed](https://dev.twitch.tv/docs/embed/)
- [Twitch Developer Services Agreement](https://legal.twitch.com/legal/developer-agreement), [Twitch Terms of Service](https://legal.twitch.com/legal/terms-of-service/), [форум: хранение данных API](https://discuss.dev.twitch.com/t/storage-of-data-from-twitch-api/63884), [ConductAtlas: исключительное право на рекламу](https://conductatlas.com/platform/twitch/twitch-terms-of-service/provision/CA-P-025003/twitch-exclusive-right-to-sell-advertisements/), [Twitch brand](https://brand.twitch.tv)
- [Форум: GQL не для третьих лиц / 410 Gone](https://discuss.dev.twitch.tv/t/410-gone-when-requesting-private-api-api-namespace-with-3rd-party-client-id/23060), [Форум: главы отсутствуют в API](https://discuss.dev.twitch.com/t/api-is-missing-information-about-video-chapters/21090)
- [streamlink: Twitch plugin](https://streamlink.github.io/cli/plugins/twitch.html), [yt-dlp twitch.py](https://github.com/yt-dlp/yt-dlp/blob/master/yt_dlp/extractor/twitch.py), [yt-dlp version history](https://www.videohelp.com/software/yt-dlp/version-history)
- [TwitchDownloader](https://github.com/lay295/TwitchDownloader), [TwitchDownloader 1.52.7 (GQL crackdown)](https://newreleases.io/project/github/lay295/TwitchDownloader/release/1.52.7)
- Xtra: [GitHub-форк](https://github.com/FrostySo/Xtra), [F-Droid](https://f-droid.org/ru/packages/com.github.andreyasadchy.xtra/index.html); [Frosty](https://github.com/tommyxchow/frosty), [Frosty в App Store](https://apps.apple.com/app/id1603987585)
- [Apple App Review Guidelines 5.2.3](https://developer.apple.com/app-store/review/guidelines/#intellectual-property), [Google Play: Intellectual Property policy](https://support.google.com/googleplay/android-developer/answer/9888072)
- Retention и хранение: [TechCrunch — лимит 100 ч](https://techcrunch.com/2025/02/20/twitch-caps-streamers-storage-at-100-hours-of-highlights-and-uploads), [ArchiveTeam: Twitch](https://wiki.archiveteam.org/index.php/Twitch), [PC Gamer — отмена «save forever»](https://pcgamer.com/twitch-eliminates-save-forever-option-as-it-moves-to-improve-video-on-demand-service)
- Жалобы пользователей: [techissuestoday — редизайн](https://techissuestoday.com/twitch-mobile-apps-ui-redesign-sparks-user-outrage-prompting-calls-for-rollback), [techissuestoday — ответ Twitch](https://techissuestoday.com/twitch-responds-to-mobile-app-redesign-backlash), [Sportskeeda — откат TikTok-функций](https://sportskeeda.com/us/streamers/news-remove-tiktok-aspects-users-flood-twitch-forums-requests-roll-back-tiktok-like-features-mobile-app), [Spilled.gg](https://spilled.gg/twitchs-tiktok-makeover-sparks-user-revolt), [PiunikaWeb — Apple TV](https://piunikaweb.com/blog/latest-twitch-app-update-on-apple-tv-allegedly-awful), [Clarity — отзывы](https://www.onclarity.com/leaderboard/insight/twitch), [Sportskeeda — Squeex](https://sportskeeda.com/us/streamers/news-squeex-blasts-twitch-ceo-collaborating-streamers-accusing-website-working-mobile-tv)

---

## 9. Итог: 10 ключевых выводов и рисков

1. Helix официально даёт каталог, фолловы (`user:read:follows`), аватары, категории, VOD и клипы, EventSub и чат, но **не даёт HLS-ссылок, глав VOD и чат-replay**.
2. Воспроизведение возможно только через недокументированный GQL (`PlaybackAccessToken` → usher) с client-id Twitch. Это может сломаться в любой день, поэтому нужны remote-config, kill-switch, канареечный CI и embed-fallback (кроме tvOS, где WebView нет).
3. Логин на всех устройствах — Device Code Grant (на TV — QR). Анонимный режим обслуживает облачный `twitch-gateway` с app token и кэшем. Наш облачный сервис не трогает GQL и видео.
4. Уведомления о фолловах — только через EventSub **webhook** в облаке (лимит стоимости WebSocket = 10) плюс поллинг `streams/followed`.
5. Главы и категории: свой таймлайн из EventSub `channel.update` (официально, для мониторимых каналов) + GQL `moments` (best-effort) в модели `CategorySegment`.
6. Скачивание и запись прямо противоречат ToS и Developer Agreement Twitch, а также Apple 5.2.3 и политике Google Play (прецедент — Xtra). Поэтому **только в desktop и серверных сборках вне сторов**: opt-in, личное использование, без шаринга, код физически исключён из store-бинарей.
7. Рекламу не блокируем никогда: SSAI-рекламу показываем с плашкой и в записях не вырезаем. Sub-only VOD и Turbo без рекламы — только через embed или официальное приложение; cookie `auth-token` в сторах не принимаем.
8. Чат: анонимный IRC для чтения без логина, EventSub + Send Chat Message с логином; 7TV, BTTV и FFZ по умолчанию; чат записей и скачанных VOD хранится в JSON формата TwitchDownloader и синхронизируется по таймкоду.
9. Ключ `twitch:vod:<id>` общий для онлайн-VOD и файла (плюс маппинг `rec ↔ vod`), поэтому resume работает между устройствами и источниками. Twitch — секция общей медиатеки, VOD ведут себя как эпизоды.
10. Наше преимущество — UX, а не обход ограничений: фолловы первым экраном без TikTok-ленты, resume до секунды, фильтры по категориям и главам, архив, который переживает 7/14/60-дневный retention Twitch, полноценные TV, PiP и фон. До релиза нужны юрист и пред-ревью Apple.
