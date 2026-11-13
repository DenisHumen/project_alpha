# Лицензии данных маппингов аниме (LEGAL-003)

> **Это не юридическая консультация.** Инженерная сводка участников проекта (с помощью AI-агента) по состоянию репозиториев на **2026-11-13**. Перед изменением политики встраивания сверяйтесь с первоисточниками; спорные вопросы решает владелец, при необходимости — с юристом.

Задача META-008 планирует снапшот маппингов в бандле приложения, обновление с GitHub и резервный форк в нашей организации. Ниже — что из этого допустимо для каждого источника.

## 1. Сводка

| Источник | Лицензия | Бандл в приложении / копия в нашем git | Загрузка на устройство пользователя в рантайме | Решение |
|---|---|---|---|---|
| [anime-offline-database](https://github.com/cedya77/anime-offline-database) (форк cedya77, продолжение manami-project) | **ODbL-1.0 + DbCL-1.0** | **Да**, с атрибуцией и share-alike для базы | Да | Встраиваем (условия — §2) |
| [Anime-Lists/anime-lists](https://github.com/Anime-Lists/anime-lists) (`anime-list-master.xml`) | **Не указана** (по умолчанию все права защищены) | **Нет**, пока нет лицензии | Да, с атрибуцией (приемлемый риск, см. §3) | Только загрузка с upstream; запросить лицензию |
| [Fribb/anime-lists](https://github.com/Fribb/anime-lists) (`anime-list-full.json`) | **Не указана** | **Нет**, пока нет лицензии | Да, с атрибуцией (приемлемый риск, см. §4) | Только загрузка с upstream; запросить лицензию; запасной план — собирать свой маппинг |
| `animetitles.xml` (копия дампа AniDB в Anime-Lists) | Условия AniDB | **Нет** | Брать с AniDB напрямую, не чаще раза в сутки | Не использовать копию из Anime-Lists |

## 2. anime-offline-database — ODbL-1.0

**Факты (проверено по репозиториям):**

- `manami-project/anime-offline-database` **архивирован 2026-07-04**, последний релиз `2026-27`. Файл `LICENSE`: база — ODbL 1.0, содержимое — DbCL 1.0.
- Продолжение — [`cedya77/anime-offline-database`](https://github.com/cedya77/anime-offline-database) (на него уже перешёл Fribb). Лицензия та же (тот же `LICENSE`). Данные публикуются **в релизах**, а не в дереве репозитория: `releases/download/latest/anime-offline-database.jsonl`, еженедельные теги `<год>-<неделя>`.
- README форка: можно делиться и адаптировать, **в том числе коммерчески**, при условии атрибуции, сохранения той же лицензии для распространяемой базы и без технических мер, ограничивающих использование. Атрибуция должна называть **и форк, и manami-project**. Права на данные отдельных провайдеров остаются у провайдеров; база содержит идентификаторы и связи между ними.

**Что это значит для project_alpha:**

1. ODbL распространяется **только на базу данных**, а не на код приложения (MPL-2.0) — копилефт базы не «заражает» код.
2. Встраивание снапшота в бандл и обновление с GitHub — это публичное распространение базы → обязательно:
   - **Атрибуция** в «О программе» и в `THIRD_PARTY_NOTICES` (LEGAL-009): «Contains information from anime-offline-database (cedya77, continuation of manami-project), made available under the ODbL v1.0» со ссылками на оба репозитория и на текст ODbL; рядом с файлом базы в бандле — `LICENSE` из upstream.
   - **Share-alike:** если мы распространяем **изменённую** базу (урезанный индекс, слияние с другими источниками), она тоже должна быть под ODbL, и нужно дать доступ к ней или к способу её получить (наш открытый скрипт сборки индекса в `tools/` это покрывает — ODbL §4.6).
   - **Без технических ограничений:** файл базы в бандле хранить в открытом формате (JSON/SQLite без шифрования), либо рядом публиковать незашифрованную версию (ODbL §4.7).
3. «Произведённые работы» (экран с названием и синонимами, результат матча) под share-alike не попадают, но требуют уведомления о том, что использована база под ODbL (ODbL §4.3) — закрывается той же строкой в «О программе».
4. Нельзя смешивать в **одну распространяемую базу** данные ODbL и данные без лицензии (Anime-Lists, Fribb): результат пришлось бы выпускать под ODbL, а у нас нет права лицензировать чужие данные. Индекс в бандле строить **только из ODbL-источников**; данные Anime-Lists/Fribb держать отдельными файлами, загруженными пользователем.

## 3. Anime-Lists/anime-lists — лицензии нет

**Факты:**

- В репозитории нет файла лицензии, GitHub показывает `license: null`. README описывает формат и проекты-потребители (HAMA для Plex, AniDB-скрейпер Kodi), условий использования нет. Последний push — 2026-11-09.
- Issue [#613 «Please add a license to your data»](https://github.com/Anime-Lists/anime-lists/issues/613) (открыт 2026-07-25, просят CC0) — **ответа мейнтейнеров нет**, только пинг сообщества 2026-09-22.
- Issue [#143 «Permission to use the anime lists»](https://github.com/Anime-Lists/anime-lists/issues/143) (2022, Jellyfin-плагин): участник сообщества **без официальных полномочий** ответил, что использование с указанием источника «должно быть нормально». Это не лицензия.
- В репозитории лежит `animetitles.xml` — копия дампа AniDB; на неё распространяются условия AniDB, а не Anime-Lists.

**Оценка:** без лицензии по умолчанию действует авторское право / право на базу данных (в ЕС — sui generis). Копирование файла в наш репозиторий, форк-зеркало и встраивание в бинарь — распространение без разрешения. Загрузка пользователем публично выложенного файла с upstream на своё устройство — то же, что делают HAMA, Shoko, Kodi-скрейперы; риск оцениваем как низкий, но ненулевой.

**Решение:**

- **Не встраивать** в бандл и **не держать копию/форк** в организации проекта, пока нет лицензии.
- Приложение (или сервер пользователя) **само скачивает** `anime-list-master.xml` с `raw.githubusercontent.com/Anime-Lists/anime-lists/...` (ETag, не чаще раза в сутки), хранит в локальном кэше; при недоступности — работает без него (маппинг из ODbL-базы + TMDB/TVDB external IDs).
- Атрибуция в «О программе» со ссылкой на репозиторий и приглашением исправлять маппинги upstream.
- `animetitles.xml` из этого репозитория **не использовать**; названия AniDB брать по правилам AniDB (см. [providers.md](providers.md), раздел AniDB).
- **Владельцу:** присоединиться к #613 с просьбой о лицензии (предпочтительно CC0 или ODbL) и разрешением на включение в open source-приложение. Черновик комментария — в §5.

## 4. Fribb/anime-lists — лицензии нет

**Факты:**

- Лицензии нет (`license: null`). README: файлы генерируются из anime-offline-database (форк cedya77, ODbL) и Anime-Lists (без лицензии), слиянием по AniDB ID. Обновляется ежедневно (push 2026-11-12).
- Issue [#33 «Clarify license and reuse terms for generated mapping data»](https://github.com/Fribb/anime-lists/issues/33) (2026-10-30) закрыт **самим автором вопроса** через 10 минут как `not_planned`, без ответа мейнтейнера. Официальной позиции нет.

**Оценка:** производная база от ODbL-источника при публичном распространении должна была бы быть под ODbL, но вторая половина (Anime-Lists) без лицензии, поэтому Fribb не может сам дать чистую лицензию на результат. Для нас Fribb — тот же статус, что Anime-Lists: «публично доступно, лицензии нет».

**Решение:**

- **Не встраивать** и не зеркалить; загружать на устройство пользователя с upstream (ETag, раз в сутки), с атрибуцией.
- **Запасной план (рекомендуется для META-008):** собирать собственный кросс-ID маппинг на устройстве из anime-offline-database (ODbL, встраиваемо) + загруженного пользователем Anime-Lists. Код сборки — наш (MPL-2.0); результат не распространяется, а создаётся локально. Так зависимость от Fribb становится опциональной.
- **Владельцу:** открыть новый issue в Fribb/anime-lists с вопросом о лицензии. Черновик — в §5.

## 5. Черновики обращений (отправляет владелец)

Публикация в чужих репозиториях — от имени владельца, поэтому агенты их не отправляют.

**Anime-Lists, комментарий в #613:**

> Hi! We are building project_alpha, a non-commercial open-source (MPL-2.0) media player that runs entirely on users' devices (no project servers). We would like to use anime-list-master.xml to map AniDB episodes to TVDB/TMDB. Could you add a licence to the data (CC0 or ODbL would both work for us)? Until then we only download the file from this repo at runtime on the user's device, with credit and a link back here, and we do not redistribute it. Thank you for maintaining the lists!

**Fribb/anime-lists, новый issue «Licence for the generated mapping files»:**

> Hi! project_alpha is a non-commercial open-source (MPL-2.0) media player; all requests go from users' devices, no servers. We'd like to use anime-list-full.json for cross-ID mapping. Could you state a licence for the generated files? Since they merge anime-offline-database (ODbL) and Anime-Lists (no licence yet), ODbL for your output plus a licence from Anime-Lists would make reuse unambiguous. Meanwhile we only download the file at runtime with attribution and do not redistribute it.

## 6. Что меняется в META-008

- Вместо «снапшот Anime-Lists и Fribb в бандле» — **в бандле только anime-offline-database (ODbL)** + `LICENSE` + атрибуция; Anime-Lists и Fribb — **загрузка с upstream на устройство** при первом запуске и по расписанию.
- «Резервный форк в нашей организации» — **только для anime-offline-database**; для Anime-Lists/Fribb — после появления лицензии.
- Офлайн-тест маппинга в CI использует **тестовые фикстуры** (несколько записей, написанных вручную, или ODbL-данные), а не копию Anime-Lists/Fribb.
- При появлении лицензии у Anime-Lists/Fribb решение пересматривается отдельным PR в этот файл.
