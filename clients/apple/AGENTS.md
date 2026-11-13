<!-- SPDX-License-Identifier: MPL-2.0 -->

# AGENTS.md — `clients/apple/` (Swift)

Общие правила — в корневом [AGENTS.md](../../AGENTS.md).

- **Что здесь:** Swift-пакеты и приложения: `AlphaPlayerKit/` (плеер, PLAY-APL), `TVApp/` (tvOS на SwiftUI, TV), `AudioKit/` (MUS), CarPlay. Ядро подключается как SwiftPM-пакет `PACore` (xcframework из `tools/bindings/`, CORE-003). Решения: ADR-0002, ADR-0003.
- **Нужна macOS** с Xcode (версия закрепляется в `.xcode-version`, REPO-008). На Linux эти цели не собираются — не пытайтесь, отметьте в PR как непроверенное.

## Сборка и тесты

```bash
# Rust-ядро в xcframework (после CORE-003)
tools/bindings/build-xcframework.sh

# Swift-пакет
cd clients/apple/AlphaPlayerKit && swift build && swift test

# tvOS-приложение (после TV-001)
xcodebuild test -project clients/apple/TVApp/TVApp.xcodeproj -scheme TVApp \
  -destination 'platform=tvOS Simulator,name=Apple TV 4K (3rd generation)'
```

## Правила области

- В каждом `.swift` первой строкой `// SPDX-License-Identifier: MPL-2.0`.
- MPVKit — только LGPL-вариант (не `-GPL`), FFmpeg — только LGPL-сборка.
- SF Pro и SF Symbols — только на Apple-платформах, в репозиторий не кладём.
- Подпись и сертификаты — только в CI-окружении `store` (см. [docs/dev/secrets.md](../../docs/dev/secrets.md)); локально — автоматическая подпись разработчика, без коммита профилей.
