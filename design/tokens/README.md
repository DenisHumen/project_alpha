<!-- SPDX-License-Identifier: MPL-2.0 -->

# tokens/ — дизайн-токены (W3C DTCG)

Источник правды для цвета, типографики, отступов, радиусов, материалов, фокуса и движения всех клиентов Kinolune (DSN-002). Формат — [Design Tokens Format Module 2025.10](https://www.designtokens.org/): `$value`, `$type`, `$description`, ссылки `{group.token}`, цвета — объекты `{colorSpace, components, alpha?, hex}`, размеры — `{value, unit}`.

Визуальное направление — tvOS / HIG: тёмная тема по умолчанию, глубина через фокус-масштаб, тень и материалы, крупные отступы на TV. Значения перенесены из `docs/research/01-product-ux-design.md` §4.2–4.7; отступления от исследования помечены в `$description` конкретного токена.

## Файлы и слои

| Файл | Слой | Что внутри |
|---|---|---|
| `primitive.tokens.json` | primitive | Палитра (gray, white/black-alpha, blue — временный акцент, red — LIVE, green, amber), отступы 0–100 (база 4 pt), радиусы, размеры, blur, шрифт, веса, длительности, кривые, коэффициенты. **Из кода платформ не используются.** |
| `semantic.tokens.json` | semantic, тёмная тема | `bg/*`, `text/*`, `icon/*`, `border/*`, `accent/default|pressed|subtle|live`, `status/*`, `state/*`, `focus/*` (fill, label, ring, scale.poster/card/button/hover/press, shadow, specular, tilt, parallax, idle-dim), `badge/*`, `scrim/*`, `glass/thin|regular|thick` (tint, blur, saturation, highlight, fallback), `motion/*` |
| `themes/light.tokens.json` | переопределение | Светлая тема (телефон, планшет, десктоп; TV всегда тёмный). Переопределяет **только существующие** semantic-токены |
| `density/{tv,desktop,tablet,phone}.tokens.json` | плотность | `type/*` (display, title1, title2, headline, subtitle, body, callout, caption, caption2, timecode), `layout/*` (поля, safe area TV 60/80, отступы, ширины постеров и карточек, колонки), `shape/*` (радиусы карточек/панелей/кнопок), `icon-size/*`, `control/*`. **Без цветов**; набор токенов во всех профилях одинаковый |
| `component.tokens.json` | component | `poster`, `card`, `button`, `media-badge`, `osd`, `dialog` — привязки к ролям |
| `contrast.pairs.json` | — | Пары «текст/фон» для проверки контраста (не токены) |

Ссылки идут только вниз: component → semantic/density → primitive. Код платформ берёт semantic, density и component.

### Типографика (pt; размер/интерлиньяж)

| Токен | TV | Desktop | Tablet / Phone |
|---|---|---|---|
| display | 76/96 semibold | 40/48 bold | 34/41 bold |
| title1 | 57/66 semibold | 28/34 bold | 28/34 bold |
| title2 | 48/56 medium | 22/28 semibold | 22/28 semibold |
| headline | 38/46 medium | 15/20 semibold | 17/22 semibold |
| subtitle | 38/46 regular | 15/20 regular | 17/22 regular |
| body | 29/36 medium | 13/18 regular | 17/22 regular |
| callout | 31/38 medium | 13/18 medium | 16/21 medium |
| caption | 25/32 medium | 11/14 regular | 13/18 regular |
| caption2 | 23/30 medium, tnum | 10/13 medium, tnum | 11/13 medium, tnum |
| timecode | = caption, tnum | = caption, tnum | = caption, tnum |

TV-размеры — по tvOS HIG (минимум 23, тело 29). Табличные цифры — `$extensions["org.project-alpha"].fontFeatures = ["tnum"]`. В DTCG `lineHeight` — множитель, поэтому интерлиньяж записан как отношение.

## Проверки

```bash
just design setup      # один раз: npm ci в design/
just design check      # всё, что проверяет CI
just design contrast   # таблица контраста всех пар
```

`scripts/validate-tokens.mjs` (без зависимостей) проверяет:

1. структуру DTCG: зарезервированные ключи, токен не содержит детей, у каждого токена есть `$type` (свой или группы), значения соответствуют типу (цвет: srgb, компоненты 0–1, `hex` совпадает с компонентами; размеры px/rem; длительности ms/s; составные типы — по полям);
2. ссылки: существуют, совместимы по типу (в том числе внутри shadow/typography/transition/gradient), без циклов — для **каждой** комбинации тема × плотность (2 × 4);
3. правила слоёв: темы только переопределяют, профили плотности одинаковы по набору и без цветов, один путь — в одном слое;
4. контраст WCAG 2.x для всех пар из `contrast.pairs.json` в обеих темах: текст ≥ 4.5:1, неактивный текст и нетекстовые элементы (кольцо фокуса, контур бейджа, прогресс) ≥ 3:1. Полупрозрачные фоны (стекло, scrim, подложка бейджа) проверяются поверх худших случаев арта — чёрного и белого.

Сейчас: 287 токенов, 66 проверок контраста, минимум 3.31:1 (неактивный текст в светлой теме), минимум для обычного текста — 4.76:1 (бейдж поверх белого арта).

## Экспорт в код

- **Flutter (релиз 0.1):** `scripts/tokens-dart.mjs` → `design/flutter/pa_design/lib/src/tokens.g.dart`: `PaTokens` (константы), `PaColorScheme.dark|light`, `PaDensity.tv|desktop|tablet|phone`, типы Flutter (`Color`, `TextStyle` с Inter и `opsz`, `BoxShadow`, `LinearGradient`, `Cubic`, `Duration`). Это промежуточный генератор; пайплайн Style Dictionary для Swift/CSS и CI-гейт — DSN-003 (API Dart-классов стоит сохранить).
- Сгенерированное руками не правится: поменяйте JSON и выполните `just design tokens`.

## Открытые вопросы

- Акцент `color.blue.*` — временный; финальный цвет бренда Kinolune — DSN-006. Замена рампы перекрашивает всё; контраст проверит CI.
- Коэффициенты фокуса, тени и idle-dim — наше предложение (Apple их не публикует), подбираются на реальном TV.
- Поддержка формата 2025.10 (объектные цвета/размеры) в выбранной версии Style Dictionary — проверить в DSN-003.
