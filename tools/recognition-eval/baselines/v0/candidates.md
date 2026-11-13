# Кандидаты из локального индекса названий (META-007)

Корпус: train.jsonl, dev.jsonl. Парсер 0.2.0. Индекс: 38724 записей, 220155 названий. Без внешних запросов.

## Итог

| Срез | Элементов | R@1 | R@3 | R@5 | R@10 | MRR |
|---|---:|---:|---:|---:|---:|---:|
| **все** | 3857 | 77.39 % | 95.51 % | 97.04 % | 99.51 % | 0.8636 |
| split `dev` | 844 | 79.03 % | 99.76 % | 99.76 % | 99.76 % | 0.8831 |
| split `train` | 3013 | 76.93 % | 94.32 % | 96.28 % | 99.44 % | 0.8582 |
| kind `anime` | 2278 | 67.16 % | 92.89 % | 95.13 % | 99.30 % | 0.7985 |
| kind `movie` | 952 | 92.44 % | 98.84 % | 99.68 % | 99.68 % | 0.9570 |
| kind `show` | 627 | 91.71 % | 100.00 % | 100.00 % | 100.00 % | 0.9585 |
| role `extra` | 279 | 84.95 % | 93.19 % | 95.70 % | 95.70 % | 0.8938 |
| role `main` | 3377 | 75.57 % | 95.50 % | 97.04 % | 99.85 % | 0.8540 |
| role `part` | 169 | 98.82 % | 100.00 % | 100.00 % | 100.00 % | 0.9941 |
| role `sample` | 32 | 90.62 % | 93.75 % | 93.75 % | 93.75 % | 0.9219 |

## По категориям

| Категория | Элементов | R@1 | R@3 | R@5 | R@10 | MRR |
|---|---:|---:|---:|---:|---:|---:|
| A1 | 456 | 36.40 % | 81.14 % | 88.60 % | 100.00 % | 0.5895 |
| A2 | 810 | 21.98 % | 82.59 % | 88.89 % | 99.38 % | 0.5164 |
| A3 | 333 | 0.90 % | 69.67 % | 79.88 % | 100.00 % | 0.3541 |
| A4 | 405 | 84.69 % | 97.53 % | 97.53 % | 100.00 % | 0.9142 |
| A5 | 247 | 95.55 % | 100.00 % | 100.00 % | 100.00 % | 0.9777 |
| A6 | 2357 | 76.11 % | 94.06 % | 96.22 % | 99.66 % | 0.8521 |
| A7 | 656 | 93.60 % | 99.54 % | 99.54 % | 99.54 % | 0.9654 |
| A8 | 205 | 68.29 % | 99.51 % | 100.00 % | 100.00 % | 0.8367 |
| A9 | 216 | 92.59 % | 100.00 % | 100.00 % | 100.00 % | 0.9630 |
| A10 | 513 | 90.06 % | 95.91 % | 97.27 % | 97.27 % | 0.9315 |
| A11 | 236 | 92.80 % | 100.00 % | 100.00 % | 100.00 % | 0.9640 |
| A12 | 86 | 95.35 % | 100.00 % | 100.00 % | 100.00 % | 0.9767 |
| A13 | 169 | 98.82 % | 100.00 % | 100.00 % | 100.00 % | 0.9941 |
| A14 | 848 | 88.80 % | 99.29 % | 99.29 % | 99.29 % | 0.9369 |
| A15 | 501 | 94.21 % | 98.80 % | 99.00 % | 100.00 % | 0.9657 |
| A16 | 608 | 26.32 % | 89.31 % | 94.41 % | 100.00 % | 0.5607 |
| A17 | 476 | 96.64 % | 97.69 % | 97.69 % | 97.69 % | 0.9716 |

## Промахи (нет в топ-10): 19

| Кейс | Ожидается | Запрос | Топ-1 | Ранг |
|---|---|---|---|---:|
| `v0-a10-1612-031` | 1612 | — | — | — |
| `v0-a10-1612-038` | 1612 | On Set | Tsukio | — |
| `v0-a10-1612-038` | 1612 | — | — | — |
| `v0-a10-bleach-memories-of-nobody-061` | Bleach Movie 1: Memories of Nobody | Trailer 2 | Uchuu | — |
| `v0-a10-evangelion-2-0-you-can-not-advance-004` | Evangelion Movie 2: Ha | Trailer 3 | Uchuu | — |
| `v0-a10-hotaru-no-haka-108` | Hotaru no Haka | Trailer 3 | Uchuu | — |
| `v0-a10-kaze-tachinu-071` | Kaze Tachinu | Trailer 1 | Uchuu | — |
| `v0-a10-perfect-blue-050` | Perfect Blue | Trailer 1 | Uchuu | — |
| `v0-a10-sennen-joyuu-042` | Sennen Joyuu | Trailer 3 | Uchuu | — |
| `v0-a14-byousoku-5-centimeter-011` | Byousoku 5 Centimeter | Trailer 3 | Uchuu | — |
| `v0-a14-evangelion-3-0-1-0-thrice-upon-a-time-002` | Shin Evangelion Movie:|| | Trailer 3 | Uchuu | — |
| `v0-a17-tonari-no-totoro-110` | Tonari no Totoro | Trailer 3 | Uchuu | — |
| `v0-a2-kimetsu-no-yaiba-hashira-geiko-hen-035` | Kimetsu no Yaiba: Hashira Geiko-hen | Kimetsu no Yaiba | Kimetsu no Yaiba | 12 |
| `v0-a2-kimetsu-no-yaiba-hashira-geiko-hen-035` | Kimetsu no Yaiba: Hashira Geiko-hen | Kimetsu no Yaiba | Kimetsu no Yaiba | 12 |
| `v0-a2-kimetsu-no-yaiba-hashira-geiko-hen-035` | Kimetsu no Yaiba: Hashira Geiko-hen | Kimetsu no Yaiba | Kimetsu no Yaiba | 12 |
| `v0-a2-kimetsu-no-yaiba-hashira-geiko-hen-083` | Kimetsu no Yaiba: Hashira Geiko-hen | Kimetsu no Yaiba | Kimetsu no Yaiba | 12 |
| `v0-a2-kimetsu-no-yaiba-hashira-geiko-hen-083` | Kimetsu no Yaiba: Hashira Geiko-hen | Kimetsu no Yaiba | Kimetsu no Yaiba | 12 |
| `v0-a10-sen-to-chihiro-no-kamikakushi-089` | Sen to Chihiro no Kamikakushi | Trailer 3 | Uchuu | — |
| `v0-a17-sen-to-chihiro-no-kamikakushi-033` | Sen to Chihiro no Kamikakushi | Trailer 2 | Uchuu | — |
