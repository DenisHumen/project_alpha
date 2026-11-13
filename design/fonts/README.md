<!-- SPDX-License-Identifier: MPL-2.0 -->

# fonts/ — шрифты (DSN-005)

| Платформа | Шрифт |
|---|---|
| Android, Android TV, Windows, Linux (Flutter), веб-админки и сайт | **Inter Variable 4.1** (OFL-1.1), оси `wght` 100–900 и `opsz` 14–32 |
| iOS, iPadOS, macOS, tvOS (SwiftUI) | Системный SF — файлы SF в репозиторий не кладём и вне Apple-платформ не используем |

Почему Inter и что проверено — [`../decisions/0002-icons-and-fonts.md`](../decisions/0002-icons-and-fonts.md).

## Файлы шрифтов не коммитятся

`fonts.lock.json` закрепляет релиз Inter (URL архива и SHA-256 архива и каждого извлекаемого файла). Скрипт скачивает и проверяет:

```bash
just design fonts                         # → design/fonts/files/ и design/flutter/pa_design/fonts/
just design fonts --dest some/dir         # дополнительно скопировать в каталог
just design fonts --archive Inter-4.1.zip # без сети, из скачанного архива
```

Так в git нет бинарников (не нужен Git LFS), а сборка воспроизводима: подмена файла роняет проверку хеша. Текст OFL (`Inter-LICENSE.txt`) извлекается рядом и должен поставляться вместе со шрифтом.

**Для сборки Flutter-приложения** шаг `just design fonts` (или `node design/scripts/fonts-fetch.mjs`) обязателен до `flutter build`: пакет `pa_design` объявляет шрифт `Inter` в своём `pubspec.yaml`. В CI релиза — отдельный шаг перед сборкой.

## Использование

- **Flutter:** токены `PaDensity.<плотность>.type*` уже содержат `fontFamily: 'Inter'`, `package: 'pa_design'`, `opsz` = размер (в пределах 14–32) и `tnum` для `caption2`/`timecode`. Отдельно подключать ничего не нужно, кроме зависимости на пакет.
- **Веб:** `InterVariable.woff2` из `files/`, `@font-face { font-family: Inter; font-weight: 100 900; }`, для цифр — `font-feature-settings: 'tnum'`.
- Масштабирование текста ОС (до 200 %) не отключать: стили задают базовый размер, масштаб применяет платформа.

## Образец типографики

`specimen.html` — образец ru/en для всех четырёх плотностей, генерируется из токенов (`npm run fonts:specimen`). Откройте в браузере после `just design fonts`. Утверждение образца — решение владельца (done when DSN-005).
