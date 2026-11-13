<!-- SPDX-License-Identifier: MPL-2.0 -->

# LGPL-сборка FFmpeg (PLAY-001)

Скрипты собирают FFmpeg как **динамические библиотеки под LGPL v2.1+** для всех
клиентских платформ. Обоснование — [`docs/research/02-client-stack-playback.md`](../../docs/research/02-client-stack-playback.md) §3.1.
GPL- и nonfree-код (x264, x265, fdk-aac, `--enable-gpl`, `--enable-nonfree`) не
включается никогда, (L)GPLv3 (`--enable-version3`) — тоже.

## Что внутри

| Файл | Назначение |
|---|---|
| `versions.sh` | Закреплённые версии, URL и SHA-256 всех исходников |
| `build.sh` | Точка входа: `list`, `fetch`, `<target>`, `sources` |
| `lib/targets.sh` | Тулчейны и флаги по таргетам (hwaccel, шрифтовой провайдер libass) |
| `lib/deps.sh` | Статические PIC-сборки зависимостей: dav1d, libass, fribidi, harfbuzz, freetype, zlib (только Windows) |
| `lib/ffmpeg.sh` | **Единственное место** с флагами `configure` FFmpeg и списком запрещённых опций |
| `check-license.sh`, `check_license.c` | Лицензионная проверка: `config.h`, строки в бинарниках, вызов `avutil_license()` |
| `docker/Dockerfile`, `docker-build.sh` | Сборочный образ Debian (Linux, Windows через mingw-w64, Android с NDK) |
| `patches/<пакет>/*.patch` | Наши патчи к исходникам (применяются по алфавиту) |

## Конфигурация FFmpeg

Общие флаги (`lib/ffmpeg.sh`):

```
--disable-gpl --disable-nonfree --disable-version3
--enable-shared --disable-static --enable-pic
--disable-programs --disable-doc --disable-debug --disable-autodetect
--enable-zlib --enable-libdav1d --enable-libass
--disable-encoders --enable-encoder=aac,ac3,png,mjpeg
--disable-indevs --disable-outdevs
```

- `--disable-autodetect`: в сборку попадает только то, что включено явно, а не всё, что нашлось на машине сборщика.
- Энкодеры: `aac` и `ac3` — перекодирование аудио; `png` и `mjpeg` — скриншоты и trickplay-превью.
- Сеть FFmpeg включена, TLS — нет: HTTPS-источники идут через наш VFS/прокси (NET-008).

Аппаратное декодирование по таргетам (`lib/targets.sh`):

| Таргет | hwaccel | Шрифты libass |
|---|---|---|
| `linux-x86_64` | VA-API + libdrm (системные `libva`, `libdrm`, MIT) | без системного провайдера |
| `windows-x86_64` | D3D11VA, D3D12VA, DXVA2 | DirectWrite |
| `android-*` (`arm64-v8a`, `armeabi-v7a`, `x86_64`) | MediaCodec (JNI) | без системного провайдера |
| `macos-*`, `ios-*`, `tvos-*` | VideoToolbox, AudioToolbox | CoreText |

Зависимости собираются статически и вшиваются в `libav*`, наружу экспортируются только
символы FFmpeg. Лицензии: dav1d — BSD-2, libass — ISC, fribidi — LGPL-2.1+, harfbuzz — MIT,
freetype — FTL, zlib — Zlib; все совместимы с LGPL-2.1+.

## Как собрать

Linux x86_64 и Windows x86_64 — в Docker (на хосте нужен только Docker):

```bash
PA_FF_JOBS=4 tools/ffmpeg/docker-build.sh linux-x86_64
PA_FF_JOBS=4 tools/ffmpeg/docker-build.sh windows-x86_64
ANDROID_NDK_VERSION=r28c tools/ffmpeg/docker-build.sh android-arm64-v8a
```

Без Docker (нужны meson, ninja, nasm, pkg-config, компилятор C/C++):

```bash
tools/ffmpeg/build.sh linux-x86_64
ANDROID_NDK_HOME=/path/to/ndk tools/ffmpeg/build.sh android-arm64-v8a
tools/ffmpeg/build.sh ios-arm64          # только на macOS с Xcode
```

