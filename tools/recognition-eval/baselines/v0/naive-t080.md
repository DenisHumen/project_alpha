# Отчёт распознавания: `naive`

Корпус: dev.jsonl, test.jsonl, train.jsonl · кейсов 2035 · файлов 5631 · порог авто 0.8

## Итог

| Группа | Элементов | Оценено | Precision@auto | Coverage | Recall | Тихие ошибки | ECE | Проверка | Вручную |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| всё | 5138 | 4784 | 96.64 % | 41.64 % | 40.24 % | 1.40 % (67) | 0.1424 | 12.23 % | 46.13 % |

## По категориям

| Категория | Элементов | Оценено | Precision@auto | Coverage | Recall | Тихие ошибки | ECE | Проверка | Вручную |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| A1 | 580 | 580 | 95.22 % | 46.90 % | 44.66 % | 2.24 % (13) | 0.1190 | 2.07 % | 51.03 % |
| A2 | 979 | 979 | 91.98 % | 16.55 % | 15.22 % | 1.33 % (13) | 0.1252 | 9.81 % | 73.65 % |
| A3 | 417 | 417 | 88.89 % | 28.06 % | 24.94 % | 3.12 % (13) | 0.0752 | 2.40 % | 69.54 % |
| A4 | 541 | 532 | 100.00 % | 32.52 % | 32.52 % | 0.00 % (0) | 0.2074 | 9.59 % | 57.89 % |
| A5 | 335 | 320 | 97.35 % | 35.31 % | 34.38 % | 0.94 % (3) | 0.2741 | 36.56 % | 28.12 % |
| A6 | 3139 | 2870 | 98.50 % | 48.64 % | 47.91 % | 0.73 % (21) | 0.1420 | 11.60 % | 39.76 % |
| A7 | 942 | 787 | 95.90 % | 49.56 % | 47.52 % | 2.03 % (16) | 0.1818 | 29.86 % | 20.58 % |
| A8 | 221 | 221 | 67.69 % | 58.82 % | 39.82 % | 19.00 % (42) | 0.1948 | 36.20 % | 4.98 % |
| A9 | 262 | 262 | 98.39 % | 23.66 % | 23.28 % | 0.38 % (1) | 0.1339 | 0.00 % | 76.34 % |
| A10 | 751 | 679 | 100.00 % | 36.23 % | 36.23 % | 0.00 % (0) | 0.1291 | 4.71 % | 59.06 % |
| A11 | 302 | 302 | 100.00 % | 30.46 % | 30.46 % | 0.00 % (0) | 0.1500 | 0.00 % | 69.54 % |
| A12 | 115 | 115 | 94.29 % | 30.43 % | 28.70 % | 1.74 % (2) | 0.0443 | 0.00 % | 69.57 % |
| A13 | 269 | 232 | 100.00 % | 51.72 % | 51.72 % | 0.00 % (0) | 0.0500 | 0.00 % | 48.28 % |
| A14 | 1166 | 1010 | 99.80 % | 49.41 % | 49.31 % | 0.10 % (1) | 0.1537 | 8.91 % | 41.68 % |
| A15 | 720 | 532 | 96.89 % | 78.57 % | 76.13 % | 2.44 % (13) | 0.1063 | 10.34 % | 11.09 % |
| A16 | 780 | 779 | 100.00 % | 6.80 % | 6.80 % | 0.00 % (0) | 0.2272 | 1.93 % | 91.27 % |
| A17 | 669 | 577 | 99.36 % | 53.90 % | 53.55 % | 0.35 % (2) | 0.2098 | 22.88 % | 23.22 % |

## По типам

| Тип | Элементов | Оценено | Precision@auto | Coverage | Recall | Тихие ошибки | ECE | Проверка | Вручную |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| anime | 2854 | 2854 | 98.30 % | 32.90 % | 32.34 % | 0.56 % (16) | 0.1961 | 15.73 % | 51.37 % |
| movie | 1143 | 1143 | 95.10 % | 62.55 % | 59.49 % | 3.06 % (35) | 0.1049 | 6.56 % | 30.88 % |
| show | 787 | 787 | 95.27 % | 42.95 % | 40.91 % | 2.03 % (16) | 0.1434 | 7.75 % | 49.30 % |

## Эпизоды по схемам нумерации

| Схема | Размечено | Авто | Верно | Точность | Полнота |
|---|---:|---:|---:|---:|---:|
| anidb | 2341 | 842 | 0 | 0.00 % | 0.00 % |
| tmdb | 2032 | 642 | 242 | 37.69 % | 11.91 % |
| tvdb_absolute | 1877 | 725 | 654 | 90.21 % | 34.84 % |

## Парсинг имён по полям

