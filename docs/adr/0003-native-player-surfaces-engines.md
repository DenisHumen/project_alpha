# ADR-0003: Нативная поверхность плеера и гибридные движки воспроизведения

- **Статус:** Принят
- **Дата:** 2026-11-13
- **Решают:** владелец проекта
- **Связано:** `PLAY-001`…, `PLAY-AND-001` (→ ADR-0008), `PLAY-DSK-001` (→ ADR-0009), `PLAY-APL-…`, `docs/STRATEGY.md` §3, `docs/research/02` §2

## Контекст и проблема

Главная ценность медиаплеера — качество картинки и звука: HDR10, Dolby Vision, переключение частоты кадров и динамического диапазона (AFR / Match Content), passthrough AC-3/E-AC-3/TrueHD/DTS, корректные ASS/PGS-субтитры. Flutter композит видео из `Texture` в SDR-поверхность: HDR, DV, AFR и HDMI-режимы на этом пути теряются. Ни один движок не умеет всё на всех платформах: AVFoundation на Apple даёт DV, но не играет MKV; mpv играет всё, но на Apple TV не выводит DV; Media3 лучше всех интегрирован с Android TV.

## Факторы решения

- HDR10, DV, AFR и passthrough на целевых устройствах (Apple TV 4K, Android TV, HDR-мониторы).
- Широкая поддержка контейнеров и кодеков (MKV, HEVC, AV1, TrueHD, DTS).
- Единый UI управления поверх видео на всех платформах.
- Лицензии: только LGPL-сборки FFmpeg/mpv, динамически (ADR-0005).
- Возможность заменить движок без переписывания UI.

## Рассмотренные варианты

1. **Нативная поверхность плеера на каждой платформе + гибрид движков** (как в Infuse).
2. Один движок везде (libmpv или libVLC) в Flutter `Texture`.
3. Один движок везде (libmpv/libVLC), но в нативной поверхности.
4. Только системные плееры (AVPlayer, ExoPlayer, Media Foundation).

## Решение

Выбран вариант **1**.

**Поверхность.** Экран воспроизведения всегда нативный: Android — `Activity`/`SurfaceView` (или PlatformView в Hybrid Composition), Apple — `UIViewController`/`NSView` с `AVSampleBufferDisplayLayer` или `AVPlayerLayer`, десктоп — нативное дочернее окно с `vo=gpu-next` для HDR (render API в Texture допустим только для SDR v1). Оверлей управления — Flutter-слой поверх или нативный; конкретный способ встраивания выбирают спайки: **ADR-0008** (Android, `PLAY-AND-001`) и **ADR-0009** (десктоп, `PLAY-DSK-001`).

**Движки по платформам:**

| Платформа | Основной | Запасной |
|---|---|---|
| Android, Android TV | Media3/ExoPlayer (+ libass-оверлей) | libmpv |
| iOS, iPadOS, macOS, tvOS | этап 1: AVPlayer + MPVKit (не-GPL сборка); этап 2: свой FFmpeg-демуксер → VideoToolbox → `AVSampleBufferDisplayLayer` (HDR/DV) | MPVKit |
| Windows, Linux, macOS (Flutter-десктоп) | libmpv (gpu-next/libplacebo) | — |

Логика «Auto»: если основной движок может воспроизвести файл (с учётом DV/HDR) — он; иначе запасной. Пользователь может выбрать движок вручную.

**Единый контракт** `PlayerEngine` (задача `PLAY-003`): `open(source, startPos)`, дорожки, `seek`, скорость, события, статистика. Ядро (ADR-0001) отдаёт `ByteSource` и политику выбора дорожек; движок отвечает только за декодирование и вывод.

### Последствия

- Хорошо: HDR, DV, AFR и passthrough доступны там, где их поддерживает железо; движок меняется без переписывания UI; Apple-путь можно улучшать поэтапно.
- Плохо: три-четыре интеграции плеера вместо одной; фокус и оверлей поверх нативной поверхности сложнее (особенно на TV); свой Apple-движок — большой объём работы (этап 2, M5).
- Требует работы: LGPL-сборка FFmpeg (`PLAY-001`), контракт `PLAY-003`, спайки → ADR-0008/0009, проверка на реальных устройствах по чек-листу `QA-011`.

### Как проверяем соблюдение

- CI проверяет, что FFmpeg собран как «LGPL version 2.1 or later», а mpv — с `-Dgpl=false`.
- Ревью отклоняет воспроизведение видео через Flutter `Texture` для HDR-контента.

## Плюсы и минусы вариантов

### Один движок в Flutter Texture

- Плюс: проще всего.
- Минус: только SDR; нет AFR и passthrough — неприемлемо для продукта уровня Infuse.

### Один движок в нативной поверхности

- Плюс: одна интеграция декодера.
- Минус: mpv/libVLC не выводят DV на Apple TV; хуже интеграция с Android TV (AFR, MediaSession, tunneled playback).

### Только системные плееры

- Плюс: лучшая энергоэффективность и интеграция.
- Минус: AVPlayer не играет MKV/ASS/TrueHD; Media Foundation и ExoPlayer ограничены по форматам — нет паритета с Infuse.

## Ссылки

- `docs/research/02-client-stack-playback.md` §2.1–2.8, §3.1
- [MPVKit](https://github.com/mpvkit/MPVKit), [mpv](https://mpv.io/), [AndroidX Media3](https://developer.android.com/media/media3)