Результат — `tools/ffmpeg/out/<target>/`: `include/`, `lib/` (или `bin/*.dll` и импорт-библиотеки
на Windows), `share/licenses/` с текстами лицензий и `BUILDINFO.txt` (версии, SHA-256, патчи,
полная строка `configure`). Каталоги `.cache/`, `build/`, `out/` в git не попадают.

Переменные окружения: `PA_FF_JOBS`, `PA_FF_CACHE`, `PA_FF_WORK`, `PA_FF_OUT`,
`PA_FF_EXTRA_FLAGS` (дополнительные флаги тоже проходят лицензионную проверку),
`ANDROID_NDK_HOME`, `PA_FF_ANDROID_API` (по умолчанию 24), `PA_FF_MACOS_MIN`,
`PA_FF_IOS_MIN`, `PA_FF_TVOS_MIN`.

Apple: на выходе — dylib с install name `@rpath/…` для каждого среза. Упаковка в
XCFramework делается в задачах PLAY-APL.

## Лицензионная проверка

Сборка падает, если:

1. в строке `configure` есть `--enable-gpl`, `--enable-nonfree`, `--enable-version3`
   или GPL/nonfree-библиотека (x264, x265, fdk-aac, vidstab, rubberband и т. д.) — проверка
   до запуска `configure` (`assert_lgpl_flags`) и по `config.h` после;
2. в `config.h` `CONFIG_GPL`, `CONFIG_NONFREE` или `CONFIG_VERSION3` не равны 0, либо
   `FFMPEG_LICENSE` не равна `LGPL version 2.1 or later`;
3. какая-либо `libav*`/`libsw*` не содержит строку `license: LGPL version 2.1 or later`
   (статическая проверка — работает и для кросс-таргетов);
4. для нативного таргета — `check_license.c`, слинкованный с артефактом, получает от
   `avutil_license()`, `avcodec_license()` и остальных что-то кроме `LGPL version 2.1 or later`.

Отдельно: `tools/ffmpeg/check-license.sh --artifact <каталог>` или `--run <каталог>`.

## Исходники для соответствия LGPL

`tools/ffmpeg/build.sh sources` собирает `out/project_alpha-ffmpeg-sources-<версия>.tar.xz`:
оригинальные архивы всех компонентов с `SHA256SUMS`, наши патчи и эти скрипты — всё, что нужно,
чтобы пересобрать библиотеки и перелинковать приложение. Архив прикладывается к каждому
релизу (CI-011).

## Статус проверки

Все таргеты собираются в CI — [`.github/workflows/ffmpeg.yml`](../../.github/workflows/ffmpeg.yml)
(на PR и push в `main`, только при изменениях в `tools/ffmpeg/`, плюс ручной запуск):

| Job | Где | Проверки |
|---|---|---|
| `linux-x86_64`, `windows-x86_64` | `ubuntu-24.04`, Docker-образ | статическая; для Linux ещё запуск `check_license` |
| `android-arm64-v8a`, `android-armeabi-v7a`, `android-x86_64` | `ubuntu-24.04`, NDK r28c через `sdkmanager` | статическая |
| `macos-arm64`, `macos-x86_64`, `ios-arm64`, `ios-sim-arm64`, `tvos-arm64`, `tvos-sim-arm64` | `macos-15`, Xcode раннера | статическая; для `macos-arm64` ещё запуск `check_license` |
| `Sources and GPL canary` | `ubuntu-24.04` | `--enable-gpl`, `--enable-nonfree`, `--enable-version3`, `--enable-libx264` роняют сборку; GPL-`config.h` не проходит `--config` |

- Исходники кэшируются по хэшу `versions.sh`, результаты — по хэшу рецепта (`*.sh`, `*.c`, `lib/`,
  `patches/`, `docker/`) и тулчейна (версия NDK или Xcode). При попадании в кэш сборка пропускается,
  а лицензионная проверка всё равно выполняется.
- Артефакты `ffmpeg-<target>` хранятся 7 дней. Раздачу готового FFmpeg другим workflow делает CI-008.
- Windows DLL на реальной Windows не запускались, библиотеки для Android, iOS и tvOS — на устройствах;
  это делают задачи интеграции плеера (PLAY-AND, PLAY-APL).

## Обновление версий

Поменять версию, URL и SHA-256 в `versions.sh` одним коммитом, пересобрать хотя бы
`linux-x86_64`, проверить, что патчи применяются.
