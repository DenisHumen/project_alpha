# Замеры PLAY-DSK-001: Linux, Intel UHD 630, 2026-11-13

Сырые данные: [`2026-11-13-linux-intel-uhd630.jsonl`](2026-11-13-linux-intel-uhd630.jsonl) (вывод `SPIKE_RESULT` каждого прогона).
Получено командой `BACKENDS="wayland x11 xvfb" SECONDS_PER_RUN=15 tools/run-in-docker.sh`, таблица — `tools/summarize.py`.

## Окружение

- CPU Intel Core i3-10100 (4C/8T), GPU Intel UHD 630 (CometLake GT2), Mesa 26.0.8, VAAPI iHD 26.1.2.
- Контейнер Ubuntu 26.04: Flutter 3.50.0 (Impeller, OpenGL ES), GTK 3.24, libmpv 0.41.0, FFmpeg 8.0.1; лимит `--cpus=4`.
- Композитор: headless sway (wlroots, рендерер gles2 на GPU хоста), Xwayland из того же sway; отдельно Xvfb (без DRI3 → llvmpipe).
- Машина общая, фоновая нагрузка во время замеров: load average 8–12. Разброс между прогонами заметный
  (тот же 4K-texture на Wayland в предыдущем прогоне давал 13.6 fps вместо 23.7).
- Окно замера: 12 с после 3 с прогрева; CPU — `getrusage` всего процесса (декод + mpv + Flutter), в процентах одного ядра.
- Клипы (генерируются скриптом): `h264_1080p30_sdr.mp4` — H.264 High 1920×1080 30p; `hevc10_2160p24_hdr10.mkv` —
  HEVC Main10 3840×2160 24p, BT.2020/PQ, HDR10-метаданные (mastering display, MaxCLL 1000).

## Таблица

«delivered fps» для `texture` — сколько раз Flutter забрал новый кадр (`populate()`), то есть реально показанные кадры.
Для `native` mpv показывает кадры сам, поэтому здесь только счётчик пропусков VO (`frame-drop-count`).

| run | VO / context | hwdec | delivered fps | CPU, % of 1 core | render ms avg / max | RSS, MB |
|---|---|---|---|---|---|---|
| h264_1080p30_sdr-wayland-texture | libmpv | vaapi | 30.0 (src 30) | 12.5 | 9.9 / 95 | 259 |
| h264_1080p30_sdr-wayland-native | gpu-next/waylandvk | vaapi | n/a (src 30, drops 0) | 47.5 | — | 281 |
| h264_1080p30_sdr-x11-texture | libmpv | vaapi | 30.0 (src 30) | 10.2 | 11.8 / 92 | 235 |
| h264_1080p30_sdr-x11-native | gpu-next/x11vk | vaapi | n/a (src 30, drops 0) | 3.0 | — | 284 |
| h264_1080p30_sdr-xvfb-texture | libmpv | vaapi-copy | 4.9 (src 30) | 237.6 | 88.8 / 476 | 562 |
| h264_1080p30_sdr-xvfb-native | gpu-next/x11egl | vaapi-copy | n/a (src 30, drops 128) | 271.5 | — | 509 |
| hevc10_2160p24_hdr10-wayland-texture | libmpv | vaapi | 23.7 (src 24) | 11.0 | 33.5 / 181 | 268 |
| hevc10_2160p24_hdr10-wayland-native | gpu-next/waylandvk | vaapi | n/a (src 24, drops 2) | 52.0 | — | 317 |
| hevc10_2160p24_hdr10-x11-texture | libmpv | vaapi | 23.7 (src 24) | 9.3 | 33.9 / 217 | 271 |
| hevc10_2160p24_hdr10-x11-native | gpu-next/x11vk | vaapi | n/a (src 24, drops 0) | 2.9 | — | 324 |
| hevc10_2160p24_hdr10-xvfb-texture | libmpv | vaapi-copy | 1.5 (src 24) | 252.3 | 420.7 / 693 | 926 |
| hevc10_2160p24_hdr10-xvfb-native | gpu-next/x11egl | vaapi-copy | n/a (src 24, drops 116) | 241.5 | — | 848 |

`x11` = приложение с `GDK_BACKEND=x11` под XWayland; `xvfb` = чистый X-сервер без GPU (только проверка видимости, не скорости).

## Что видно на скриншотах

| Случай | Видео в окне приложения | OSD Flutter поверх видео |
|---|---|---|
| `texture`, Wayland / XWayland / Xvfb | да | да |
| `native`, X11 (Xlib-дочернее окно, `--wid`), XWayland и Xvfb | да | **нет** — окно mpv перекрывает Flutter |
| `native`, X11, дочернее окно через GDK (`SPIKE_X11_GDK_CHILD=1`) | **нет** — GTK перерисовывает поверх | да (но видео нет) |
| `native`, Wayland | **нет** — mpv открывает отдельное окно (своё `xdg_toplevel`), в окне приложения — чёрная область | только в своём окне |

HDR10-клип в `texture` выводится тон-маппингом в SDR (выход Flutter — 8-битный sRGB). Цвета у `vo=libmpv` (рендерер `gpu`) и
`vo=gpu-next` заметно различаются (разный тон-маппинг по умолчанию) — в продукте рендерер и параметры тон-маппинга нужно задавать явно.

## Оговорки

- **Wayland `native`: высокий CPU (47–52 %) — скорее всего артефакт headless-композитора.** У headless-бэкенда wlroots нет DRM-устройства
  (в логе sway: `Failed to get backend DRM FD`), и Vulkan WSI клиента, по всей видимости, копирует кадры через CPU.
  На реальном композиторе (GNOME/KDE) не проверено.
- Синтетический ввод (пробел через `wtype`/`xdotool`) в headless-сессии доходит нестабильно: в `native` Flutter получил клавишу
  во всех случаях (после явного фокуса окна композитором), в `texture` под sway — нет, под Xvfb — да. Вывод о фокусе сделан
  по устройству окон, а не по этим цифрам: см. ADR-0009.
- Реальные HDR-дисплеи, HDR-сигнализация (Wayland color-management, Windows HDR) и звук не проверялись (`ao=null`).
