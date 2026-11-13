# ADR-0009: Встраивание libmpv во Flutter-десктоп — текстура для окна, нативная поверхность для полноэкранного HDR

- **Статус:** Предложен
- **Дата:** 2026-11-13
- **Решают:** владелец проекта
- **Связано:** `PLAY-DSK-001` (спайк), `PLAY-DSK-002`…`PLAY-DSK-005`, `PLAY-002`, `PLAY-003`, ADR-0003, `docs/research/02` §2.2, §2.3, §2.8; спайк — [`spikes/desktop-player/`](../../spikes/desktop-player/), замеры — [`results/2026-11-13-linux-intel-uhd630.md`](../../spikes/desktop-player/results/2026-11-13-linux-intel-uhd630.md)

## Контекст и проблема

На Windows, Linux и macOS движок — libmpv (ADR-0003). Вопрос — как показать его кадры внутри Flutter-приложения:

- **Texture:** libmpv render API рисует в GL-текстуру, Flutter композитит её как обычный виджет.
- **Нативная поверхность:** mpv сам владеет окном/поверхностью (`vo=gpu-next`, libplacebo) — HWND, X11-окно,
  Wayland-поверхность, `NSView`.

От выбора зависят HDR (ADR-0003 запрещает HDR через `Texture`), OSD поверх видео, фокус клавиатуры и сложность кода.
Спайк собран и прогнан на Linux; Windows и macOS разобраны по документации и опыту других проектов, но не запускались.

## Факторы решения

- HDR10/HLG/DV на HDR-мониторах: нужен вывод 10 бит и HDR-сигнализация (`vo=gpu-next`, ADR-0003).
- Единый Flutter-OSD поверх видео (контролы, субтитры-меню, статистика) без отдельной реализации на каждой ОС.
- Фокус и горячие клавиши (`PLAY-DSK-005`) должны оставаться у Flutter.
- CPU/GPU на слабом железе (iGPU уровня Intel UHD 630) — 4K24 без пропусков.
- Объём и хрупкость нативного кода на трёх ОС и двух оконных системах Linux.

## Рассмотренные варианты

1. **Только Texture** (render API → `FlTextureGL` / DXGI shared handle / `CVPixelBuffer`) — как media_kit.
2. **Только нативная поверхность** (`--wid`/своё окно mpv, `vo=gpu-next`).
3. **Гибрид:** Texture по умолчанию (оконный режим, SDR, превью), нативная поверхность — для полноэкранного
   воспроизведения HDR-контента на HDR-дисплее.

## Что показал спайк (Linux)

Окружение: Flutter 3.50.0 (Impeller, GLES), libmpv 0.41.0, Intel UHD 630 + VAAPI, headless sway (Wayland + XWayland) и Xvfb.
Полная таблица — в [`results/`](../../spikes/desktop-player/results/2026-11-13-linux-intel-uhd630.md).

| | Texture, Wayland | Texture, X11 (XWayland) | Native, X11 (`--wid`) | Native, Wayland |
|---|---|---|---|---|
| 1080p30 H.264: показано кадров/с | 30.0 | 30.0 | все (0 пропусков) | все (0 пропусков) |
| 1080p30: CPU процесса, % ядра | 12.5 | 10.2 | 3.0 | 47.5 ¹ |
| 2160p24 HEVC10 HDR10: показано кадров/с | 23.7 ² | 23.7 | все (0 пропусков) | 2 пропуска |
| 2160p24: CPU, % ядра | 11.0 | 9.3 | 2.9 | 52.0 ¹ |
| Рендер кадра mpv в FBO, мс (сред./макс.) | 10 / 95 (1080p), 34 / 181 (4K) | 12 / 92, 34 / 217 | — | — |
| Видео видно в окне приложения | да | да | да | **нет**, отдельное окно mpv |
| Flutter-OSD поверх видео | **да** | **да** | **нет** | **нет** |
| HDR-выход | нет (8-бит SDR, тон-маппинг) | нет | возможен (gpu-next, но X11 без HDR) | возможен (gpu-next + color-management), не проверено |