| Поле | Размечено | Верно | Точность |
|---|---:|---:|---:|
| absolute | 580 | 580 | 100.00 % |
| crc32 | 1299 | 1299 | 100.00 % |
| episodes | 2246 | 504 | 22.44 % |
| number | 2826 | 2267 | 80.22 % |
| part | 232 | 0 | 0.00 % |
| release_group | 2557 | 1897 | 74.19 % |
| season | 893 | 504 | 56.44 % |
| title | 4482 | 3439 | 76.73 % |
| version | 23 | 0 | 0.00 % |
| year | 1092 | 1020 | 93.41 % |

## Парсинг имён по категориям

| Категория | строго | номер без схемы | absolute | crc32 | episodes | number | part | release_group | season | title | version | year |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| A1 | 99.59 % | 99.59 % | 100.00 % | 100.00 % | — | 100.00 % | — | 100.00 % | — | 98.62 % | — | — |
| A2 | 76.23 % | 84.01 % | 100.00 % | 100.00 % | 37.00 % | 87.44 % | — | 86.35 % | 54.01 % | 85.50 % | — | — |
| A3 | 95.42 % | 97.82 % | 100.00 % | 100.00 % | 40.35 % | 100.00 % | — | 92.94 % | 74.19 % | 100.00 % | — | — |
| A4 | 68.45 % | 89.57 % | — | 100.00 % | 0.00 % | 92.39 % | — | 100.00 % | — | 71.19 % | 0.00 % | — |
| A5 | 67.35 % | 82.31 % | — | 100.00 % | 6.17 % | 64.32 % | — | 100.00 % | 50.00 % | 80.94 % | — | 100.00 % |
| A6 | 75.46 % | 86.33 % | 100.00 % | 100.00 % | 19.95 % | 92.54 % | 0.00 % | 74.19 % | 83.52 % | 88.94 % | 0.00 % | 99.65 % |
| A7 | 69.79 % | 76.72 % | — | 100.00 % | 24.28 % | 60.57 % | — | 50.42 % | 57.41 % | 83.46 % | — | 100.00 % |
| A8 | 68.48 % | 88.91 % | — | — | 10.62 % | 93.81 % | — | 71.11 % | 63.16 % | 95.02 % | — | 100.00 % |
| A9 | 43.29 % | 43.29 % | — | 100.00 % | 23.66 % | 23.66 % | — | 73.17 % | 49.60 % | 23.66 % | — | — |
| A10 | 72.88 % | 86.91 % | — | 100.00 % | 0.00 % | 100.00 % | — | 87.30 % | — | 70.29 % | — | 98.96 % |
| A11 | 41.67 % | 41.67 % | — | — | — | — | — | 0.00 % | — | 49.67 % | — | — |
| A12 | 30.00 % | 30.00 % | — | — | — | — | — | — | — | 32.17 % | — | 27.37 % |
| A13 | 43.14 % | 43.14 % | — | — | — | — | 0.00 % | 0.00 % | — | 51.72 % | — | 100.00 % |
| A14 | 68.97 % | 75.73 % | 100.00 % | 100.00 % | 33.68 % | 74.76 % | 0.00 % | 54.46 % | 66.53 % | 78.85 % | 0.00 % | 98.38 % |
| A15 | 81.90 % | 90.21 % | 100.00 % | 100.00 % | 20.95 % | 100.00 % | — | 62.15 % | 100.00 % | 97.18 % | — | 100.00 % |
| A16 | 74.28 % | 79.96 % | 100.00 % | 100.00 % | 39.48 % | 80.60 % | — | 88.53 % | 52.96 % | 79.33 % | 0.00 % | — |
| A17 | 78.81 % | 87.19 % | — | 100.00 % | 9.30 % | 100.00 % | — | 70.19 % | 100.00 % | 89.80 % | — | 91.46 % |

## Калибровка (reliability)

ECE: 0.1424

| Уверенность | Элементов | Средняя уверенность | Доля верных |
|---|---:|---:|---:|
| 0.5–0.6 | 585 | 0.5000 | 0.8496 |
| 0.8–0.9 | 1298 | 0.8500 | 0.9484 |
| 0.9–1.0 | 694 | 0.9500 | 1.0000 |

## Производительность

- запросов к провайдерам: 5631
- на 1 000 файлов: 1000.0000
- промахов кассеты (replay): 0

## Тихие ошибки (67)

