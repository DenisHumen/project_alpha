<!-- SPDX-License-Identifier: MPL-2.0 -->

# Тестовые медиа (QA-002)

Набор файлов для тестов сканера, распознавания, `pa-probe` и плееров на всех платформах:
контейнеры и кодеки, HDR10 / HDR10+ / HLG / Dolby Vision, многоканальный звук, субтитры
(SRT, ASS, PGS, VobSub, DVB, tx3g, WebVTT), главы, много дорожек, пограничные случаи,
gapless-альбом и музыкальные форматы. Каждый файл описан в [`manifest.toml`](manifest.toml):
путь, лицензия, назначение, задачи, которые его используют, и ожидаемые свойства.

**Бинарники в git не хранятся.** Вместо этого в репозитории — воспроизводимый генератор и
манифест. Так набор не раздувает историю, не требует Git LFS для обычной работы и
проверяем: после генерации `verify.py` сравнивает каждый файл с манифестом через ffprobe.

| Каталог | Что | Откуда | Размер |
|---|---|---|---|
| `synthetic/` | ~95 синтетических файлов | `just tests::media` (`tools/qa/media/generate.py`) | ≈ 55 МБ |
| `external/` | Реальные фильмы и звук под свободными лицензиями | `just tests::media-fetch` | ≈ 570 МБ |
| `library/` | Фейковая медиатека для скриншотов и демо (релиз 0.1) | `just tests::media-library` | ≈ 40 МБ |

Все три каталога в `.gitignore`. Общий бюджет набора (`synthetic/` + `external/`) — меньше 2 ГБ;
`verify.py` падает, если он превышен.

## Быстрый старт

```bash
just tests::media              # сгенерировать synthetic/ (≈ 1–2 мин, -j 2 по умолчанию, максимум -j 4)
just tests::media-verify       # проверить по манифесту: "94 ok, 0 failed"
just tests::media-fetch        # скачать external/ (по желанию)
just tests::media-verify --external --strict
just tests::media-library      # фейковая медиатека в library/
just tests::media-selftest     # юнит-тесты инструментов (FFmpeg не нужен)
```

Отдельные рецепты: `just tests::media --only hdr-dv-p81 s-bitmap`; список — `--list`.

### Что нужно для генерации

- **FFmpeg ≥ 7.1** (проверено на 8.0.1) с `libx264`, `libx265`, `libsvtav1`, `libvpx`, `libopus`,
  `libvorbis`, `libmp3lame`, `libtheora` и BSF `dovi_rpu`. Пакеты Ubuntu 25.10+/Debian 13 подходят.
  GPL-энкодеры работают только на машине разработчика; ни один сгенерированный файл не попадает
  в приложения (CLAUDE.md: в клиентах только LGPL-сборки FFmpeg).
- **cargo** — один раз собрать `tools/qa/media/hdrmeta` (HDR10+ и Dolby Vision, см. ниже).
- Шрифт **DejaVu Sans** (`fonts-dejavu-core`) — вкладывается в MKV с ASS.
- Python ≥ 3.11 (`tomllib`).

Генератор проверяет инструменты заранее и пишет, чего не хватает. Байтовая идентичность между
версиями энкодеров не гарантируется, поэтому в манифесте не хеши, а **ожидаемые свойства**
(кодек, профиль, размеры, частота кадров, цвет, HDR-метаданные, каналы, языки и флаги дорожек,
главы, кодировка текста).

## Как устроены сложные сэмплы

- **HDR10 / HLG** — x265 Main10 с VUI BT.2020 + PQ/HLG, MDCV (P3-D65, 1000/0.0001 нит) и CLL 1000/400.
  Картинка — SDR-таблица, помеченная как HDR: выглядит тускло, но проверяет переключение режима
  дисплея и передачу метаданных. Для оценки качества картинки — реальные HDR-файлы владельца (QA-011).
- **HDR10+** — сборки x265 в дистрибутивах без `HDR10_PLUS`, поэтому SEI ST 2094-40 (ITU-T T.35)
  вставляет `tools/qa/media/hdrmeta` (Rust, крейт `hdr10plus`, MIT) в каждый кадр сырого HEVC.
- **Dolby Vision 5 / 8.1 / 8.4** — `hdrmeta` генерирует RPU крейтом `dolby_vision` (MIT, тот же код,
  что `dovi_tool generate`) и добавляет NAL 62 в каждый access unit; конфигурацию контейнера
  (`dvcC` в MP4, BlockAdditionMapping в MKV) пишет BSF FFmpeg `dovi_rpu`. Профиль 5 — с
  неуказанным VUI, как настоящие P5-файлы: без RPU-reshaping цвета неверные, это и проверяется.
- **PGS** — у FFmpeg нет PGS-энкодера, поэтому `.sup` пишет `tools/qa/media/pgs.py`; VobSub и DVB
  получаются из него перекодированием битмапов. Событие N показывает N жёлтых квадратов.