¹ Скорее всего артефакт headless-композитора без DRM-устройства (Vulkan WSI копирует через CPU); на реальном GNOME/KDE не проверено.
² Нестабильно на общей машине: в другом прогоне 13.6 кадра/с. Причина — FBO размером 4K (см. ниже), не сам подход.

Выводы по Linux:

1. **Texture работает на Wayland и X11, OSD «бесплатный», CPU ~10 % ядра**, hwdec VAAPI zero-copy (`hwdec-current=vaapi`)
   при передаче `wl_display`/`Display*` в render API. Цена — копия кадра в FBO (≈10 мс на 1080p и ≈34 мс на 4K на UHD 630
   с `glFinish`) и только SDR.
2. **Native на X11 — самый дешёвый путь (≈3 % ядра)**, но Flutter не может рисовать поверх: OSD пропадает.
   Под XWayland он работает только с обычным Xlib-окном; дочернее окно через GDK перерисовывается GTK.
3. **На Wayland встроить mpv-окно в окно Flutter нельзя**: в libmpv нет `--wid` для Wayland, mpv открывает своё
   `xdg_toplevel`. Позиция, стек и фокус такого окна под контролем композитора, а не приложения.
4. Нужные приёмы (все в коде спайка): EGL-контекст, разделяемый с контекстом Flutter, создаётся в первом `populate()`
   (Flutter 3.50 использует свои EGL-контексты, `GdkGLContext` не подходит); mpv рендерит на своём потоке
   (на X11 GTK держит GLX-контекст на главном потоке, и `eglMakeCurrent` там падает с `EGL_BAD_ACCESS`);
   `MPV_RENDER_PARAM_BLOCK_FOR_TARGET_TIME=0`; для `--wid` под XWayland — явный `gpu-context=x11vk,x11egl`.

## Решение

Выбран вариант **3 — гибрид**, с Texture как основным путём.

**Texture (по умолчанию, все ОС).** Встраиваемый плеер, оконный режим, мини-плеер, превью, SDR-контент
и HDR-контент на SDR-мониторе (тон-маппинг mpv). Flutter-OSD поверх, фокус всегда у Flutter.

- Linux: render API (OpenGL) → FBO на отдельном render-потоке, EGL-контекст shared с контекстом Flutter → `FlTextureGL`.
- Windows (не проверено): Flutter рисует через ANGLE (D3D11). mpv рендерит в ANGLE-контексте (OpenGL ES) в pbuffer поверх
  D3D11-текстуры с shared handle (`EGL_ANGLE_d3d_share_handle_client_buffer`), Flutter получает её через
  `FlutterDesktopGpuSurfaceDescriptor` (`kFlutterDesktopGpuSurfaceTypeDxgiSharedHandle`). Так делает media_kit.
- macOS (не проверено): render API в OpenGL-контекст (CGL, устаревший, но рабочий), цель — IOSurface-текстура,
  Flutter получает `CVPixelBuffer` через `FlutterTexture.copyPixelBuffer`.

**Нативная поверхность (полный экран + HDR-контент + HDR-дисплей; `PLAY-DSK-003`).** Плагин создаёт и сам
управляет окном плеера с `vo=gpu-next`; OSD в этом режиме рисует mpv (OSD/libass через `osd-overlay` и свойства,
состояние приходит из Flutter), а не Flutter.

- Windows (не проверено): дочерний HWND (`--wid`), `gpu-api=d3d11`, `target-colorspace-hint=yes`, Windows HDR.
- Linux Wayland (не проверено на реальном композиторе): своё окно mpv на весь экран (полноэкранный режим),
  HDR через Wayland color-management (KDE Plasma 6, GNOME 48+). Встраивание в окно Flutter — отложено (см. открытые вопросы).
- Linux X11: обычное Xlib-дочернее окно (не через GDK), `gpu-context=x11vk,x11egl`. HDR на X11 нет — режим полезен
  только ради экономии CPU.
