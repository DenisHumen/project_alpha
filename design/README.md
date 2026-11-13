<!-- SPDX-License-Identifier: MPL-2.0 -->

# design/ — дизайн-система Kinolune

Здесь лежат **исходники дизайна**, из которых генерируется код и ассеты всех платформ: токены, иконки, бренд, иллюстрации, анимации, плейсхолдеры. Каталог принадлежит workstream DSN (см. `docs/ROADMAP.md` §0.4).

**Главное правило:** источник правды — файлы в git. Инструмент макетов (рекомендуется Penpot) — только редактор. Почему так — в [`decisions/0001-design-tool.md`](decisions/0001-design-tool.md).

## Структура

```
design/
├── README.md              # этот файл
├── LICENSES.md            # лицензии шрифтов, иконок и стороннего арта
├── justfile, package.json # инструменты: `just design setup|check|build|tokens|icons|fonts|contrast`
├── decisions/             # ADR дизайн-workstream (0001 инструмент, 0002 иконки и шрифт, генерация токенов)
├── tokens/                # W3C DTCG JSON — источник правды (DSN-002), см. tokens/README.md
│   ├── primitive.tokens.json   # палитра, отступы 0–100, радиусы, тени, шрифт, длительности, кривые
│   ├── semantic.tokens.json    # роли, тёмная тема по умолчанию: bg/*, text/*, accent, focus/*, glass/* …
│   ├── themes/light.tokens.json  # светлая тема — переопределяет только цвета
│   ├── component.tokens.json   # poster/focus/scale, button/*, media-badge/* …
│   ├── density/                # tv, desktop, tablet, phone: типографика, сетка, радиусы, размеры
│   ├── contrast.pairs.json     # пары текст/фон для проверки WCAG AA
│   └── generated/              # Tokens.swift, PaTokens.kt, tokens.css (генерируется, DSN-003)
├── icons/                 # иконки (DSN-004), см. icons/svg/README.md
│   ├── manifest.json      # курированный набор: наше имя → иконка Lucide
│   ├── svg/               # исходные SVG 24×24, штрих 1.75 px
│   └── generated/apple/   # PaIcons.xcassets + PaIcon.swift (генерируется)
├── fonts/                 # Inter Variable (DSN-005): fonts.lock.json с SHA-256, образец specimen.html
├── flutter/pa_design/     # Flutter-пакет: токены (Dart), иконки, шрифт Inter — генерируется отсюда
├── scripts/               # валидатор токенов, импорт/линтер/генератор иконок, загрузка шрифтов, тесты
├── brand/                 # бренд Kinolune: знак, словесный знак, иконки всех платформ (DSN-006, brand/README.md)
├── illustrations/         # пустые состояния, онбординг (SVG, при необходимости PNG)
├── motion/                # .riv (Rive) и .json (Lottie)
├── placeholders/          # собственные плейсхолдеры постеров и обложек (не TMDB-арт!)
└── sources/               # ссылки на проекты Penpot и экспорт мастер-файлов
```

## Быстрый старт

```bash
just design setup     # npm ci в design/ (Node ≥ 20)
just design check     # то же, что CI: токены (DTCG, ссылки, контраст), иконки, свежесть сгенерированного, тесты
just design build     # перегенерировать код токенов (Dart, Swift, Kotlin, CSS), иконки, бренд, образец шрифта
just design fonts     # скачать Inter (SHA-256) — нужно перед сборкой Flutter
```

**Flutter-приложение** подключает пакет `pa_design` (`path:`-зависимость на `design/flutter/pa_design`) и получает `PaColorScheme.dark|light`, `PaDensity.tv|desktop|tablet|phone` (типографика Inter, сетка, радиусы), `PaTokens` (фокус, движение, материалы), `PaIcon` + `PaIconView`. Пакет называется по внутреннему префиксу `pa_` (ADR-0011: пакеты не переименовываем до проверки товарного знака Kinolune).