- **Многоканальный звук** — канал i звучит частотой 300 + 100·i Гц (FL 300, FR 400, FC 500, LFE 600…),
  поэтому перепутанная раскладка слышна и измерима.
- **Gapless** — одна непрерывная свип-волна, разрезанная на 3 трека по сэмплам, не кратным
  размеру кадра кодека, в FLAC, MP3 (LAME delay/padding), AAC/M4A и Opus. Щелчок на стыке —
  ошибка gapless.

Чего нет и почему: Dolby Atmos (E-AC-3 JOC, TrueHD), DTS-HD MA и DTS:X, DV профиль 7 (FEL),
DSD, CEA-608 — свободных энкодеров нет, а коммерческие демо-ролики несвободны. Их проверяют на
устройствах владельца по чек-листу QA-011. Дисковые структуры (BDMV, VIDEO_TS, ISO) — в фикстурах
LIB-007.

## Внешние файлы

Только свободные лицензии, источник — `download.blender.org`:

| Файл | Лицензия | Атрибуция |
|---|---|---|
| Sintel (трейлеры, субтитры) | CC BY 3.0 | © Blender Foundation, durian.blender.org |
| Tears of Steel (720p, субтитры) | CC BY 3.0 | © Blender Foundation, mango.blender.org |
| Tears of Steel OST | CC BY-ND 3.0 (только без изменений) | © Joram Letwory, tearsofsteel.org |
| Elephants Dream (480p, AC-3 5.1) | CC BY 2.5 | © Blender Foundation, orange.blender.org |

`fetch.py` проверяет размер, а после закрепления — SHA-256. Мейнтейнер при первой загрузке
выполняет `just tests::media-fetch --pin`, и хеши записываются в манифест.
Не добавляйте в набор файлы без свободной лицензии (трейлеры студий, демо Dolby/DTS и т. п.).

## Фейковая медиатека (`library/`)

`tools/qa/media/library.py` строит дерево `Movies/`, `TV Shows/`, `Anime/`, `Music/` с
реалистичными именами: Plex/Jellyfin-папки, scene-имена (`Tears.of.Steel.2012.1080p.BluRay.x264-GRP.mkv`),
`S01E01` и `1x01`, спецвыпуски, `[Group] Title - 01`, NCOP/NCED, трейлеры и Featurettes, части
фильма, русские имена и транслит, `{tmdb-…}`, внешние субтитры, альбомы с тегами и обложками.
Фильмы и сериалы — почти все общественное достояние или Creative Commons, поэтому постеры TMDB
на скриншотах — свободных произведений. Видео — один и тот же крошечный ролик (жёсткие ссылки).

## Формат манифеста

```toml
[[file]]
id = "hdr-dv-p81-mkv"                       # стабильный ID, на него ссылаются тесты
path = "synthetic/hdr/dv-p8.1-hevc-1080p.mkv"
source = "synthetic"                        # [sources.<имя>] задаёт лицензию и атрибуцию по умолчанию
recipe = "hdr-dv-p81"                       # рецепт генератора (по умолчанию = id)
licence = "…"                               # переопределение лицензии (SPDX)
what = "…"                                  # зачем файл нужен
used_by = ["PLAY-DSK-003"]                  # задачи ROADMAP
url = "…"; size = 0; sha256 = "…"           # только для внешних файлов
expect = { format = "matroska,webm", duration = 2.0, chapters = 3, chapter_titles = […], tags = {…}, probe = "error" }
streams = [{ type = "video", codec = "hevc", dv_profile = 8, frame_side_data = ["Dolby Vision RPU Data"] }]
text = { encoding = "cp1251", bom = false, crlf = false, contains = "…" }
```

Ключи `streams` (по порядку дорожек, число дорожек должно совпадать): `type`, `codec`, `codec_tag`,
`profile`, `width`, `height`, `pix_fmt`, `field_order`, `fps`, `sar`, `color_transfer`,
`color_primaries`, `channels`, `layout`, `sample_rate`, `sample_fmt`, `bits`, `duration_ts`,
`lang`, `title`, `filename`, `mimetype`, флаги `default`/`forced`/`comment`/`hearing_impaired`/`attached_pic`,
`side_data` (на уровне дорожки), `frame_side_data` (в первых кадрах), `dv_profile`, `dv_compat`, `rotation`.

Добавляя файл: рецепт в `generate.py`, запись в манифест, `just tests::media --only <рецепт>`,
`just tests::media-verify --only <id>`, `just tests::media-selftest`.

## Как использовать в тестах

Тесты находят файлы по `id` через манифест, а не по жёстким путям. Если `synthetic/` не
сгенерирован, тест должен пропускаться с понятным сообщением (`run: just tests::media`), а не
падать. В CI набор генерируется в джобах, которым он нужен (см. `docs/dev/testing.md`).
