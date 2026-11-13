# Название продукта и проверка товарных знаков (LEGAL-005)

> Дата проверки: 2026-11-13. Рабочее название — `project_alpha`. **Финальный выбор делает владелец (DEC-004).** Ничего не регистрировалось: ни домены, ни хэндлы, ни товарные знаки.
>
> **Это не юридическая консультация.** Документ — предварительный скрининг (knock-out search). Перед регистрацией домена и подачей заявки на товарный знак нужен поиск в официальных базах по классам МКТУ и, желательно, консультация патентного поверенного.

## 1. Требования к имени

- Без «Infuse», «Firecore», «Fedora», «Twitch»; без созвучия с Plex, Jellyfin, Emby, Kodi, Infuse, VLC, Fedora (ROADMAP LEGAL-005, DEC-004; CLAUDE.md; 05 §7).
- Выдуманное слово (fanciful/coined mark): у таких имён наибольшая различительная способность и меньше риск конфликта.
- Легко произносится и пишется по-английски и по-русски; 2–3 слога.
- Свободен хотя бы один из доменов `.com`, `.app`, `.org`; свободно имя организации на GitHub.
- Есть пара для NAS-ОС. Имя ОС строим как `<Имя> OS` (по аналогии с примером «Alpha OS» из 05 §7) и пишем в `os-release` `NAME="<Имя> OS"`, `ID_LIKE=fedora`. Слово «Fedora» в имени ОС запрещено. Формулировка «основано на Fedora Linux» допустима.

## 2. Как проверяли

| Проверка | Метод | Ограничения |
|---|---|---|
| Домены `.com`, `.app`, `.org`, `.io` | DNS-запрос к корню зоны: `NXDOMAIN` = делегирования нет, домен **вероятно свободен** | Не учитывает зарезервированные и премиум-домены и домены без NS. Перед покупкой проверить у регистратора |
| GitHub | `gh api search/repositories` (имя в названии репо) и `gh api users/<имя>` (свободно ли имя пользователя или организации) | Только точное имя |
| Существующие продукты, приложения в App Store и Google Play | Веб-поиск по точному написанию, в том числе вместе со словами «app», «app store», «google play» | Поиск — только US-индекс; региональные сторы могли не попасть |
| Товарные знаки (USPTO, EUIPO, WIPO) | Веб-поиск по агрегаторам баз знаков (Justia Trademarks, Trademarkia, uspto.report, TMview, WIPO Brand DB) | Официальные поисковики USPTO, EUIPO/TMview и WIPO Brand Database — JS-приложения, и автоматизированный доступ к ним из этой среды недоступен. **Владелец должен повторить поиск вручную** (§5) |
| Созвучие с известными брендами | Ручной анализ: Infuse, Plex, Jellyfin, Emby, Kodi, VLC, Fedora, Twitch, а также крупные бренды в медиа (Vue Cinemas, Loom, Vellum, TIDAL, Otter.ai, Lark) | Субъективно |

Проверяемые классы МКТУ (Ниццкая классификация) для будущей заявки: **9** (ПО, приложения), **38** (передача данных, стриминг), **41** (развлечения, предоставление медиаконтента), **42** (SaaS и разработка ПО).

## 3. Кандидаты

Уровни риска: **низкий** — точных совпадений не найдено, созвучие слабое; **средний** — есть похожие имена в смежных областях или занят `.com`; **высокий** — есть продукт с тем же или почти тем же именем в нашей или смежной нише.