- `v0-a12-beauty-and-the-beast-109` `Images/BEAUTY_AND_THE_BEAST.iso` — уверенность 0.8500
- `v0-a12-carrie-066` `Images/CARRIE.iso` — уверенность 0.8500
- `v0-a15-dump-012` `Video/The.Office.S01E03.2160p.BluRay.HEVC-GECKOS.mkv` — уверенность 0.8500
- `v0-a15-dump-077` `Desktop/new/The.Office.S01E03.2160p.NF.WEBRip.HEVC-GalaxyRG.mkv` — уверенность 0.8500
- `v0-a15-dump-096` `Desktop/new/House.of.Cards.S02E03.1080p.WEB.H.264-ROVERS.mkv` — уверенность 0.8500
- `v0-a3-bleach-024` `Anime/Bleach/[Beatrice-Raws] Bleach - 388 [1080p].mkv` — уверенность 0.8500
- `v0-a3-bleach-024` `Anime/Bleach/[Beatrice-Raws] Bleach - 389 [1080p].mkv` — уверенность 0.8500
- `v0-a3-bleach-034` `Anime/Bleach/[LoliHouse] Bleach - 370 (1080p) [2E40A7F0].mkv` — уверенность 0.8500
- `v0-a3-bleach-037` `Anime/Bleach/Bleach - 370 [1080p].mkv` — уверенность 0.8500
- `v0-a3-bleach-039` `Anime/Bleach/[SubsPlease] Bleach - 382 [720p].mkv` — уверенность 0.8500
- `v0-a3-bleach-039` `Anime/Bleach/[SubsPlease] Bleach - 383 [720p].mkv` — уверенность 0.8500
- `v0-a3-bleach-054` `Anime/Bleach/Bleach - 376 [1080p].mkv` — уверенность 0.8500
- `v0-a3-bleach-061` `Anime/Bleach/Bleach - 385 [720p].mkv` — уверенность 0.8500
- `v0-a3-bleach-061` `Anime/Bleach/Bleach - 386 [720p].mkv` — уверенность 0.8500
- `v0-a3-bleach-061` `Anime/Bleach/Bleach - 387 [720p].mkv` — уверенность 0.8500
- `v0-a3-bleach-061` `Anime/Bleach/Bleach - 388 [720p].mkv` — уверенность 0.8500
- `v0-a3-bleach-107` `Anime/Bleach/[Judas] Bleach - 389 (720p) [46970E79].mkv` — уверенность 0.8500
- `v0-a3-bleach-107` `Anime/Bleach/[Judas] Bleach - 390 (720p) [391C92BE].mkv` — уверенность 0.8500
- `v0-a7-dororo-007` `Аниме/Дороро ∕ Dororo [TV] [26 из 26] [RUS(ext), JAP+Sub]/[StudioBand] Dororo [15] [1080p].mkv` — уверенность 0.8500
- `v0-a7-dororo-007` `Аниме/Дороро ∕ Dororo [TV] [26 из 26] [RUS(ext), JAP+Sub]/[StudioBand] Dororo [16] [1080p].mkv` — уверенность 0.8500
- `v0-a7-dororo-007` `Аниме/Дороро ∕ Dororo [TV] [26 из 26] [RUS(ext), JAP+Sub]/[StudioBand] Dororo [17] [1080p].mkv` — уверенность 0.8500
- `v0-a7-shameless-097` `Сериалы/Бесстыдники/Сезон 11/Shameless.S11E02.1080p.rus.LostFilm.mkv` — уверенность 0.8500
- `v0-a7-shameless-097` `Сериалы/Бесстыдники/Сезон 11/Shameless.S11E04.1080p.rus.LostFilm.mkv` — уверенность 0.8500
- `v0-a7-shameless-097` `Сериалы/Бесстыдники/Сезон 11/Shameless.S11E05.1080p.rus.LostFilm.mkv` — уверенность 0.8500
- `v0-a8-a-star-is-born-061` `Кино/A Star Is Born.mkv` — уверенность 0.8500
- `v0-a8-a-star-is-born-069` `Кино/A Star Is Born (1976)/A Star Is Born.mkv` — уверенность 0.8500
- `v0-a8-battlestar-galactica-024` `TV/Battlestar Galactica/Battlestar.Galactica.S01E16.720p.HDTV.x264.mkv` — уверенность 0.8500
- `v0-a8-battlestar-galactica-073` `TV/Battlestar Galactica/Battlestar Galactica - S01E16.mkv` — уверенность 0.8500
- `v0-a8-battlestar-galactica-119` `TV/Battlestar Galactica/Battlestar.Galactica.S01E21.720p.HDTV.x264.mkv` — уверенность 0.8500
- `v0-a8-cinderella-012` `Кино/Cinderella (1950)/Cinderella.576p.DVDRip.XviD-DiMENSION.mkv` — уверенность 0.8500
- `v0-a8-cinderella-033` `Movies/Cinderella (1950)/Cinderella.480p.DVDRip.XviD-TEPES.mkv` — уверенность 0.8500
- `v0-a8-cinderella-116` `Movies/Cinderella/Cinderella.mkv` — уверенность 0.8500
- `v0-a8-crash-059` `Movies/Crash/Crash.mkv` — уверенность 0.8500
- `v0-a8-fantastic-four-049` `Movies/Fantastic Four.mkv` — уверенность 0.8500
- `v0-a8-ghostbusters-076` `Movies/Ghostbusters (1984)/Ghostbusters.mkv` — уверенность 0.8500
- `v0-a8-godzilla-022` `Movies/Godzilla/Godzilla.mkv` — уверенность 0.8500
- `v0-a8-halloween-010` `Video/Halloween.mkv` — уверенность 0.8500
- `v0-a8-hellboy-029` `Кино/Hellboy (2004)/Hellboy.480p.DVDRip.x264-CMRG.mkv` — уверенность 0.8500
- `v0-a8-hellboy-054` `Movies/Hellboy (2004)/Hellboy.mkv` — уверенность 0.8500
- `v0-a8-hellboy-108` `Movies/Hellboy/Hellboy.mkv` — уверенность 0.8500
- `v0-a8-king-kong-081` `Кино/King-Kong.720p.mkv` — уверенность 0.8500
- `v0-a8-king-kong-094` `Movies/King Kong (1976)/King Kong.mkv` — уверенность 0.8500
- `v0-a8-little-women-038` `Movies/Little Women/Little Women.mkv` — уверенность 0.8500
- `v0-a8-planet-of-the-apes-048` `Movies/Planet of the Apes (1968)/Planet.of.the.Apes.480p.DVDRip.x264-NOGRP.mkv` — уверенность 0.8500
- `v0-a8-robocop-002` `Movies/RoboCop (1987)/RoboCop.576p.TVRip.XviD-KILLERS.mkv` — уверенность 0.8500
- `v0-a8-robocop-083` `Video/RoboCop.720p.WEB-DL.HEVC-YTS.mkv` — уверенность 0.8500
- `v0-a8-robocop-098` `Video/RoboCop (1987)/RoboCop.mkv` — уверенность 0.8500
- `v0-a8-robocop-122` `Кино/RoboCop.576p.DVD5.XviD-YTS.mkv` — уверенность 0.8500
- `v0-a8-scarface-068` `Кино/Scarface (1932)/Scarface.480p.DVD5.DivX-SVA.mkv` — уверенность 0.8500
- `v0-a8-shameless-018` `TV/Shameless/Shameless.S05E15.720p.HDTV.x264.mkv` — уверенность 0.8500
- `v0-a8-shameless-044` `TV/Shameless/Shameless - S08E17.mkv` — уверенность 0.8500
- `v0-a8-shameless-046` `TV/Shameless/Shameless - S06E16.mkv` — уверенность 0.8500
- `v0-a8-shameless-107` `TV/Shameless/Shameless.S05E13.720p.HDTV.x264.mkv` — уверенность 0.8500
- `v0-a8-shameless-113` `TV/Shameless/Shameless - S05E15.mkv` — уверенность 0.8500
- `v0-a8-shameless-121` `TV/Shameless/Shameless - S08E18.mkv` — уверенность 0.8500
- `v0-a8-solaris-110` `Movies/Solaris/Solaris.mkv` — уверенность 0.8500
- `v0-a8-suspiria-037` `Video/Suspiria.mkv` — уверенность 0.8500
- `v0-a8-the-fly-015` `Movies/The Fly/The Fly.mkv` — уверенность 0.8500
- `v0-a8-the-fly-126` `Кино/The.Fly.480p.DVD5.DivX-W4NK3R.mkv` — уверенность 0.8500
- `v0-a8-the-italian-job-050` `Кино/The.Italian.Job.480p.TVRip.DivX-BLOW.mkv` — уверенность 0.8500
- `v0-a8-the-italian-job-066` `Movies/The Italian Job/The Italian Job.mkv` — уверенность 0.8500
- `v0-a8-the-lion-king-086` `Movies/The Lion King/The Lion King.mkv` — уверенность 0.8500
- `v0-a8-the-magnificent-seven-067` `Video/The Magnificent Seven (1960)/The Magnificent Seven.mkv` — уверенность 0.8500
- `v0-a8-the-mummy-019` `Movies/The Mummy/The Mummy.mkv` — уверенность 0.8500
- `v0-a8-the-mummy-030` `Movies/The.Mummy.480p.SATRip.x264-W4NK3R.mkv` — уверенность 0.8500
- `v0-a8-true-grit-057` `Video/True Grit (1969)/True Grit.mkv` — уверенность 0.8500
- `v0-a9-battlestar-galactica-037` `Series/Battlestar.Galactica.S01/Battlestar Galactica - S01E11.mkv` — уверенность 0.8500