| Каталог | Формат | Git LFS | Задача |
|---|---|---|---|
| `tokens/` | `*.tokens.json` (DTCG) | нет — нужны диффы и чтение в CI | DSN-002, DSN-003 |
| `icons/svg/` | `*.svg` | нет — текст, ревью построчно | DSN-004 |
| `fonts/` | `*.ttf`, `*.woff2` — **не коммитятся**, скачиваются `just design fonts` с проверкой SHA-256 | не нужен | DSN-005 |
| `brand/`, `illustrations/`, `placeholders/` | `*.svg` — без LFS; `*.png`, `*.psd`, `*.kra`, `*.blend`, `*.icon` — LFS, кроме сгенерированных `brand/export/`, `brand/preview.png`, `placeholders/export/` (обычный git, см. `.gitattributes`) | частично | DSN-006 |
| `motion/` | `*.riv`, `*.lottie` — LFS; `*.json` (Lottie) — без LFS | частично | — |
| `sources/` | `*.penpot`, `*.fig` — LFS | да | DSN-008 |

Нужные шаблоны LFS (их добавляет REPO-001 в корневой `.gitattributes`, этот каталог `.gitattributes` не трогает):

```
design/**/*.png filter=lfs diff=lfs merge=lfs -text
design/**/*.jpg filter=lfs diff=lfs merge=lfs -text
design/**/*.webp filter=lfs diff=lfs merge=lfs -text
design/**/*.avif filter=lfs diff=lfs merge=lfs -text
design/**/*.psd filter=lfs diff=lfs merge=lfs -text
design/**/*.kra filter=lfs diff=lfs merge=lfs -text
design/**/*.blend filter=lfs diff=lfs merge=lfs -text
design/**/*.icon filter=lfs diff=lfs merge=lfs -text
design/**/*.riv filter=lfs diff=lfs merge=lfs -text
design/**/*.lottie filter=lfs diff=lfs merge=lfs -text
design/**/*.penpot filter=lfs diff=lfs merge=lfs -text
design/**/*.fig filter=lfs diff=lfs merge=lfs -text
design/**/*.ttf filter=lfs diff=lfs merge=lfs -text
design/**/*.otf filter=lfs diff=lfs merge=lfs -text
design/**/*.woff2 filter=lfs diff=lfs merge=lfs -text
```

`*.json` и `*.svg` в LFS класть **нельзя**: Style Dictionary, линтеры и проверка контраста должны читать их без `git lfs pull`.

## Токены

- Формат — [W3C Design Tokens Community Group (DTCG)](https://www.designtokens.org/): ключи `$value`, `$type`, `$description`; ссылки — `{group.token}`. Значения цвета, размера и длительности записаны объектами (`{"colorSpace": "srgb", "components": [...], "hex": "..."}`, `{"value": 4, "unit": "px"}`), как в стабильной версии спецификации 2025.10.
- Имена файлов — `*.tokens.json` (рекомендация DTCG).
- Три слоя: **primitive → semantic → component**. Код платформ ссылается только на semantic и component.
- Плотности (TV, desktop, tablet, phone) — отдельные файлы в `tokens/density/`, переопределяют масштаб, а не цвета.
- Значения перенесены из 01 §4 (DSN-002); правила слоёв, тем и плотностей, проверки — в [`tokens/README.md`](tokens/README.md). Код платформ — `scripts/tokens-build.mjs` (DSN-003): Dart (`pa_design`), Swift (`tokens/generated/apple/Tokens.swift`), Kotlin (`tokens/generated/android/PaTokens.kt`), CSS (`tokens/generated/web/tokens.css`); CI падает, если токен изменён без перегенерации. Сгенерированный код руками не правится.

## Как вносить изменения

1. Поменяйте файл в `design/` (токен, SVG, мастер-ассет), выполните `just design build` и `just design check`. Если правили в Penpot — экспортируйте результат в соответствующий каталог.
2. Для визуальных изменений приложите в PR скриншот до/после.
3. Новый шрифт, набор иконок или сторонний арт — сначала строка в [`LICENSES.md`](LICENSES.md), иначе PR не принимается.

## Что запрещено

- Шрифты Apple SF и SF Symbols в репозитории (на Apple-платформах используем системные, не копируя файлы).
- Любые ассеты, скриншоты, иконки, тексты и бренд Infuse/Firecore.
- Логотипы Dolby, DTS, HDR10+, Fedora и т. п. без лицензии; логотипы Plex, Jellyfin, Emby, Twitch, TMDB — только официальные и по их brand guidelines (DSN-007, LEGAL-009).
- Арт из TMDB/Fanart.tv в плейсхолдерах, маркетинге и сторах.
- Шрифты и иконки с несвободной или неясной лицензией.
