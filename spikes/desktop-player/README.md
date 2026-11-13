# Спайк PLAY-DSK-001: libmpv во Flutter-десктопе

Одноразовый прототип (throw-away) для [ADR-0009](../../docs/adr/0009-desktop-player-embedding.md).
Сравнивает два способа показать видео libmpv внутри Flutter-приложения для Linux
и проверяет, как каждый влияет на OSD (Flutter-оверлей поверх видео) и фокус клавиатуры.
Код спайка в продукт не переносится: рабочая реализация — задача `PLAY-DSK-002`
(`clients/flutter/packages/pa_player_desktop/`, `clients/desktop/`).

| Режим | Как устроен | OSD Flutter поверх видео |
|---|---|---|
| `texture` | libmpv render API (OpenGL, `vo=libmpv`) рисует в FBO на отдельном потоке, в своём EGL-контексте, **разделяемом с EGL-контекстом Flutter**; текстура отдаётся как `FlTextureGL` | да |
| `native` | mpv сам владеет поверхностью, `vo=gpu-next`: X11 — дочернее Xlib-окно через `--wid`; Wayland — собственное окно mpv (в libmpv нет `--wid` для Wayland) | нет |

Файлы:

- `linux/runner/mpv_spike.cc` — весь нативный код (канал `pa.spike/mpv`, `FlTextureGL`, render-поток, `--wid`);
- `lib/main.dart` — экран: видео + красная плашка «FLUTTER OSD», авто-выход и печать метрик `SPIKE_RESULT {json}`;
- `docker/Dockerfile` — окружение сборки и тестов (Ubuntu 26.04, Flutter 3.50.0, libmpv 0.41, sway, Xwayland, Xvfb);
- `tools/run-in-docker.sh` — точка входа на хосте; `tools/bench-headless.sh` — прогон матрицы внутри контейнера;
- `tools/summarize.py` — таблица из `out/results.jsonl`.

## Как запустить

Нужен только Docker (пользователь в группе `docker`); sudo и системные пакеты на хосте не нужны.
GPU пробрасывается через `/dev/dri` (проверено на Intel, VAAPI iHD).

```bash
cd spikes/desktop-player
tools/run-in-docker.sh                       # полная матрица: 2 клипа × (Wayland, X11/XWayland) × (texture, native)
BACKENDS="wayland x11 xvfb" tools/run-in-docker.sh   # + чистый X-сервер (Xvfb, без GPU — только «что видно»)
CLIPS='h264*' MODES=texture SECONDS_PER_RUN=10 tools/run-in-docker.sh   # подмножество
python3 tools/summarize.py out/results.jsonl
```

Скрипт генерирует тестовые клипы ffmpeg-ом (`out/media/`: H.264 1080p30 SDR и HEVC Main10 2160p24 с метаданными HDR10),
собирает приложение, поднимает headless sway (Wayland + Xwayland) на GPU хоста и для каждого случая:
запускает приложение на 15 с, отправляет пробел (проверка фокуса), снимает скриншот (`grim`/`import`) и пишет метрики.
Всё складывается в `out/` (в git не попадает).

Ручной запуск на своём рабочем столе (нужны `libmpv-dev`, `libgtk-3-dev`, `clang`, `cmake`, `ninja-build` и Flutter):

```bash
flutter build linux --release
SPIKE_FILE=/path/to/clip.mkv SPIKE_MODE=texture build/linux/x64/release/bundle/desktop_player
GDK_BACKEND=x11 SPIKE_FILE=... SPIKE_MODE=native build/linux/x64/release/bundle/desktop_player
```

Переменные: `SPIKE_FILE`, `SPIKE_MODE` (`texture`|`native`), `SPIKE_SECONDS` (авто-выход), `SPIKE_HWDEC` (по умолчанию `auto-safe`),
`SPIKE_AO`, `SPIKE_VO` (для `native`, по умолчанию `gpu-next`), `SPIKE_MPV_OPTS="k=v;k=v"`,
`SPIKE_X11_GDK_CHILD=1` (показать ловушку с GDK-окном, см. ниже).

## Результаты

Сводка и выводы — в [ADR-0009](../../docs/adr/0009-desktop-player-embedding.md). Ниже — то, что важно знать при чтении кода.

Найденные ловушки (каждая стоила часа отладки):

1. **Flutter 3.50 на Linux рисует через собственные EGL-контексты (Impeller, GLES).** `GdkGLContext` не делит с ними
   текстуры — видео получается чёрным. Работает только контекст, созданный `eglCreateContext(..., share = текущий контекст Flutter)`
   внутри первого вызова `FlTextureGL::populate()` (он идёт не на главном потоке GTK).
2. **На X11 GTK держит GLX-контекст текущим на главном потоке**, и libglvnd отказывает `eglMakeCurrent` (`EGL_BAD_ACCESS`).
   Поэтому mpv рендерит на своём потоке, где EGL-контекст текущий постоянно. На Wayland та же схема тоже работает.
3. **`MPV_RENDER_PARAM_BLOCK_FOR_TARGET_TIME` по умолчанию 1** — `mpv_render_context_render` ждёт момента показа кадра.
   Если рендерить на главном потоке GTK, это блокирует UI (видели 30 мс на кадр и падение композиции Flutter до ~11 fps).
4. **mpv выбирает Wayland-контекст, если задан `WAYLAND_DISPLAY`, даже при `--wid`** (приложение под XWayland):
   mpv молча открывает своё Wayland-окно. Нужно явно `gpu-context=x11vk,x11egl`.
5. **Дочернее окно, созданное через GDK (`gdk_window_new` + `ensure_native`), перекрашивается GTK**: mpv встраивается и играет,
   но видео не видно ни на Xvfb, ни на XWayland. Обычное Xlib-окно (`XCreateSimpleWindow`, GDK о нём не знает) показывается правильно.
6. В режиме `texture` FBO сейчас размером с видео (4K для HEVC-клипа) — это дорого (≈35 мс на кадр на Intel UHD 630).
   В продукте FBO должен быть размером с виджет в физических пикселях, плюс кольцо из 2–3 текстур с fence вместо `glFinish`.

## Лицензии

- Код спайка — MPL-2.0.
- libmpv подключается **динамически** (`pkg-config mpv`). В Docker-образе — системный `libmpv2` Ubuntu 26.04 (mpv 0.41.0, FFmpeg 8.0.1).
  Пакеты Ubuntu собраны с GPL-частями (FFmpeg `--enable-gpl`), поэтому образ — **только для локальной проверки**,
  бинарники из него не распространяются. Для релизов mpv собирается с `-Dgpl=false` поверх LGPL-FFmpeg из `PLAY-001` (задача `PLAY-002`).
- `libx264`/`libx265` используются только для генерации тестовых клипов внутри контейнера и в приложение не попадают.
