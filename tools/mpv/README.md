<!-- SPDX-License-Identifier: MPL-2.0 -->

# LGPL-сборка libmpv (PLAY-002)

Скрипты собирают **libmpv под LGPL v2.1+**: mpv с `-Dgpl=false`, libplacebo и libass,
динамически слинкованный с нашей LGPL-сборкой FFmpeg из [`tools/ffmpeg`](../ffmpeg/README.md).
Обоснование — [`docs/research/02-client-stack-playback.md`](../../docs/research/02-client-stack-playback.md)
§2.2, §3.1 и [ADR-0003](../../docs/adr/0003-native-player-surfaces-engines.md); первый потребитель —
media_kit в релизе 0.1 ([ADR-0012](../../docs/adr/0012-release-0-1-player-media-kit.md)).

Зачем своя сборка: готовые libmpv из пакетов media_kit и дистрибутивов не подходят.
`media_kit_libs_android_video` 1.3.8 скачивает `libmpv.so` со статически вшитым FFmpeg под
**LGPL v3** (`--enable-version3`), а системная libmpv в Debian/Ubuntu собрана как **GPL**.
Наша сборка — LGPL v2.1+, FFmpeg — отдельными `.so`, исходники и рецепт открыты.

## Что внутри

| Файл | Назначение |
|---|---|
| `versions.sh` | Закреплённые версии, URL и SHA-256: mpv, libplacebo, сабмодули libplacebo (glad, jinja2, markupsafe, fast_float, Vulkan-Headers), JNI-хелпер media_kit |
| `build.sh` | Точка входа: `list`, `fetch`, `<target>`, `sources` |
| `lib/mpv.sh` | **Единственное место** с опциями meson для mpv и libplacebo и списком запрещённых (GPL) возможностей |
| `check-license.sh`, `check_libmpv.c` | Лицензионная проверка: `config.h`, строки в бинарниках, вызов `mpv_client_api_version()` |
| `bundle.sh` | Хук для упаковки: кладёт libmpv туда, откуда её берёт media_kit (Linux-бандл, Android `jniLibs`) |
| `docker/Dockerfile`, `docker-build.sh` | Сборочный образ Ubuntu 24.04 (Linux, Windows через mingw-w64, Android с NDK) |
| `patches/<пакет>/*.patch` | Наши патчи к исходникам (применяются по алфавиту) |

FFmpeg и его зависимости (dav1d, libass, fribidi, harfbuzz, freetype, zlib) собираются
рецептом `tools/ffmpeg` без изменений: те же версии, флаги и лицензионный гейт. `build.sh`
подключает его скрипты и собирает FFmpeg в тот же каталог, что и libmpv.

## Конфигурация

Общие опции mpv (`lib/mpv.sh`):

```
-Dgpl=false -Dauto_features=disabled
-Dlibmpv=true -Dcplayer=false -Dbuild-date=false
-Dlua=disabled -Djavascript=disabled -Dcplugins=disabled -Dlibavdevice=disabled
-Dzlib=enabled -Dvector=enabled -Dgl=enabled -Dplain-gl=enabled
```

- `-Dauto_features=disabled` — аналог `--disable-autodetect` у FFmpeg: в сборку попадает только
  включённое явно.
- Без Lua, JavaScript и C-плагинов: скрипты mpv клиентам не нужны, управление идёт через client API.
- Вывод — render API OpenGL (через него рисует media_kit и Texture-путь ADR-0009).

По таргетам:

| Таргет | Видео | Аудио | hwdec |
|---|---|---|---|
| `linux-x86_64` | EGL + render API | PulseAudio, ALSA | VA-API (libva, libdrm) |
| `android-*` (`arm64-v8a`, `armeabi-v7a`, `x86_64`) | EGL (Android Surface) | AudioTrack, AAudio, OpenSL ES | MediaCodec |
| `windows-x86_64` | WGL + render API | WASAPI | D3D11VA, DXVA2 |
| `macos-arm64`, `macos-x86_64` | render API | CoreAudio | VideoToolbox (copy) |