- macOS (не проверено): `NSView` с `CAMetalLayer` под прозрачным `FlutterView` в том же окне — здесь, в отличие от
  Linux/Windows, слои композитит Core Animation, поэтому Flutter-OSD поверх, вероятно, сохранится. EDR — `PLAY-DSK-003`.

**Требования к реализации Texture-пути (`PLAY-DSK-002`):** FBO размером с виджет в физических пикселях, а не с видео
(4K-FBO стоит ~34 мс/кадр на iGPU); кольцо из 2–3 текстур с fence (`EGL_KHR_fence_sync`/`GL_ARB_sync`) вместо `glFinish`;
render-поток на плеер; явные `vo`-параметры тон-маппинга (у `gpu` и `gpu-next` по умолчанию разные цвета).

Не входит: собственный HDR-композитинг поверх Flutter, HDR внутри Flutter (`Texture` остаётся SDR), DV-профиль 7 на десктопе.

### Последствия

- Хорошо: один OSD на Flutter для 90 % сценариев на всех ОС; HDR и минимальная нагрузка остаются доступны в полноэкранном режиме;
  Texture-путь проверен на Wayland и X11.
- Плохо: две реализации вывода и два OSD (Flutter и mpv); переключение между ними при входе/выходе из полного экрана
  (пересоздание VO, короткая пауза). Texture стоит ~10 % ядра CPU и лишнюю копию кадра на GPU.
- Требует работы: `PLAY-DSK-002` (Texture-путь по требованиям выше), `PLAY-DSK-003` (нативный HDR-режим и mpv-OSD),
  `PLAY-DSK-005` (полный экран и фокус), проверка на Windows и macOS, замеры на реальном GNOME/KDE и HDR-мониторе (`QA-011`).

## Плюсы и минусы вариантов

### Только Texture

- Плюс: одна реализация, Flutter-OSD и фокус без вопросов на всех ОС; проверено на Linux (Wayland и X11).
- Минус: только 8-бит SDR — нарушает ADR-0003 для HDR; копия кадра (~10 мс 1080p на iGPU); render API mpv — только OpenGL,
  на Windows нужен мост ANGLE↔D3D11, на macOS — устаревший OpenGL.

### Только нативная поверхность

- Плюс: HDR, минимальный CPU (3 % против 10 % на X11), mpv делает всё сам.
- Минус: Flutter не может рисовать поверх (Linux, Windows) — нужен второй OSD; на Wayland нет встраивания вовсе
  (только своё окно); фокус и стек окон — забота плагина на каждой ОС.

### Гибрид

- Плюс: берёт сильные стороны обоих; нативный путь нужен только в одном сценарии (полный экран + HDR).
- Минус: больше кода и тестов; два OSD.

## Открытые вопросы

- Wayland-встраивание: `wl_subsurface` под прозрачной поверхностью Flutter с render API в свой EGL-surface дал бы OSD и
  позиционирование, но без `gpu-next` и HDR-сигнализации mpv. Проверить при `PLAY-DSK-003`, следить за поддержкой
  встраивания на Wayland в upstream mpv.
- Нагрузка native-Wayland (47–52 % CPU в headless) — перемерить на реальном композиторе.
- Windows и macOS — замеры по той же методике (метрики `SPIKE_RESULT`) при `PLAY-DSK-002`.
- Источник ввода в нативном полноэкранном окне: на X11 и Windows дочернее окно mpv с `input-vo-keyboard=no` не забирает клавиатуру,
  но мышь над ним уходит в mpv (`input-cursor=no` отключает только обработку); на Wayland у окна mpv свой фокус — клавиши придётся
  принимать в mpv и пересылать во Flutter.

## Ссылки

- `docs/research/02-client-stack-playback.md` §2.2, §2.3, §2.8
- Спайк: `spikes/desktop-player/` (README — инструкция и список ловушек)
- mpv: `libmpv/render.h`, `libmpv/render_gl.h` (render API: только OpenGL и SW), опция `--wid`
- Flutter Linux embedder: `FlTextureGL`, `FlTextureRegistrar`
