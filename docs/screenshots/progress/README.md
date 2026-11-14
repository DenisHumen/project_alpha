<!-- SPDX-License-Identifier: MPL-2.0 -->
# Kinolune: прогресс в картинках (2026-11-13)

Снимки сделаны с того, что уже есть в репозитории, без правок кода. Видео — синтетический тестовый ролик
(ffmpeg `smptehdbars` + `testsrc2` 1080p30 H.264 с титром «Kinolune»), стороннего контента нет.

| Файл | Что на снимке |
|---|---|
| [`01-desktop-texture-osd.png`](01-desktop-texture-osd.png) | Десктоп-спайк PLAY-DSK-001 (Flutter Linux + libmpv), режим `texture`: видео в текстуре Flutter, поверх — красная плашка Flutter OSD (headless sway в Docker, VAAPI). |
| [`10-androidtv-launcher.png`](10-androidtv-launcher.png) | Android-спайк PLAY-AND-001 на эмуляторе Android TV 14: главный экран с вариантами A–D, AFR, tunneling. |
| [`11-androidtv-variant-b-flutter-osd.png`](11-androidtv-variant-b-flutter-osd.png) | Вариант B (рекомендованный): нативный Media3-плеер на `SurfaceView` + Flutter OSD во втором движке — панель статистики и кнопки поверх видео (эмулятор, программный декодер, кадр в конце ролика). |
| [`20-arch-context.png`](20-arch-context.png) | Архитектура §2: контекст — что где работает (из `docs/dev/architecture.md`). |
| [`21-arch-components.png`](21-arch-components.png) | Архитектура §3: компоненты — клиенты, нативные плееры, привязки ядра, сервер, NAS-ОС. |
| [`22-arch-dataflow.png`](22-arch-dataflow.png) | Архитектура §6: поток данных «скан → распознавание → медиатека → воспроизведение». |
| [`30-recognition-eval-report.png`](30-recognition-eval-report.png) | Отчёт `pa-eval` (бейзлайн `naive`) на 54 примерах `tools/recognition-eval/examples`: точность разбора имён по полям; идентификация 0 %, т. к. кассеты ответов провайдеров ещё нет. |
| [`40-nas-os-login.png`](40-nas-os-login.png) | NAS-ОС (`distro/`, bootc): образ собран `build-image.sh`, диск — `make-disk.sh`, загружен в QEMU/KVM; VGA-консоль с приглашением входа. |
| [`41-nas-os-bootc-status.png`](41-nas-os-bootc-status.png) | Та же VM, вход на консоли: `bootc status` (загружен наш образ), сервисы `alpha-*` (заглушки), Samba, wsdd, Avahi, firewalld активны, `/health` отвечает, SELinux `Enforcing`, зона `home`. |

## Как получено

- **Десктоп**: `CLIPS='kinolune*' MODES=texture BACKENDS=wayland spikes/desktop-player/tools/run-in-docker.sh`, кадр `grim` обрезан до окна 1280×720.
- **Android TV**: `flutter build apk --debug`, AVD `android-tv` API 34 (2 ядра, 2 GB), `adb exec-out screencap -p`.
  Замечания: с `-gpu swiftshader_indirect` главный Flutter-экран снимается чёрным (известный артефакт, см. README спайка),
  поэтому он снят с `-gpu guest`; в этом режиме вариант B на эмуляторе получил ANR при старте второго Flutter-движка
  (известная проблема блокировки главного потока, см. ADR-0008), поэтому снимок B — из прогона со swiftshader.
  Первый кадр в B на эмуляторе — ~48 с (программный декодер, ARM-трансляция), это не характеристика железа.
- **Архитектура**: mermaid-cli 11, тема `dark`.
- **Распознавание**: `cargo run -p pa-eval -- run --recognizer naive --json report.json --md report.md tools/recognition-eval/examples/examples.jsonl`,
  затем `compare` с бейзлайном `abstain` — «регресса нет». Markdown-отчёт отрисован в headless Chrome (разделы «по категориям» и пустая калибровка опущены).
- **NAS-ОС**: `build-image.sh` → локальный реестр → `make-disk.sh` (~5 мин), QEMU 2 vCPU / 3 GiB, `-vga std`;
  снимки через `screendump` монитора QEMU, команды набраны на консоли через `sendkey` (одноразовый пароль root только в этой VM, VM удалена).
- **PLAY-007 (субтитры)**, `play-007/`: debug-APK x86_64 с нашим libmpv на эмуляторе Android 14 (AVD без Google APIs).
  Фильм — `testsrc2` из ffmpeg с sidecar-файлами `*.en.srt` и `*.ru.forced.srt`; локальная папка `/sdcard/Movies` и WebDAV
  (nginx в Docker на сервере сборки, эмулятор ходит через `10.0.2.2`; файлы играют через loopback-прокси).
  Системный стиль задан через `adb shell settings put secure accessibility_captioning_*` (жёлтый моноширинный текст на синем,
  масштаб 1,5); `android-standard-style.png` — субтитры при выключенных системных субтитрах.
