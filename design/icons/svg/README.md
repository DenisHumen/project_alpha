<!-- SPDX-License-Identifier: MPL-2.0 -->

# icons/svg

Исходные SVG иконок интерфейса (DSN-004). Набор — курированное подмножество **Lucide** (ISC) из `../manifest.json`, приведённое к нашему стилю. Почему Lucide — [`../../decisions/0002-icons-and-fonts.md`](../../decisions/0002-icons-and-fonts.md).

## Стиль (проверяет линтер)

- Сетка 24×24, `viewBox="0 0 24 24"`, без `width`/`height`.
- Штрих `stroke-width="1.75"` (допустимо 1.75–2), `stroke-linecap="round"`, `stroke-linejoin="round"`, `stroke="currentColor"`, `fill="none"`. Цвета — только `currentColor`/`none`.
- Только `path`, `circle`, `ellipse`, `rect`, `line`, `polyline`, `polygon`, `g`; без `style`, `class`, ссылок, растров и скриптов. Размер ≤ 4 КБ.
- Первая строка — SPDX-комментарий лицензии (`ISC` для иконок на основе Lucide, `MPL-2.0` для своих).
- Имя файла — `kebab-case.svg`, по смыслу, а не по форме (`seek-back`, а не `rotate-ccw`); из него генерируется `PaIcon.seekBack`.

## Как добавить или поменять иконку

1. Добавьте строку в `../manifest.json`: `"имя": { "lucide": "имя-в-lucide", "group": "..." }` (каталог — https://lucide.dev/icons).
2. `just design icons` — импорт из закреплённой версии `lucide-static`, нормализация штриха, svgo, линтер, генерация кода.
3. Своя иконка (не из Lucide): нарисуйте по сетке 24×24 в Penpot/Inkscape, положите SVG сюда с `<!-- SPDX-License-Identifier: MPL-2.0 -->`, добавьте в манифест с `"lucide": null` (импорт её не перезаписывает), затем `just design icons`.

## Что генерируется

| Платформа | Файл | Использование |
|---|---|---|
| Flutter | `design/flutter/pa_design/lib/src/icons.g.dart` | `PaIconView(PaIcon.play)` — размер и цвет из `IconTheme` |
| Apple (SwiftUI) | `design/icons/generated/apple/PaIcons.xcassets` + `PaIcon.swift` | `PaIcon.play.image()` — вектор, template rendering, тонируется `foregroundStyle` |

CI (`.github/workflows/design.yml`) падает, если SVG нарушает стиль или сгенерированные файлы устарели.

Не класть сюда: SF Symbols, иконки Infuse/Firecore, логотипы сервисов (те — отдельно по DSN-007).
