<!-- SPDX-License-Identifier: MPL-2.0 -->

# LGPL-сборка libsmb2 (NET-002)

Скрипты собирают [libsmb2](https://github.com/sahlberg/libsmb2) — клиент SMB2/3 на C
под **LGPL v2.1+** — как **динамическую библиотеку** для всех клиентских платформ;
на iOS и tvOS — как embedded `smb2.framework` (и общий `smb2.xcframework`).
Обоснование — [`docs/research/02-client-stack-playback.md`](../../docs/research/02-client-stack-playback.md)
§3.1, §4. Rust-обёртка — крейт [`pa-smb2`](../../core/crates/pa-smb2), она загружает
библиотеку во время выполнения ([ADR-NNNN](../../docs/adr/0033-libsmb2-runtime-loading.md)).

Структура и лицензионный гейт повторяют [`tools/ffmpeg`](../ffmpeg/README.md) и
[`tools/mpv`](../mpv/README.md).

## Что внутри

| Файл | Назначение |
|---|---|
| `versions.sh` | Закреплённая версия, URL и SHA-256 исходников (ветка 6.x, сейчас 6.3.2) |
| `build.sh` | Точка входа: `list`, `fetch`, `<target>`, `xcframework`, `sources` |
| `lib/targets.sh` | Тулчейны CMake по таргетам (mingw-w64, NDK, Xcode SDK) |
| `lib/smb2.sh` | **Единственное место** с опциями CMake, списком запрещённых опций и списком функций, которые загружает `pa-smb2` |
| `check-license.sh`, `check_smb2.c` | Гейт: разделяемая библиотека, зависимости только системные, нужные функции экспортируются, тексты LGPL на месте; для нативного таргета — `ldd`/`otool -L` тестовой программы и её запуск |
| `docker/Dockerfile`, `docker-build.sh` | Сборочный образ Ubuntu 24.04 (Linux, Windows через mingw-w64, Android с NDK) |
| `patches/libsmb2/*.patch` | Наши патчи (сейчас один: сборка через mingw-w64) |

## Конфигурация

Общие опции CMake (`lib/smb2.sh`):

```
-DBUILD_SHARED_LIBS=ON -DENABLE_EXAMPLES=OFF
-DENABLE_LIBKRB5=OFF -DENABLE_GSSAPI=OFF
-DCMAKE_BUILD_TYPE=Release -DCMAKE_POSITION_INDEPENDENT_CODE=ON
```

- Только разделяемая библиотека: `-DBUILD_SHARED_LIBS=OFF` роняет сборку до запуска CMake.
- Без Kerberos: аутентификация — встроенный в libsmb2 NTLMSSP, подпись и шифрование SMB3
  (AES-CCM/GCM, SHA, HMAC) — тоже встроенные; внешних зависимостей нет, OpenSSL не нужен.
- `examples/` (BSD-2) не собирается.

| Таргет | Артефакт | Зависимости |
|---|---|---|
| `linux-x86_64` | `lib/libsmb2.so.1` (soname `libsmb2.so.1`) | `libc.so.6` |
| `windows-x86_64` | `bin/libsmb2.dll` + `lib/libsmb2.dll.a` | `KERNEL32`, `WS2_32`, `msvcrt` (libgcc статически) |
| `android-*` (`arm64-v8a`, `armeabi-v7a`, `x86_64`) | `lib/libsmb2.so` (soname без версии, страницы 16 КБ) | `libc`, `libm`, `libdl` |
| `macos-arm64`, `macos-x86_64` | `lib/libsmb2.1.dylib`, install name `@rpath/libsmb2.1.dylib` | `libSystem` |
| `ios-*`, `tvos-*` (устройство и симулятор) | `smb2.framework` (install name `@rpath/smb2.framework/smb2`); `build.sh xcframework` → `smb2.xcframework` | `libSystem` |

Linux-библиотека собирается на Ubuntu 24.04 (glibc 2.39), чтобы работать везде, где работает
Linux-сборка приложения.

## Как собрать

```bash
PA_SMB2_JOBS=4 tools/libsmb2/docker-build.sh linux-x86_64
PA_SMB2_JOBS=4 tools/libsmb2/docker-build.sh windows-x86_64
ANDROID_NDK_VERSION=r28c tools/libsmb2/docker-build.sh android-arm64-v8a
```

Без Docker (нужны CMake ≥ 3.5, Ninja, компилятор C):

```bash
tools/libsmb2/build.sh linux-x86_64
ANDROID_NDK_HOME=/path/to/ndk tools/libsmb2/build.sh android-arm64-v8a
tools/libsmb2/build.sh ios-arm64 && tools/libsmb2/build.sh xcframework   # только на macOS с Xcode
```

Результат — `tools/libsmb2/out/<target>/`: библиотека, `include/smb2/`, `share/licenses/`
(`COPYING` и полный текст LGPL 2.1) и `BUILDINFO.txt` (версия, SHA-256, патчи, флаги CMake).
Каталоги `.cache/`, `build/`, `out/` в git не попадают.

Переменные окружения: `PA_SMB2_JOBS`, `PA_SMB2_CACHE`, `PA_SMB2_WORK`, `PA_SMB2_OUT`,
`PA_SMB2_EXTRA_FLAGS` (тоже проходят гейт), `ANDROID_NDK_HOME`, `PA_SMB2_ANDROID_API`
(по умолчанию 24), `PA_SMB2_MACOS_MIN`, `PA_SMB2_IOS_MIN`, `PA_SMB2_TVOS_MIN`.

## Где библиотеку ищет приложение

`pa-smb2` открывает её через `libloading` в момент первого SMB-подключения (`Smb2Lib::shared`):
сначала путь из `PA_SMB2_LIB`, затем имена платформы — `libsmb2.so.1` (Linux), `libsmb2.so`
(Android, из `jniLibs`), `libsmb2.1.dylib` через `@rpath` (macOS), `@rpath/smb2.framework/smb2`
(iOS, tvOS), `libsmb2.dll` рядом с exe (Windows). Без библиотеки SMB-источники недоступны,
остальное приложение работает. Упаковку в клиенты делают задачи платформ (как для FFmpeg и libmpv).

## Лицензионная проверка и «линкуется динамически»

Сборка падает, если:

1. в опциях CMake есть `-DBUILD_SHARED_LIBS=OFF` или включён Kerberos (`assert_dynamic_flags`
   до CMake, `check-license.sh --config` по `CMakeCache.txt` после);
2. рядом лежит статический архив (`libsmb2.a`);
3. артефакт — не разделяемая библиотека (ELF `ET_DYN`, Mach-O `MH_DYLIB`, PE с флагом `DLL`);
4. библиотека зависит от чего-то кроме системных (`readelf -d` для ELF, `otool -L` для Mach-O,
   `objdump -p` — аналог `dumpbin /dependents` — для PE);
5. на Apple install name не `@rpath/…`;
6. не экспортируется какая-то из функций, которые загружает `pa-smb2` (`SMB2_REQUIRED_SYMBOLS`);
7. нет `share/licenses/libsmb2-COPYING` и полного текста LGPL 2.1;
8. для нативного таргета (`linux-x86_64`, `macos-arm64` на Apple Silicon): `check_smb2.c`,
   слинкованный с артефактом, по `ldd`/`otool -L` грузит libsmb2 из артефакта, не содержит её
   копии (`nm`) и проходит путь «контекст → опции → ошибка подключения».

Наши бинарники libsmb2 не линкуют вовсе (загрузка в рантайме), так что в них её кода нет по
построению; это проверяет тест `pa-smb2` и отсутствие `smb2_*` в `nm` любого нашего бинаря.

Отдельно: `tools/libsmb2/check-license.sh --artifact <каталог>` или `--run <каталог>`.

## Исходники для соответствия LGPL

`tools/libsmb2/build.sh sources` собирает `out/project_alpha-libsmb2-sources-<версия>.tar.xz`:
оригинальный архив с `SHA256SUMS`, наши патчи и эти скрипты. Архив прикладывается к релизу
вместе с исходниками FFmpeg (CI-011).

## Статус проверки

Все таргеты собираются в CI — [`.github/workflows/libsmb2.yml`](../../.github/workflows/libsmb2.yml)
(на PR при изменениях в `tools/libsmb2/` и вручную):

| Job | Где | Проверки |
|---|---|---|
| `linux-x86_64`, `windows-x86_64` | `ubuntu-24.04`, Docker-образ | гейт; для Linux ещё `ldd` и запуск `check_smb2` |
| `android-*` (3 ABI) | `ubuntu-24.04`, NDK r28c | гейт |
| `apple` | `macos-15`: 6 срезов и `smb2.xcframework` | гейт; для `macos-arm64` ещё `otool -L` и запуск `check_smb2` |
| `Sources and static-build canary` | `ubuntu-24.04` | `-DBUILD_SHARED_LIBS=OFF` и `-DENABLE_LIBKRB5=ON` роняют сборку; статический `CMakeCache.txt` не проходит `--config` |

Проверено вручную на Linux-сервере сборки: `linux-x86_64` (гейт, `ldd`, `check_smb2`, тесты
`pa-smb2` с `PA_SMB2_LIB` и контракт-тесты `pa-vfs-smb` против Samba), `windows-x86_64`
и три Android ABI (гейт). Apple-срезы проверяются только в CI; DLL на настоящей Windows и
`.so` на устройствах не запускались — это задачи упаковки клиентов.

## Обновление версии

Поменять версию, URL и SHA-256 в `versions.sh` одним коммитом, пересобрать хотя бы
`linux-x86_64` и `windows-x86_64`, проверить, что патчи применяются, прогнать контракт-тесты
`pa-vfs-smb` против Samba.
