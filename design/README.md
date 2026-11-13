<!-- SPDX-License-Identifier: MPL-2.0 -->

# design/ — дизайн-система project_alpha

Здесь лежат **исходники дизайна**, из которых генерируется код и ассеты всех платформ: токены, иконки, бренд, иллюстрации, анимации, плейсхолдеры. Каталог принадлежит workstream DSN (см. `docs/ROADMAP.md` §0.4).

**Главное правило:** источник правды — файлы в git. Инструмент макетов (рекомендуется Penpot) — только редактор. Почему так — в [`decisions/0001-design-tool.md`](decisions/0001-design-tool.md).

## Структура

```
design/
├── README.md              # этот файл
├── LICENSES.md            # лицензии шрифтов, иконок и стороннего арта
├── decisions/             # ADR дизайн-workstream (инструмент, правила)
├── tokens/                # W3C DTCG JSON — источник правды для Style Dictionary
│   ├── primitive.tokens.json   # сырые значения: палитра, отступы, радиусы, тени
│   ├── semantic.tokens.json    # роли: bg/base, text/*, accent, focus/* … (ссылки на primitive)
│   ├── component.tokens.json   # компонентные: poster/focus/scale и т. п.
│   └── density/                # профили плотности: tv, desktop, tablet, phone
├── icons/svg/             # исходные SVG 24×24, штрих 1.75–2 px (DSN-004)
├── brand/                 # логотип, мастер иконки приложения 1024+, слои tvOS/Liquid Glass (DSN-006)
├── illustrations/         # пустые состояния, онбординг (SVG, при необходимости PNG)
├── motion/                # .riv (Rive) и .json (Lottie)
├── placeholders/          # собственные плейсхолдеры постеров и обложек (не TMDB-арт!)
└── sources/               # экспорт мастер-файлов макетов (.penpot / .fig) и ссылки на облачные проекты
```

| Каталог | Формат | Git LFS | Задача |
|---|---|---|---|
| `tokens/` | `*.tokens.json` (DTCG) | нет — нужны диффы и чтение в CI | DSN-002, DSN-003 |
| `icons/svg/` | `*.svg` | нет — текст, ревью построчно | DSN-004 |
| `brand/`, `illustrations/`, `placeholders/` | `*.svg` — без LFS; `*.png`, `*.psd`, `*.kra`, `*.blend`, `*.icon` — LFS | частично | DSN-006 |
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

- Формат — [W3C Design Tokens Community Group (DTCG)](https://www.designtokens.org/): ключи `$value`, `$type`, `$description`; ссылки — `{group.token}`. Значения цвета, размера и длительности записаны объектами (`{"colorSpace": "srgb", "components": [...], "hex": "..."}`, `{"value": 4, "unit": "px"}`), как в стабильной версии спецификации 2025.10 **[проверить поддержку в выбранной версии Style Dictionary — DSN-003]**.
- Имена файлов — `*.tokens.json` (рекомендация DTCG).
- Три слоя: **primitive → semantic → component**. Код платформ ссылается только на semantic и component.
- Плотности (TV, desktop, tablet, phone) — отдельные файлы в `tokens/density/`, переопределяют масштаб, а не цвета.
- Значения в текущих файлах — **каркас** (DSN-001). Полный перенос значений из 01 §4 — задача DSN-002; генерация `tokens.dart`, `Tokens.swift`, `tokens.css` и контраст-линт — DSN-003. Сгенерированный код руками не правится.

## Как вносить изменения

1. Поменяйте файл в `design/` (токен, SVG, мастер-ассет). Если правили в Penpot/Figma — экспортируйте результат в соответствующий каталог.
2. Для визуальных изменений приложите в PR скриншот до/после.
3. Новый шрифт, набор иконок или сторонний арт — сначала строка в [`LICENSES.md`](LICENSES.md), иначе PR не принимается.

## Что запрещено

- Шрифты Apple SF и SF Symbols в репозитории (на Apple-платформах используем системные, не копируя файлы).
- Любые ассеты, скриншоты, иконки, тексты и бренд Infuse/Firecore.
- Логотипы Dolby, DTS, HDR10+, Fedora и т. п. без лицензии; логотипы Plex, Jellyfin, Emby, Twitch, TMDB — только официальные и по их brand guidelines (DSN-007, LEGAL-009).
- Арт из TMDB/Fanart.tv в плейсхолдерах, маркетинге и сторах.
- Шрифты и иконки с несвободной или неясной лицензией.