libplacebo собирается статически и вшивается в libmpv: только OpenGL, без Vulkan, D3D11 и
shaderc/glslang; Dolby Vision (reshaping RPU) — встроенной реализацией libplacebo (`-Ddovi`).

GPL-части mpv (`x11`, `dvdnav`, `cdda`, `dvbin`, `jack`, `oss-audio`, `caca`, `direct3d`,
`rubberband`) при `-Dgpl=false` выключены. Поэтому на Linux **нет X11-окна mpv**: libmpv
работает только через render API (media_kit, ADR-0009). Собственное окно для HDR
(PLAY-DSK-003) потребует Wayland (MIT) и Vulkan — это отдельная задача.

Лицензии компонентов: mpv, libplacebo, FFmpeg, fribidi — LGPL-2.1+; libass — ISC; dav1d — BSD-2;
harfbuzz — MIT; freetype — FTL; fast_float — MIT/Apache-2.0/BSL-1.0; Vulkan-Headers — Apache-2.0 или MIT (только объявления для заглушек Vulkan); glad — MIT (генератор),
сгенерированный загрузчик — CC0/Apache-2.0; JNI-хелпер media_kit — MIT. jinja2 и markupsafe
нужны только во время сборки. Системные библиотеки Linux (libva, libdrm, libEGL, libpulse,
libasound, zlib) линкуются динамически и не входят в артефакт.

## Как собрать

Linux x86_64 и Windows x86_64 — в Docker (на хосте нужен только Docker):

```bash
PA_FF_JOBS=4 tools/mpv/docker-build.sh linux-x86_64
PA_FF_JOBS=4 tools/mpv/docker-build.sh windows-x86_64
ANDROID_NDK_VERSION=r28c tools/mpv/docker-build.sh android-arm64-v8a
```

Без Docker (нужны meson ≥ 1.3, ninja, nasm, pkg-config, python3, компилятор C/C++; на Linux ещё
`libva-dev libdrm-dev libegl-dev libpulse-dev libasound2-dev zlib1g-dev`):

```bash
tools/mpv/build.sh linux-x86_64
ANDROID_NDK_HOME=/path/to/ndk tools/mpv/build.sh android-arm64-v8a
tools/mpv/build.sh macos-arm64          # только на macOS с Xcode
```

Результат — `tools/mpv/out/<target>/`: `lib/` (`libmpv.so.2` или `libmpv.so` на Android,
`bin/libmpv-2.dll` на Windows, `libmpv.2.dylib` на macOS, плюс библиотеки FFmpeg), `include/`
(`mpv/client.h`, `mpv/render*.h` и заголовки FFmpeg), `lib/pkgconfig/` (перемещаемые `.pc` через
`${pcfiledir}`), `share/licenses/` и `BUILDINFO.txt` (версии, SHA-256, патчи, список возможностей
mpv, опции meson, строка `configure` FFmpeg). Каталоги `.cache/`, `build/`, `out/` в git не попадают.

Переменные окружения: `PA_FF_JOBS`, `PA_MPV_CACHE`, `PA_MPV_WORK`, `PA_MPV_OUT`,
`PA_MPV_EXTRA_FLAGS` и `PA_FF_EXTRA_FLAGS` (дополнительные опции тоже проходят лицензионную
проверку), `ANDROID_NDK_HOME`, `PA_FF_ANDROID_API` (по умолчанию 24), `PA_FF_MACOS_MIN`.

Linux-сборка делается на Ubuntu 24.04 (glibc 2.39) — как и Flutter-приложение в R01-REL, — поэтому
работает на дистрибутивах с glibc ≥ 2.39. Для более старых систем — Flatpak (PKG-задачи).

## Лицензионная проверка

Сборка падает, если:

1. в опциях mpv нет `-Dgpl=false`, есть `-Dgpl=true` или включена GPL-возможность
   (`assert_mpv_lgpl_flags`, до сборки зависимостей);
