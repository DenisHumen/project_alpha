<!-- SPDX-License-Identifier: MPL-2.0 -->

# Подготовка машины разработчика (REPO-008)

Новый ПК поднимается одной командой из корня репозитория:

```bash
just bootstrap          # ставит закреплённые тулчейны в домашний каталог и проверяет машину
just bootstrap check    # только проверка: без sudo, без загрузок, ничего не меняет
just bootstrap run --skip android,flutter   # Windows: -Skip android,flutter
```

Скрипт **никогда не вызывает sudo** и не повышает права. Если не хватает системных пакетов,
в конце печатается готовая команда (`apt`, `dnf`, `pacman`, `brew` или `winget`) — выполните её
сами и запустите `just bootstrap` ещё раз. `just` нужен заранее: `cargo install just` или пакет ОС.

## Где закреплены версии

| Файл | Что закрепляет |
|---|---|
| `rust-toolchain.toml` | Rust (ставит rustup) |
| `mise.toml` | Flutter (вместе с Dart), JDK, Go, Node, `just`, `cargo-ndk`, `cargo-nextest` (ставит [mise](https://mise.jdx.dev)) |
| `.xcode-version` | Xcode для Apple-таргетов (проверяется только на macOS; ставить через `xcodes`) |
| `tools/bootstrap/versions.env` | mise, Android cmdline-tools (с SHA-256), пакеты Android SDK (platform, build-tools, NDK), минимальные версии Python и git |

Обновление версии — правка одного из этих файлов в отдельном PR, затем `just bootstrap` и
пересборка Dev Container.

## Что делает `just bootstrap`

| Шаг | Linux | macOS | Windows |
|---|---|---|---|
| Системные пакеты (git, git-lfs, компилятор C, cmake, ninja, Python ≥ 3.10; на Linux — GTK 3 и libmpv для Flutter-десктопа) | проверка + команда `apt`/`dnf`/`pacman` | проверка + `brew` | проверка + `winget`, MSVC Build Tools |
| Rust из `rust-toolchain.toml` | `rustup toolchain install` | так же | так же |
| mise | ставится в `~/.local/bin` (с проверкой SHA-256) | команда `brew install mise` | команда `winget` |
| `mise install` по `mise.toml` | да | да | да |
| Android SDK | cmdline-tools в `$ANDROID_HOME` (по умолчанию `~/Android/Sdk`), затем `sdkmanager --install` | только проверка и подсказки | только проверка и подсказки |
| Xcode по `.xcode-version` | — | проверка | — |

**Лицензии Android SDK** скрипт не принимает за вас: если они не приняты, он попросит
выполнить `sdkmanager --licenses` и прочитать их самому, после чего повторный
`just bootstrap` поставит пакеты.

После установки включите mise в своей оболочке (`eval "$(mise activate bash)"` или аналог
для zsh, fish, PowerShell), чтобы `flutter`, `go`, `java` брались из закреплённых версий.

## Dev Container

`.devcontainer/` — окружение для Linux-части: Rust, Android cmdline-tools и NDK, cargo-ndk,
Flutter Linux (GTK, libmpv), Go, Node, podman. Версии берутся из тех же файлов, что и у
`just bootstrap`. Apple-сборки возможны только на macOS.

- VS Code: «Dev Containers: Reopen in Container». CLI: `devcontainer up --workspace-folder .`.
- Ручная сборка образа: `docker build -f .devcontainer/Dockerfile -t pa-dev .`
- При первом старте выполняется `just bootstrap`. Пакеты Android SDK ставятся после того, как
  вы приняли лицензии (`sdkmanager --licenses` внутри контейнера). Чтобы запечь их в образ,
  соберите его с `ACCEPT_ANDROID_SDK_LICENSES=true` (в `devcontainer.json` или `--build-arg`) —
  этим вы подтверждаете, что прочитали и принимаете лицензии.
- podman внутри контейнера требует дополнительных прав (`--privileged` или `--device /dev/fuse`)
  и нужен только для задач DIST.