| # | Имя приложения | NAS-ОС | Смысл | `.com` | `.app` | `.org` | GitHub (репо / org) | Найденные конфликты | Риск |
|---|---|---|---|---|---|---|---|---|---|
| 1 | **Kinolune** | Kinolune OS | «кино» + lune (луна): кино под луной; по-русски легко «Кинолун» | свободен | свободен | свободен | 0 / свободно | Точных совпадений нет. Приставка «kino» в нише многолюдна (Kinolime, Kinora, Kinon, Kinolights — приложения про кино), но сочетание различимо. Не использовать «LuneOS» отдельно: так называется порт webOS | **низкий** |
| 2 | **Ambreel** | Ambreel OS | amber (янтарь) + reel (катушка плёнки) | свободен | свободен | свободен | 0 / свободно | Точных совпадений нет. Созвучно с Ambriel (имя ангела, музыкант на Bandcamp) — другая сфера | **низкий** |
| 3 | **Wrenvue** | Wrenvue OS | wren (крапивник) + vue (вид) | свободен | свободен | свободен | 0 / свободно | Точных совпадений нет. Wren AI (BI-аналитика), Wrensilva (Hi-Fi консоли) — другие знаки. Общая оговорка про «-vue» (см. ниже) | **низкий** |
| 4 | **Mirravue** | Mirravue OS | mirror/mira (смотреть) + vue | свободен | свободен | свободен | 0 / свободно | Точных совпадений нет. Похожий сайт mirrorvue.com (без описания). Оговорка про «-vue» | **низкий** |
| 5 | **Bramblevue** | Bramblevue OS | bramble (ежевичник) + vue | свободен | свободен | свободен | 0 / свободно | Точных совпадений нет. Длинное (3 слога, 10 букв) | **низкий** |
| 6 | **Hollowvue** | Hollowvue OS | hollow (лощина) + vue | свободен | свободен | свободен | 0 / свободно | Точных совпадений нет; «hollow» по-английски также «пустой» — слабая коннотация | низкий–средний |
| 7 | **Hearthvue** | Hearthvue OS | hearth (домашний очаг) + vue | свободен | свободен | свободен | 0 / свободно | Hearth Display — семейный органайзер, позиционируется как «Family OS»: рядом по смыслу с домашним устройством и NAS-ОС | средний |
| 8 | **Ottervue** | Ottervue OS | otter (выдра) + vue | свободен | свободен | свободен | 0 / свободно | Бренды Otter.ai, Otter Browser; на GitHub есть `otter-player/otter-player` (медиаплеер) | средний |
| 9 | **Kestrelvue** | Kestrelvue OS | kestrel (пустельга) + vue | свободен | свободен | свободен | 0 / свободно | Kestrel Vision (машинное зрение), Kestrel (метеоприборы с приложением Kestrel Viewer); много знаков «Kestrel» | средний |
| 10 | **Skynema** | Skynema OS | sky + cinema | свободен | свободен | свободен | 0 / пользователь занят | Точных совпадений нет, но сильная ассоциация со Skynet («Терминатор») и Sky (британский медиа-гигант с агрессивной защитой знака SKY) | средний–высокий |
| 11 | **Kinlark** | Kinlark OS | kin + lark (жаворонок) | **занят** | свободен | свободен | 0 / свободно | Точных совпадений нет; Lark — офисный пакет ByteDance | средний |
| 12 | **Tidelune** | Tidelune OS | tide (прилив) + lune | **занят** | свободен | свободен | 0 / свободно | Tidelight (лампа и приложение); «Tide…» в музыкальном плеере может напоминать TIDAL | средний |
| 13 | **Velluma** | Velluma OS | vellum (пергамент) | **занят** | свободен | свободен | 19 / пользователь занят | Vellum — известная программа вёрстки книг для Mac, Vellum Assistant в сторах; на GitHub есть репо Velluma | средний–высокий |
| 14 | **Loomvie** | Loomvie OS | loom + movie | свободен | свободен | свободен | репо Loomview (похожие) | Loom (Atlassian) — сильный знак в видео; Loovie, Loomi — AI-видеоприложения | **высокий** |
| 15 | **Lyravue** | Lyravue OS | lyra + vue | свободен | свободен | свободен | 0 / свободно | **Lyravue AI** — продукт на Product Hunt (визуализация музыки); рядом LyricVue | **высокий** |

