<!-- SPDX-License-Identifier: MPL-2.0 -->

# AGENTS.md — `clients/apple/` (Swift)

Общие правила — в корневом [AGENTS.md](../../AGENTS.md).

- **Что здесь:** Swift-пакеты и приложения: `AlphaPlayerKit/` (плеер, PLAY-APL), `TVApp/` (tvOS на SwiftUI, TV), `AudioKit/` (MUS), CarPlay. Ядро подключается как SwiftPM-пакет `PACore` (xcframework из `tools/bindings/`, CORE-003). Решения: ADR-0002, ADR-0003.
- **Нужна macOS** с Xcode (версия закрепляется в `.xcode-version`, REPO-008). На Linux эти цели не собираются — не пытайтесь, отметьте в PR как непроверенное.

## Сборка и тесты

```bash
# Rust-ядро в xcframework (после CORE-003)
tools/bindings/build-xcframework.sh

# Swift-пакет AlphaPlayerKit (PLAY-APL-001): медиа для тестов, XCTest на симуляторе, проверка LGPL
just apple player-fixtures && just apple player-qa-media
just apple player-test ios        # ios | tvos | macos
just apple player-test-linux      # без Mac: те же тесты с libmpv дистрибутива в Docker (не замена Apple CI)

# tvOS-приложение (TV-001): проект генерирует XcodeGen из project.yml (ADR-NNNN)
just apple tv-core       # tools/bindings/build-xcframework.sh tvos tvos-sim
just apple tv-test       # xcodegen generate + xcodebuild test (unit + UI)
```

- `TVApp.xcodeproj` не коммитим: правки проекта — только в `TVApp/project.yml`.
- В CI tvOS-приложение собирает и тестирует job `tvapp` в `.github/workflows/apple-core.yml` (macos-15); скриншот Home из симулятора — артефакт `tvos-home-screenshot`. Ручной запуск: `gh workflow run apple-core.yml --ref <ветка>`.
- Иконки — `PaIcon` и `PaIcons.xcassets` из `design/icons/generated/apple` (DSN-004); цвета — `TVApp/Sources/Theme.swift` (копия токенов до `Tokens.swift` из DSN-003).

## Правила области

- В каждом `.swift` первой строкой `// SPDX-License-Identifier: MPL-2.0`.
- MPVKit — только LGPL-вариант (не `-GPL`), FFmpeg — только LGPL-сборка.
- SF Pro и SF Symbols — только на Apple-платформах, в репозиторий не кладём.
- Подпись и сертификаты — только в CI-окружении `store` (см. [docs/dev/secrets.md](../../docs/dev/secrets.md)); локально — автоматическая подпись разработчика, без коммита профилей.
