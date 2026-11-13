<!-- SPDX-License-Identifier: MPL-2.0 -->

# Лицензии дизайн-ассетов

Реестр всего стороннего в `design/`: шрифты, наборы иконок, арт, brand-ассеты сервисов. Каждый сторонний файл или набор должен иметь здесь строку **до** мержа. Собственные ассеты проекта распространяются под лицензией репозитория (MPL-2.0), если в строке не указано иное.

Допустимые лицензии: OFL-1.1 (шрифты), ISC, MIT, Apache-2.0, CC0-1.0, CC-BY-4.0 (с атрибуцией). Неясная или несвободная лицензия — не принимаем.

## Шрифты

| Шрифт | Где используется | Лицензия | Источник | Задача | Статус |
|---|---|---|---|---|---|
| Inter Variable 4.1 (`InterVariable.ttf`, `-Italic.ttf`, `.woff2`) | Не-Apple платформы: Flutter (пакет `pa_design`), веб | OFL-1.1, © 2016 The Inter Project Authors; текст лицензии — `Inter-LICENSE.txt`, извлекается рядом со шрифтом и поставляется с ним | https://github.com/rsms/inter/releases/tag/v4.1 | DSN-005 | используется; файлы не в git — `fonts/fonts.lock.json` (SHA-256) |
| Manrope / Golos | — | Manrope и Golos Text — OFL-1.1; лицензия Golos UI как OFL не подтверждена | — | DSN-005 | не нужны: Inter покрывает кириллицу (`decisions/0002-icons-and-fonts.md`) |
| SF Pro (системный) | Только Apple-платформы, **файлы в репо не кладём** | Apple, только для Apple-платформ | — | — | системный |

## Иконки

| Набор | Лицензия | Источник | Задача | Статус |
|---|---|---|---|---|
| Lucide 1.72.0 (`lucide-static`), 92 иконки, штрих изменён на 1.75 | ISC, © Lucide Icons and Contributors; производные от Feather — также MIT, © Cole Bemis. Текст — `icons/LICENSE-lucide.txt` | https://lucide.dev, https://github.com/lucide-icons/lucide | DSN-004 | используется: `icons/svg/`, сгенерированные `icons.g.dart`, `PaIcons.xcassets` |
| Phosphor | MIT | https://phosphoricons.com | DSN-004 | отклонён (`decisions/0002-icons-and-fonts.md`) |
| SF Symbols | Только Apple-платформы, **в репо не кладём** | — | — | запрещено в репо |

## Brand-ассеты сторонних сервисов

Только официальные файлы и строго по guidelines; ссылка на guidelines обязательна (DSN-007, LEGAL-009).

| Сервис | Файлы | Guidelines | Статус |
|---|---|---|---|
| — | — | — | пока нет |

## Прочий сторонний арт

| Файл / каталог | Автор | Лицензия | Источник | Атрибуция |
|---|---|---|---|---|
| — | — | — | — | — |

## Тексты лицензий

### Lucide (ISC) и Feather (MIT)

Полный текст — дословная копия из пакета: [`icons/LICENSE-lucide.txt`](icons/LICENSE-lucide.txt). Кратко: Lucide — ISC, © 2026 Lucide Icons and Contributors; иконки, производные от Feather, дополнительно MIT, © 2013-present Cole Bemis. У таких SVG заголовок `SPDX-License-Identifier: ISC AND MIT` (проставляет `scripts/icons-import.mjs`). Уведомление должно идти со сборками, которые содержат иконки (экран «Лицензии» в приложении).

### Inter (SIL Open Font License 1.1)

Copyright (c) 2016 The Inter Project Authors (https://github.com/rsms/inter). Полный текст OFL-1.1 — в `Inter-LICENSE.txt`, который `scripts/fonts-fetch.mjs` извлекает из официального архива релиза (SHA-256 закреплён), а также на https://openfontlicense.org. Шрифт не продаётся отдельно и не переименовывается; мы его не модифицируем.