**Оговорка про «-vue».** «Vue» — обычное слово (фр. «вид»), его используют многие марки. Но в кино есть крупный знак **Vue** (сеть кинотеатров Vue в Великобритании и Европе), а в разработке — Vue.js. Составные выдуманные слова (Wrenvue, Mirravue) различимы, но защита у таких знаков слабее, чем у полностью выдуманных (Kinolune, Ambreel). Поэтому имена без «-vue» стоят в списке выше.

Отброшены до таблицы (занят или не резолвится `.com`, часто вместе с `.app`/`.org`; многие уже используются как марки): Lumora, Vidora, Movara, Cinavo, Velvio, Plyra, Kinetra, Strimo, Orvella, Sonavi, Lumivo, Reelkin, Kinomi, Corvio, Lunavo, Emberly, Fennix, Tidewell.

## 4. Короткий список для DEC-004

Пять имён, по которым скрининг **не нашёл** конфликтов, а все домены `.com`, `.app` и `.org` выглядят свободными:

1. **Kinolune** / Kinolune OS — лучший баланс: выдуманное слово, понятный смысл «кино», читается по-русски.
2. **Ambreel** / Ambreel OS — выдуманное, короткое, «reel» намекает на кино.
3. **Wrenvue** / Wrenvue OS.
4. **Mirravue** / Mirravue OS.
5. **Bramblevue** / Bramblevue OS — запасной вариант (длинное).

«Чистые» здесь значит «быстрый скрининг конфликтов не нашёл». Это не гарантия регистрируемости.

## 5. Что сделать владельцу перед решением (DEC-004)

1. Для 1–2 финалистов вручную повторить поиск в официальных базах, по точному написанию и по фонетически похожим вариантам, в классах 9, 38, 41, 42:
   - USPTO: <https://tmsearch.uspto.gov/>
   - EUIPO / TMview (ЕС и национальные ведомства): <https://www.tmdn.org/tmview/>
   - WIPO Global Brand Database: <https://branddb.wipo.int/>
   - Роспатент (если важен рынок РФ): <https://fips.ru/>
2. Проверить App Store и Google Play в нескольких регионах (US, DE, RU), а также Microsoft Store, Flathub, F-Droid и Snap.
3. Проверить хэндлы: GitHub org, X/Twitter, Mastodon, Bluesky, Reddit, YouTube, Telegram, Boosty, Open Collective, Liberapay.
4. Проверить доменное имя у регистратора (премиум и резерв) и только потом покупать. Минимум — `.app` (HSTS preload, удобно для сайта на GitHub Pages) и `.org`.
5. Записать выбор в DEC-004 (ROADMAP) и обновить: `os-release` (DIST), имя пакетов (PKG), домен (SITE), брендинг (DSN).
6. Подача заявки на знак — по желанию владельца. Для open source-проекта она полезна, чтобы закрепить имя за проектом (пример — политика знаков Mozilla и Fedora). Стоимость и юрисдикцию выбрать с поверенным.

---

## English summary

Preliminary knock-out screening (not legal advice) of 15 coined name candidates for the app and its NAS OS (`<Name> OS`). Checks: DNS-level availability of `.com/.app/.org/.io`, GitHub repo and org names, web search for existing products and App Store/Google Play listings, and trademark aggregators. Official USPTO/EUIPO/WIPO search UIs could not be queried from the automated environment, so the owner must repeat those searches manually in Nice classes 9, 38, 41, 42 before deciding (DEC-004). Nothing was registered.

Shortlist (no conflicts found; `.com`, `.app` and `.org` appear unregistered): **Kinolune**, **Ambreel**, **Wrenvue**, **Mirravue**, **Bramblevue**. Rejected as high-risk: Lyravue (an existing "Lyravue AI" music product), Loomvie (close to Loom), Velluma (close to Vellum, `.com` taken). Names ending in "-vue" are weaker marks because of the Vue cinema chain and Vue.js.