2. в `config.h` mpv `HAVE_GPL` не равен 0, в `CONFIGURATION` нет `-Dgpl=false` или в списке
   возможностей (`FULLCONFIG`) есть GPL-возможность — сразу после `meson setup` mpv;
3. FFmpeg не проходит гейт [`tools/ffmpeg/check-license.sh`](../ffmpeg/README.md#лицензионная-проверка)
   (`LGPL version 2.1 or later`, без `--enable-gpl/nonfree/version3`);
4. libmpv в артефакте не содержит строку `-Dgpl=false` или содержит `-Dgpl=true`, либо в ней нет
   списка возможностей из `BUILDINFO.txt` (так проверяется, что бинарник соответствует проверенной
   конфигурации), либо какая-то библиотека артефакта ссылается на GPL/nonfree-библиотеку
   (`librubberband`, `libdvdnav`, `libcdio`, `libjack`, `libx264` и т. д.);
5. для нативного таргета — `check_libmpv.c`, слинкованный с артефактом: `mpv_client_api_version()`
   совпадает с заголовками, `mpv_create()`/`mpv_initialize()` работают, свойство
   `mpv-configuration` содержит `-Dgpl=false`; плюс `check_license.c` из `tools/ffmpeg`.

Отдельно: `tools/mpv/check-license.sh --config <build/mpv>`, `--artifact <каталог>` или `--run <каталог>`.
Гейт релиза R01-REL (`packaging/app/android/check-native-licences.sh`) ищет в `libmpv.so`
строку `-Dgpl=false` — наша сборка её содержит.

## Как media_kit берёт нашу libmpv

media_kit (Dart) загружает libmpv через FFI: сначала путь из `MediaKit.ensureInitialized(libmpv: …)`,
затем переменная `LIBMPV_LIBRARY_PATH`, затем имена по умолчанию (`libmpv.so`, `libmpv.so.2` на
Linux; `libmpv.so` на Android; `libmpv-2.dll` на Windows; `Mpv.framework/Mpv` на macOS).

### Linux (AppImage, tar.gz)

`media_kit_libs_linux` своей libmpv не содержит, а плагин `media_kit_video` линкуется с libmpv
через `pkg-config mpv` во время `flutter build linux`.

1. Сборка приложения — с нашими заголовками и `.pc`:
   `PKG_CONFIG_PATH=<libmpv-linux-x86_64>/lib/pkgconfig flutter build linux --release`.
2. Упаковка — хук перед сборкой tar.gz/AppImage:
   `tools/mpv/bundle.sh linux <libmpv-linux-x86_64> build/linux/x64/release/bundle`.
   Он кладёт `libmpv.so.2` и `libav*/libsw*` в `bundle/lib/`, лицензии и `BUILDINFO.txt` — в
   `bundle/licenses/libmpv/`, и ставит `RUNPATH=$ORIGIN` у `libmedia_kit_video_plugin.so`
   (нужен `patchelf`), чтобы плагин взял libmpv из бандла, а не из системы. У самой `libmpv.so.2`
   `RUNPATH=$ORIGIN` задан при сборке — FFmpeg находится рядом.
3. Dart-сторона должна открыть **ту же** библиотеку, иначе при установленной системной `libmpv`
   по имени `libmpv.so` загрузится вторая копия. Достаточно одного из двух:
   `MediaKit.ensureInitialized(libmpv: '<каталог исполняемого файла>/lib/libmpv.so.2')` в `pa_player`
   (если файл существует) или `export LIBMPV_LIBRARY_PATH="$APPDIR/usr/lib/kinolune/lib/libmpv.so.2"`
   в `AppRun` и лаунчере tar.gz.

### Android

`media_kit_libs_android_video` скачивает при сборке jar-файлы с `libmpv.so` (LGPL v3, FFmpeg внутри)
и `libmediakitandroidhelper.so`. Наш набор для каждого ABI —
`libmpv.so`, `libavcodec.so`, `libavformat.so`, `libavfilter.so`, `libavutil.so`, `libswresample.so`,
`libswscale.so` и `libmediakitandroidhelper.so` (JNI-хелпер media_kit, MIT, собран из исходников).
Имена без версий, SONAME совпадает с именем файла, сегменты выровнены на 16 КБ (`bundle.sh check-android`).
media_kit на Android вызывает `av_jni_set_java_vm` из `libavcodec.so` — с отдельным FFmpeg патч
media_kit `mpv_lavc_set_java_vm` не нужен.

Замена: `tools/mpv/bundle.sh android <модуль>/src/main/jniLibs <libmpv-android-arm64-v8a> …` и
отказ от jar-файлов `media_kit_libs_android_video`. Java-часть этого пакета (регистрация плагина и
`MediaKitAndroidHelper`) нужна, поэтому проще всего — локальная копия пакета через
`dependency_overrides` в `pubspec.yaml` приложения, у которой вместо задачи `downloadDependencies`
в `src/main/jniLibs` лежит наш набор. Это делает R01-PLAYER/R01-REL; гейт
`packaging/app/android/check-native-licences.sh` затем видит в APK `libmpv.so` с `-Dgpl=false` и
FFmpeg `LGPL version 2.1 or later`.

### Windows и macOS

Windows: `bin/libmpv-2.dll` и DLL FFmpeg кладутся рядом с `.exe` — media_kit найдёт их по имени.
macOS: media_kit ищет `Mpv.framework/Mpv`; упаковка dylib во framework — задача PLAY-APL/PKG.

## Исходники для соответствия LGPL

`tools/mpv/build.sh sources` собирает `out/project_alpha-libmpv-sources-<версия mpv>.tar.xz`:
оригинальные архивы всех компонентов (FFmpeg и его зависимостей тоже) с `SHA256SUMS`, оба рецепта
(`tools/ffmpeg`, `tools/mpv`) с патчами — всё, чтобы пересобрать библиотеки и перелинковать
приложение. CI выкладывает архив артефактом `libmpv-sources`; к релизам его прикладывает CI-011.

## Статус проверки

CI — [`.github/workflows/libmpv.yml`](../../.github/workflows/libmpv.yml) (PR и push в `main` при
изменениях в `tools/mpv/` или `tools/ffmpeg/`, плюс ручной запуск):

| Job | Где | Проверки |
|---|---|---|
| `linux-x86_64`, `windows-x86_64` | `ubuntu-24.04`, Docker-образ | статическая; для Linux ещё `check_libmpv` и `check_license` на раннере |
| `android-arm64-v8a`, `android-armeabi-v7a`, `android-x86_64` | `ubuntu-24.04`, NDK r28c | статическая + `bundle.sh check-android` |
| `macos-arm64`, `macos-x86_64` | `macos-15` | статическая; для `macos-arm64` ещё `check_libmpv` |
| `Sources and GPL canary` | `ubuntu-24.04` | `-Dgpl=true`, `-Dx11`, `-Ddvdnav`, `-Drubberband`, `-Djack` и FFmpeg `--enable-gpl` роняют сборку; GPL-`config.h` не проходит `--config`; архив исходников |

- Результаты кэшируются по хэшу обоих рецептов и тулчейна; при попадании в кэш сборка
  пропускается, лицензионная проверка выполняется всё равно. Артефакты `libmpv-<target>` хранятся 7 дней.
- Не проверено: воспроизведение на реальном Android-устройстве и в собранном приложении,
  DLL на реальной Windows. Это делают R01-PLAYER/R01-REL и PLAY-AND-007.
- Не сделано: libdovi (Rust, MIT) — нужен cargo-c под все таргеты; DV P5 обрабатывает встроенная
  реализация libplacebo, FFmpeg 9 сам разбирает RPU. iOS/tvOS не собираются: там MPVKit и
  нативный движок (ADR-0003).

## Обновление версий

Поменять версию, URL и SHA-256 в `versions.sh` одним коммитом, пересобрать хотя бы `linux-x86_64`,
проверить, что патчи применяются. mpv требует минимальную версию libplacebo (`meson.build`).
