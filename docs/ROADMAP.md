# project_alpha — дорожная карта и чек-лист разработки

> Версия 1 · 2026-11-13. Источник решений — [`docs/STRATEGY.md`](STRATEGY.md) v2. Если исследование в `docs/research/` расходится со STRATEGY, прав STRATEGY.
>
> **Отменено и в задачи не входит:** облако проекта (04 §2), биллинг, подписки и RevenueCat/Paddle (04 §3), облачный twitch-gateway и EventSub-webhook (06 §1.2–1.4), metadata-шлюз и краудсорс-сервер хэшей (03 §3.5, §4.8), деление функций на free/Pro (01 §1.2). Вместо них — локальные решения: sync по LAN, через папку SMB/WebDAV или через сервер пользователя; запросы к TMDB, Twitch и другим сервисам идут прямо с устройства; донаты.
>
> Ссылки `ref:` указывают на исследования: `01` — продукт и UX, `02` — клиентский стек и плеер, `03` — распознавание, `04` — архитектура, `05` — NAS-дистрибутив, `06` — Twitch, `S` — STRATEGY.

---

## 0. Как пользоваться документом

Этот файл — **единый backlog проекта**. Каждая задача занимает одну строку-чекбокс со стабильным ID и короткими полями. Задачи нарезаны так, чтобы их можно было раздавать параллельным AI-агентам и внешним контрибьюторам: одна задача — одна ветка — один небольшой PR.

### 0.1. Формат задачи

```
- [ ] **CORE-0NN** Короткое название
  - Что: 1–3 строки, что сделать.
  - deps: ID через запятую (или —) · size: S|M|L · parallel: yes|no · веха: M1 · ref: 04 §1.8
  - done when: проверяемые критерии приёмки (тесты, платформы).
```

- **ID стабильны.** Их никогда не перенумеровывают. Новая задача получает следующий свободный номер в своём workstream. Отменённую задачу не удаляют, а зачёркивают с причиной.
- **size:** `S` — до 1 дня, `M` — до 1 недели, `L` — больше недели. L-задачу перед стартом лучше разбить: дочерние задачи получают новые ID, а исходная становится эпиком (закрывается, когда закрыты все дочерние).
- **parallel:** `yes` — задача не трогает общие «горячие» файлы (§0.5) и может идти одновременно с любыми другими задачами без общих deps. `no` — задача меняет общий контракт или горячий файл. Её делает один агент, остальные задачи этого workstream ждут мержа.
- **Пометки в названии:** `(owner)` — делает только владелец: аккаунты, деньги, юридические письма, публикация. `[full]` — код только для full-сборок. `[store]` — только для store-сборок. `[P1]`/`[P2]` — приоритет паритета из 01 §1.5; без пометки — P0.

### 0.2. Легенда статусов

| Отметка | Значение |
|---|---|
| `- [ ]` | Открыта. Если в репозитории есть открытый (draft) PR с `[ID]` в заголовке — задача **занята** |
| `- [x]` | Смержена в `main`, все пункты `done when` выполнены |
| `- [ ] ~~**ID** …~~ (отменена: причина)` | Отменена, ID не переиспользуется |
| `(blocked: DEC-00N)` в названии | Ждёт решения владельца |

Статус «в работе» в этом файле **не ведётся**: его роль играет draft PR. Так не возникает конфликтов из-за правок «взял задачу».

### 0.3. Правила для агентов и контрибьюторов

1. **Выбор задачи.** Брать только задачу, у которой все `deps` уже отмечены `[x]` в `main` и нет открытого PR с её ID (`gh pr list --search "[ID] in:title" --state open`). Скрипт `just roadmap next` (задача REPO-012) выдаёт такие задачи. Задачи `(owner)` агенты не берут.
2. **Одна задача — одна ветка — один worktree.**
   ```bash
   git fetch origin
   git worktree add ../pa-CORE-012 -b feat/CORE-012-hlc-clock origin/main
   ```
   Имя ветки: `<type>/<ID>-<slug>`, где type ∈ `feat|fix|docs|ci|build|chore|test|refactor|perf`.
3. **Сразу открыть draft PR**, чтобы занять задачу. Заголовок — Conventional Commits с ID в конце: `feat(sync): add HLC clock and LWW merge [SYNC-001]`. Scope — имя workstream в нижнем регистре (`core`, `net`, `lib`, `meta`, `play`, `play-and`, `play-apl`, `play-dsk`, `ui`, `tv`, `sync`, `srv`, `mus`, `twi`, `dist`, `dsn`, `pkg`, `qa`, `docs`, `legal`, `site`, `comm`, `repo`, `ci`).
4. **Коммиты с DCO:** `git commit -s`. Строка `Signed-off-by` — от человека, который отвечает за вклад (у агентов — владелец или контрибьютор, запустивший агента). Дополнительно можно указать трейлер `Co-Authored-By` агента.
5. **В том же PR отметить чекбокс своей задачи** (`- [ ]` → `- [x]`) в этом файле. Менять можно только строку своей задачи. Если по ходу появилась новая работа, добавить новую задачу в конец своего workstream со следующим свободным ID отдельным коммитом `docs(roadmap): add <ID>`.
6. **Маленькие PR.** Ориентир — до ~400 изменённых строк рукописного кода (сгенерированный код и фикстуры не считаются). Если задача разрастается, её нужно разбить (п. 0.1).
7. **Тесты обязательны.** Rust — `cargo nextest` (unit и интеграционные), Flutter — unit, widget и golden, Android — JUnit и instrumented на эмуляторе, Apple — XCTest, Go — `go test`. Если в `done when` указаны платформы, в описании PR нужно перечислить, на каких устройствах или симуляторах проверено.
8. **Свои директории.** Менять можно только каталоги своего workstream (§0.4). Нужна правка в чужом каталоге — либо минимальная, с пометкой в PR, либо отдельная задача.
9. **Перед мержем** — rebase на `main`, зелёный CI, squash merge. Ревью — CODEOWNERS (владелец). Агенты не запускают релизные workflow: окружения `release` и `store` требуют подтверждения владельца.
10. **Запреты:** GPL-код в клиентах (FFmpeg и mpv — только LGPL-сборки, динамически); ассеты, тексты и бренд Infuse/Firecore, Fedora, Dolby/DTS, Apple SF вне Apple-платформ; секреты в git (только `.env.local` и GitHub Secrets); блокировка рекламы Twitch; телеметрия без opt-in.
11. **Заголовок лицензии** в каждом новом исходнике: `// SPDX-License-Identifier: MPL-2.0` (или комментарий в синтаксисе языка). Проверяется `reuse lint`.
12. **Язык:** документация и тексты для пользователя — русский и английский. Код, идентификаторы, коммиты и заголовки PR — английский.

**Definition of Done (для всех задач):** CI зелёный; тесты из `done when` есть и проходят; публичный API или поведение задокументированы (rustdoc, dartdoc, `docs/`); новые зависимости прошли `cargo deny` и лицензионную проверку; чекбокс отмечен; `just roadmap lint` проходит.

### 0.4. Целевая структура монорепо и владение каталогами

```
project_alpha/
├── core/crates/          # Rust-ядро: pa-* крейты (один Cargo workspace с server/ и tools/)
├── server/               # медиасервер (axum) + server/web (веб-админка сервера)
├── clients/
│   ├── flutter/          # Flutter-приложение: Android, Android TV, iOS, iPadOS, macOS, Windows, Linux
│   │   └── packages/     # pa_core (FRB), pa_player_android|apple|desktop (плагины плеера)
│   ├── android/          # Kotlin: Media3-плеер, MediaSession/Auto, TV-каналы
│   ├── apple/            # Swift: AlphaPlayerKit, TVApp (SwiftUI, tvOS), AudioKit, CarPlay
│   └── desktop/          # нативный хост libmpv (Win/Linux/macOS)
├── distro/               # NAS-ОС: Containerfile, installer (Go), admin (Go + web), updated (Go), tests
├── design/               # токены DTCG, иконки, бренд, иллюстрации (Git LFS)
├── packaging/            # flatpak, AUR, winget, MSIX, DMG, F-Droid, Docker, nfpm
├── config/               # публичные удалённые конфиги (twitch/remote-config.json и т. п.)
├── tools/                # ffmpeg-сборка, bindings, recognition-eval, roadmap-скрипты
├── tests/                # e2e, тестовые медиа (LFS), фикстуры
├── site/                 # сайт (GitHub Pages) — или отдельный репо по DEC-001
└── docs/                 # research, adr, api, dev, user, legal, STRATEGY, ROADMAP
```

| Workstream | Владеет каталогами |
|---|---|
| REPO | корневые конфиги (`.editorconfig`, `rust-toolchain.toml`, `mise.toml`, `.gitattributes`), `.github/` кроме `workflows/` и `FUNDING.yml`, `.devcontainer/`, `tools/roadmap/`, `CONTRIBUTING.md`, `CODE_OF_CONDUCT.md`, `SECURITY.md`, `AGENTS.md` |
| CI | `.github/workflows/`, `tools/ci/` |
| CORE | `core/crates/{pa-model,pa-runtime,pa-secrets,pa-log,pa-jobs,pa-ffi}`, `clients/flutter/packages/pa_core/`, `tools/bindings/` |
| NET | `core/crates/{pa-vfs,pa-proxy,pa-discovery,pa-remote}`, `third_party/{libsmb2,libnfs}/` |
| LIB | `core/crates/{pa-library,pa-scanner,pa-probe}` (вкл. `migrations/`) |
| META | `core/crates/{pa-recognition,pa-metadata}`, `tools/recognition-eval/`, `data/anime-mappings/` |
| PLAY | `core/crates/pa-playback`, `tools/ffmpeg/`, `third_party/{ffmpeg,mpv,libass}/` (скрипты и патчи) |
| PLAY-AND | `clients/android/player/`, `clients/flutter/packages/pa_player_android/` |
| PLAY-APL | `clients/apple/AlphaPlayerKit/`, `clients/flutter/packages/pa_player_apple/` |
| PLAY-DSK | `clients/desktop/`, `clients/flutter/packages/pa_player_desktop/` |
| UI | `clients/flutter/lib/` (кроме `lib/tv/`, `lib/features/music/`, `lib/features/twitch/`, `lib/design/generated/`), runner-проекты `clients/flutter/{android,ios,macos,windows,linux}/` |
| TV | `clients/flutter/lib/tv/`, `clients/apple/TVApp/` |
| SYNC | `core/crates/pa-sync`, `docs/api/sync-protocol.md` |
| SRV | `server/`, `docs/api/openapi.json` |
| MUS | `core/crates/pa-audio`, `clients/flutter/lib/features/music/`, `clients/android/audio/`, `clients/apple/AudioKit/` |
| TWI | `core/crates/pa-twitch`, `clients/flutter/lib/features/twitch/`, `config/twitch/` |
| DIST | `distro/` |
| DSN | `design/`, `clients/flutter/lib/design/generated/` (только генерация) |
| PKG | `packaging/` |
| QA | `tests/`, `tools/qa/` |
| DOC | `docs/{adr,dev,user}/` |
| LEGAL | `docs/legal/`, `LICENSES/`, `THIRD_PARTY_NOTICES*` |
| SITE | `site/` |
| COMM | `README*.md`, `.github/FUNDING.yml`, `.github/DISCUSSION_TEMPLATE/` |

### 0.5. Горячие файлы и правила их правки

| Файл | Правило |
|---|---|
| `Cargo.toml` (корень) | `members`: глобы `core/crates/*`, `tools/crates/*` и явные пути (`tools/recognition-eval`; `server` добавляется задачей, создающей `server/Cargo.toml`). Новый крейт в `core/crates/` или `tools/crates/` корень не трогает, крейт в другом месте — добавляет одну строку в `members`. Новая зависимость — одна строка в `[workspace.dependencies]`, по алфавиту. Конфликт решать объединением строк |
| `Cargo.lock` | Не править руками. При конфликте взять версию `main` и выполнить `cargo check --workspace` |
| `justfile` (корень) | Только строки `mod <area> '<path>/justfile'`. Рецепты — в justfile своей области |
| `core/crates/pa-ffi/src/lib.rs` | Каждый домен экспортируется своим модулем `src/<domain>.rs`. В `lib.rs` добавляется одна строка `mod` |
| `core/crates/pa-library/migrations/` | Имя файла: `YYYYMMDDHHMM_<ID>_<slug>.sql` — номера не конфликтуют. Только expand→contract (LIB-002) |
| `clients/flutter/pubspec.yaml` | Зависимости — по алфавиту, одна строка. Конфликт — объединение |
| `clients/flutter/lib/app/router.dart` | Каждая фича экспортирует `routes.dart`. В роутер добавляется одна строка |
| `clients/flutter/lib/l10n/*.arb` | Ключи с префиксом фичи (`player_`, `sources_`…), только добавление, сортировка по ключу. Конфликт — объединение |
| `docs/ROADMAP.md` | Только своя строка и новые задачи в конце своего раздела |

---

## 1. Решения владельца (блокеры)

Эти задачи делает только владелец. Пока они не закрыты, зависящие задачи не стартуют.

- [x] **DEC-001** (owner) Сделать `DenisHumen/project_alpha` публичным; решить судьбу `project_alpha-releases`
  - Что: публичный репо и релизы через GitHub Releases этого же репо; `project_alpha-releases` — сайт, архив или удаление.
  - deps: — · size: S · parallel: yes · веха: M0 · ref: S §7, §11.1
  - done when: решение записано в ADR (DOC-001); видимость репо переключается в REPO-011.
- [x] **DEC-002** (owner) Лицензия кода: MPL-2.0 (рекомендация) или GPL-3.0
  - Что: зафиксировать лицензию и DCO.
  - deps: — · size: S · parallel: yes · веха: M0 · ref: S §5
  - done when: ADR-0005 принят.
- [x] **DEC-003** (owner) Донаты в store-сборках: без ссылок или «чаевые» через IAP
  - Что: выбрать вариант для App Store, Google Play и Microsoft Store.
  - deps: — · size: S · parallel: yes · веха: M5 · ref: S §4
  - done when: решение записано в ADR; от него зависят PKG-015 и LEGAL-008.
- [x] **DEC-004** (owner) Название и бренд продукта (и имя NAS-ОС)
  - Что: выбрать имя по итогам LEGAL-005, без «Infuse», «Fedora» и «Twitch» в названии.
  - deps: LEGAL-005 · size: S · parallel: yes · веха: M1 · ref: S §11.4, 05 §7
  - done when: имя, домен и хэндлы закреплены; от решения зависят DSN-006 и DIST-003.
- [x] **DEC-005** (owner) Юридическая позиция по кодекам (H.264/HEVC/AAC, RPM Fusion, своя сборка FFmpeg на сервере)
  - Что: принять решение по итогам LEGAL-004.
  - deps: LEGAL-004 · size: S · parallel: yes · веха: M4 · ref: 05 §7, 02 §3.2
  - done when: ADR о составе FFmpeg на сервере и в дистрибутиве.
- [x] **DEC-006** (owner) Twitch без логина: публичные данные через GQL или только вход через Device Code Flow
  - Что: решить по итогам прототипа TWI-003.
  - deps: TWI-003 · size: S · parallel: yes · веха: M7 · ref: S §6
  - done when: ADR; от него зависят TWI-013 и TWI-014.
- [ ] **DEC-007** (owner) Аккаунты и юрисдикция: Apple Developer, Google Play, SignPath, Microsoft Store
  - Что: физлицо или юрлицо, сроки покупки Apple ($99/год — когда готовы ≥ 2 Apple-таргета).
  - deps: — · size: S · parallel: yes · веха: M1 · ref: S §8, 02 §6.2
  - done when: решение записано; от него зависят PKG-005, PKG-008, PKG-011, PKG-012.
- [x] **DEC-008** (owner) Ключ TMDB в открытом коде: встроенный ключ проекта по умолчанию или обязательный свой ключ
  - Что: решить по ответу TMDB (LEGAL-001).
  - deps: LEGAL-001 · size: S · parallel: yes · веха: M2 · ref: S §3, §5
  - done when: ADR; META-009 реализует выбранный вариант.

---

## 2. Вехи

Вехи идут не строго последовательно: после M0 полосы работают параллельно (§5). Веха закрыта, когда закрыты все её задачи и выполнены критерии выхода.

| Веха | Содержание | Критерии выхода |
|---|---|---|
| **M0. Фундамент** | Лицензия и governance, публичный репо, CI-матрица, Rust-ядро вызывается со всех платформ, ADR, LGPL-сборка FFmpeg, каркас дизайн-токенов | `core_version()` вызывается из smoke-приложений на iOS, iPadOS, tvOS, macOS, Android, Windows и Linux, и все они собираются в CI (CORE-010). CI зелёный на всех таргетах. ADR 0001–0007 приняты. `reuse lint`, DCO-проверка и `roadmap lint` обязательны для мержа. FFmpeg собирается как LGPL v2.1+, и это проверяется в CI |
| **M1. Плеер-MVP (режим A) на десктопе и Android** | Локальные папки, SMB, WebDAV → сканер → медиатека → базовое сопоставление с TMDB → нативный плеер на Linux, Windows, macOS (Flutter), Android и Android TV | Источник SMB или WebDAV с 10 000 файлов сканируется (поверхностно) быстрее 60 с по LAN. Grid и Detail работают со скоростью 60 fps. Воспроизведение через libmpv (десктоп) и Media3 (Android): SRT/ASS/PGS, смена дорожек, главы, resume, Up Next, trickplay, stats overlay. На Android TV: HDR10 и passthrough AC-3/E-AC-3 на реальном устройстве, AFR. QA-003 проходит на Linux, Windows и Android |
| **M2. Распознавание v1** | Парсер с N-best гипотезами, локальный индекс названий, TMDB/AniList/Shikimori, маппинги аниме, скорер, инбокс проверки и исправление матчей, корпус и метрики | Размечен корпус ≥ 5 000 групп. Eval идёт в CI на каждый PR. Тихих ошибок ≤ 1 % в целом и ≤ 2 % на аниме. На категориях A1–A4 и A7 ошибок минимум вдвое меньше, чем у бейзлайна Jellyfin. Исправление матча становится правилом папки (FolderRule) |
| **M3. Синхронизация** | HLC, outbox, P2P в LAN, синхронизация через папку SMB/WebDAV, профили, Trakt | Продолжение между двумя устройствами в одной LAN без сервера — с точностью ≤ 2 с, обновление ≤ 1 с. Синхронизация через папку сходится (property-тесты). Профили изолируют прогресс. Офлайн-изменения доходят после реконнекта |
| **M4. Медиасервер (режим B) и совместимость (режим C)** | Сервер на axum: скан, REST и WebSocket, direct play, remux и транскод с HW-ускорением, mDNS, iroh, очередь задач, веб-админка; клиенты Plex, Jellyfin и Emby в приложении | Docker-образ и бинарники для linux x64/arm64, Windows и macOS. Клиент находит сервер по mDNS и играет direct play. Транскод с VAAPI/QSV на Intel N100 даёт ≥ 2 потока 1080p. Удалённое воспроизведение через iroh (relay n0 или свой). Сервер работает хабом синхронизации. Plex, Jellyfin и Emby: вход, библиотеки, воспроизведение, двусторонняя синхронизация «просмотрено» |
| **M5. Apple-клиенты** | Flutter на iOS, iPadOS и macOS; tvOS на SwiftUI; MPVKit и AVPlayer → свой движок FFmpeg + VideoToolbox; HDR и DV | На Apple TV 4K и HDR-телевизоре работают HDR10, DV P5 и P8.1, P7 воспроизводится как P8.1, AFR (`AVDisplayCriteria`) работает, TrueHD декодируется в LPCM 7.1 с честной индикацией. iOS, iPadOS, macOS и tvOS выходят в TestFlight. Top Shelf работает |
| **M6. Музыка** | Библиотека, очередь, gapless, ReplayGain, LRC, mini player, системные медиаконтролы → свой движок, bit-perfect, DSD | Gapless без щелчков на тест-альбоме (проверка по сэмплам). ReplayGain track и album. LRC синхронизирован. Bit-perfect подтверждён на Windows (WASAPI exclusive), macOS (hog mode) и Linux (ALSA hw). DSD воспроизводится через DoP. Android Auto и CarPlay — P2, допустимо после M6 |
| **M7. Twitch** | Device Code Flow, фолловы, live и чат (7TV/BTTV/FFZ), VOD с главами, синхронизация; в full-сборках — загрузки, запись, чат-replay; kill-switch | На всех платформах: вход по QR, live с чатом и эмоутами, VOD с маркерами категорий, resume между устройствами по ключу `twitch:vod:<id>`. Full-сборки скачивают VOD и пишут live на сервере. Канареечный CI ночью зелёный. CI доказывает, что store-бинари не содержат кода архива |
| **M8. NAS-дистрибутив MVP** | Образ bootc, установщик на Go (2 экрана), мастер пулов, Samba, обновления с greenboot и откатом, веб-админка, медиасервер в образе | ISO меньше 1.9 GiB. В QEMU CI проходят установка, обновление N-1→N, автооткат плохого обновления и отказ от неподписанного образа. На реальных Intel N100 и AMD работает VAAPI. ISO опубликован на R2 и в GitHub Release с подписью |
| **M9. Дизайн в стиле tvOS** | Полировка дизайн-системы: фокус и параллакс, Liquid Glass на Apple, Glass с fallback, motion, консистентность на всех платформах | Golden-тесты ключевых экранов на 4 плотностях. Фокус-коэффициенты утверждены на реальном TV. Reduce Motion соблюдается. Blur автоматически выключается на слабых Android TV |
| **M10. Публичный 1.0** | Все каналы публикации, сайт, документация, юридические подтверждения | Опубликованы GitHub Releases, Flathub, AUR, winget, Microsoft Store, F-Droid, Google Play, App Store (iOS, iPadOS, macOS, tvOS), Docker и ISO. Есть письменный ответ TMDB. Сайт со страницей «Скачать» и документацией работает. Релизный чек-лист QA-008 пройден |

---

## 3. Задачи по workstream

### 3.1. REPO — репозиторий и governance

- [x] **REPO-001** Каркас монорепо
  - Что: каталоги по §0.4; корневой `Cargo.toml` (workspace, `members` по глобу, `[workspace.dependencies]`), `rust-toolchain.toml`, `.editorconfig`, `.gitignore`, `.gitattributes` (LFS для `design/**`, `tests/media/**`), корневой `justfile` только с `mod`-строками, `.env.example`.
  - deps: — · size: S · parallel: no · веха: M0 · ref: S §7, 04 §4.1
  - done when: `just --list` показывает модули; `cargo metadata` проходит на пустом workspace; `git lfs track` настроен.
- [x] **REPO-002** LICENSE, SPDX-заголовки и REUSE
  - Что: `LICENSE` (MPL-2.0 или по DEC-002), `LICENSES/`, `REUSE.toml` для сгенерированных и сторонних файлов, `reuse lint` в CI.
  - deps: DEC-002, REPO-001 · size: S · parallel: yes · веха: M0 · ref: S §5
  - done when: `reuse lint` зелёный и обязателен в CI.
- [x] **REPO-003** CONTRIBUTING, CODE_OF_CONDUCT, SECURITY
  - Что: CONTRIBUTING (DCO, Conventional Commits, worktree-процесс, ссылка на §0.3); Contributor Covenant 2.1; SECURITY.md с GitHub Private Vulnerability Reporting и сроками ответа.
  - deps: REPO-001 · size: S · parallel: yes · веха: M0 · ref: S §7
  - done when: файлы есть, GitHub показывает их во вкладке Community Standards.
- [x] **REPO-004** DCO-проверка и линт заголовка PR
  - Что: DCO GitHub App или workflow, который проверяет `Signed-off-by` у каждого коммита; `amannn/action-semantic-pull-request` со списком scope из §0.3; проверка `[ID]` в заголовке.
  - deps: REPO-003, CI-001 · size: S · parallel: yes · веха: M0 · ref: S §5, 04 §4.2
  - done when: PR без sign-off или с неверным заголовком красный; тестовый PR из форка проходит проверку.
- [ ] **REPO-005** (owner) CODEOWNERS, rulesets и окружения
  - Что: CODEOWNERS по таблице §0.4; ruleset для `main` (только PR, squash, обязательные проверки, запрет force-push, linear history); окружения `release` и `store` с required reviewer.
  - deps: REPO-004, CI-001 · size: S · parallel: yes · веха: M0 · ref: 04 §4.5
  - done when: прямой push в `main` отклоняется; релизный job ждёт подтверждения.
- [x] **REPO-006** Шаблоны issue и PR, метки как код
  - Что: issue-формы (bug, feature, playback-issue с полем для stats-отчёта, recognition-miss с анонимизированным путём); PR-шаблон (ID, тесты, платформы, чекбокс в ROADMAP, DCO); `.github/labels.yml` (`ws:*`, `M0..M10`, `size:*`, `flavor:store|full`, `good first issue`, `help wanted`) с синхронизацией через action.
  - deps: REPO-001 · size: S · parallel: yes · веха: M0 · ref: 01 §3 п.11
  - done when: метки созданы автоматически; новое issue открывается по форме.
- [x] **REPO-007** AGENTS.md и CLAUDE.md для агентов
  - Что: корневой AGENTS.md (как брать задачу, команды проверки, запреты из §0.3 п.10) плюс короткие AGENTS.md в `core/`, `clients/flutter/`, `clients/apple/`, `server/`, `distro/` с командами сборки и тестов области.
  - deps: REPO-001 · size: S · parallel: yes · веха: M0 · ref: 04 §4.1
  - done when: агент по одному AGENTS.md собирает и тестирует свою область на чистой машине.
- [ ] **REPO-008** Закрепить toolchain, `just bootstrap`, Dev Container
  - Что: `mise.toml` (Flutter/Dart, JDK 17, Go, Node, cargo-ndk), `.xcode-version`, `just bootstrap` для Linux, macOS и Windows; `.devcontainer/` для Linux-части (Rust, Android SDK/NDK, Flutter Linux, Go, podman).
  - deps: REPO-001 · size: M · parallel: yes · веха: M0 · ref: 04 §4.3
  - done when: `just bootstrap && just test` проходит на чистых Ubuntu 24.04, macOS 15 и Windows 11, а также в devcontainer.
- [x] **REPO-009** Политика секретов
  - Что: gitleaks (pre-commit и CI), GitHub secret scanning и push protection, `.env.example` со всеми переменными, документ «какие секреты где живут» (CI, окружения, OIDC).
  - deps: REPO-001 · size: S · parallel: yes · веха: M0 · ref: 04 §4.5, CLAUDE.md
  - done when: коммит с тестовым ключом блокируется локально и в CI.
- [ ] **REPO-010** release-please (manifest mode)
  - Что: компоненты `core`, `server`, `app`, `tvos`, `distro`; теги `app-vX.Y.Z`, `server-vX.Y.Z` и т. д.; build number для сторов — `github.run_number`; changelog по Conventional Commits.
  - deps: CI-001 · size: S · parallel: yes · веха: M0 · ref: S §7, 04 §4.2
  - done when: тестовый merge `feat:` создаёт release PR с корректным changelog.
- [ ] **REPO-011** (owner) Подготовка и переключение репо в публичный режим
  - Что: скан всей истории gitleaks, проверка на приватные ссылки и чужие ассеты, переключение видимости, судьба `project_alpha-releases` по DEC-001, редиректы в README.
  - deps: DEC-001, REPO-002, REPO-009 · size: S · parallel: yes · веха: M0 · ref: S §7
  - done when: репо публичный, в истории нет секретов, Actions на публичных runners работают.
- [x] **REPO-012** Инструменты ROADMAP: `just roadmap lint` и `just roadmap next`
  - Что: скрипт в `tools/roadmap/` (блоки кода игнорируются) проверяет уникальность ID, формат полей, существование ID в deps, отсутствие циклов, что у `[x]` нет открытых deps; `next` выдаёт задачи с закрытыми deps, у которых нет открытого PR (через `gh`).
  - deps: REPO-001 · size: S · parallel: yes · веха: M0 · ref: —
  - done when: lint в CI зелёный на этом файле; `next` на текущем состоянии выдаёт список первой волны (§6).
- [x] **REPO-013** Dependabot или Renovate
  - Что: обновления cargo, pub, gradle, swiftpm, gomod, github-actions; группировка по экосистемам; auto-merge только для patch-обновлений dev-зависимостей.
  - deps: CI-001 · size: S · parallel: yes · веха: M0 · ref: 04 §5.3
  - done when: приходят сгруппированные PR, и CI на них проходит.

### 3.2. CI — сборка и проверки

- [x] **CI-001** `rust.yml`: fmt, clippy, nextest
  - Что: `cargo fmt --check`, `clippy -D warnings`, `cargo nextest` на ubuntu; кэш `Swatinem/rust-cache`; `dorny/paths-filter`; агрегирующий job `all-green` для branch protection.
  - deps: REPO-001 · size: S · parallel: no · веха: M0 · ref: 04 §4.4
  - done when: PR, затрагивающий только `docs/`, не запускает Rust-сборку; `all-green` обязателен.
- [ ] **CI-002** Безопасный CI для PR из форков
  - Что: только `pull_request` (не `pull_request_target`) для кода из форков; `permissions: read-all` по умолчанию; actions закреплены по SHA; одобрение запуска для новых контрибьюторов; self-hosted runners никогда не берут PR; секреты только в окружениях.
  - deps: CI-001 · size: S · parallel: yes · веха: M0 · ref: 04 §4.4–4.5
  - done when: тестовый PR из форка не видит секретов; `zizmor` или `actionlint` проходит на всех workflow.
- [x] **CI-003** `cargo deny` и `cargo audit`
  - Что: deny-list лицензий (GPL, AGPL и SSPL запрещены в графе клиентских крейтов), advisories, запрет дубликатов основных крейтов.
  - deps: CI-001, REPO-002 · size: S · parallel: yes · веха: M0 · ref: 02 §3.1, 04 §5.3
  - done when: добавление GPL-крейта в `pa-ffi` роняет CI.
- [ ] **CI-004** Матрица Rust-таргетов и sccache
  - Что: x86_64 и aarch64 для linux-gnu и musl, windows-msvc, apple-darwin universal, aarch64-apple-ios и симулятор, aarch64-apple-tvos и симулятор (tier 3: закреплённый nightly с `-Zbuild-std`), Android arm64-v8a, armeabi-v7a и x86_64 через cargo-ndk; sccache в GitHub cache.
  - deps: CORE-001, CI-001 · size: M · parallel: no · веха: M0 · ref: 04 §4.4, 02 §1.4
  - done when: все таргеты собираются в CI, повторная сборка с кэшем быстрее холодной минимум в 2 раза.
- [ ] **CI-005** `flutter.yml`
  - Что: `flutter analyze`, format, unit и widget-тесты; сборки apk (debug), linux, windows, macos, ios (`--no-codesign`) для обоих флейворов.
  - deps: UI-001, CI-001 · size: M · parallel: no · веха: M0 · ref: 02 §7
  - done when: все пять сборок зелёные на PR, который трогает `clients/flutter/`.
- [ ] **CI-006** `android.yml`: нативные модули и эмулятор
  - Что: Gradle-сборка `clients/android/*`, JUnit, instrumented-тесты на эмуляторе Android TV (API 34) и телефона через `reactivecircus/android-emulator-runner`.
  - deps: CORE-004, CI-001 · size: M · parallel: yes · веха: M1 · ref: 02 §7
  - done when: instrumented smoke (открыть плеер, воспроизвести тестовый файл 5 с) зелёный на эмуляторе.
- [ ] **CI-007** `apple.yml` на macOS-runner
  - Что: сборка xcframework, тесты Swift packages, сборка tvOS-приложения и iOS/macOS runner-ов Flutter; запуск по paths и ночью.
  - deps: CORE-003, CI-001 · size: M · parallel: yes · веха: M0 · ref: 04 §4.4
  - done when: XCTest проходит на симуляторах iOS и tvOS.
- [ ] **CI-008** Кэш и раздача артефактов FFmpeg и libmpv
  - Что: сборки из PLAY-001 и PLAY-002 кэшируются по хэшу рецепта и публикуются как артефакты для остальных job; проверка `avutil_license()` = LGPL v2.1+.
  - deps: PLAY-001, PLAY-002 · size: S · parallel: yes · веха: M1 · ref: 02 §3.1
  - done when: job клиента берёт готовый FFmpeg из кэша; сборка с `--enable-gpl` роняет CI.
- [ ] **CI-009** Golden-скриншоты Flutter по плотностям
  - Что: golden-тесты ключевых экранов в 4 плотностях (phone, tablet, desktop, TV) и двух темах; diff прикладывается артефактом к PR.
  - deps: CI-005, DSN-003 · size: S · parallel: yes · веха: M1 · ref: 01 §4.10 п.5
  - done when: изменение токена цвета даёт видимый diff в PR.
- [ ] **CI-010** Eval распознавания на каждый PR
  - Что: при изменениях в `pa-recognition` или `pa-metadata` прогон dev-сплита корпуса (record/replay, без сети), комментарий в PR с diff метрик по категориям A1–A17 и списком новых ошибок.
  - deps: META-003, META-004 · size: S · parallel: yes · веха: M2 · ref: 03 §6.4
  - done when: регресс тихих ошибок больше 0.2 п. п. роняет проверку.
- [ ] **CI-011** Nightly и релизный пайплайн
  - Что: nightly-сборки всех платформ (артефакты хранятся 7 дней); по тегу release-please — матрица, затем SHA256SUMS, подписи cosign или minisign, SBOM (`cargo cyclonedx`, syft) и GitHub Release через окружение `release`.
  - deps: REPO-010, PKG-001, CI-004, CI-005, CI-007 · size: M · parallel: no · веха: M1 · ref: 04 §4.6
  - done when: тестовый тег `app-v0.0.1-rc.1` даёт черновик релиза со всеми артефактами и подписями.
- [ ] **CI-012** Сетевые интеграционные тесты
  - Что: docker-compose с Samba (SMB2/3, signing, encryption), NFS-ganesha, WebDAV (nginx, Apache, `rclone serve`), SFTP и FTP; деградация сети через `tc netem`; общий тест-сьют `pa-vfs`.
  - deps: NET-001 · size: M · parallel: yes · веха: M1 · ref: 01 §1.5, 02 §4
  - done when: контракт-тесты Vfs запускаются против всех серверов в CI.

### 3.3. CORE — ядро на Rust, FFI, общие сервисы

- [x] **CORE-001** `pa-ffi`: каркас UniFFI
  - Что: крейт `pa-ffi` с UniFFI proc-macro, функция `core_version()`, генерация Kotlin и Swift через `just bindings` (сгенерированное не коммитится, кроме требований Xcode).
  - deps: REPO-001 · size: S · parallel: no · веха: M0 · ref: 02 §1.4, 04 §1.3
  - done when: unit-тест FFI, биндинги Kotlin и Swift генерируются в CI.
- [x] **CORE-002** flutter_rust_bridge: пакет `pa_core`
  - Что: `clients/flutter/packages/pa_core` с FRB codegen, async-функции и Stream; загрузка нативной библиотеки на 5 Flutter-платформах.
  - deps: CORE-001 · size: M · parallel: no · веха: M0 · ref: 02 §1.4
  - done when: Dart-тест вызывает `core_version()` и подписывается на тестовый Stream.
- [x] **CORE-003** Apple: xcframework и SwiftPM `PACore`
  - Что: `tools/bindings/build-xcframework.sh` (ios, ios-sim, tvos, tvos-sim, macos universal), binary target и Swift-обёртка UniFFI.
  - deps: CORE-001 · size: M · parallel: yes · веха: M0 · ref: 02 §1.4, 04 §4.4
  - done when: XCTest на симуляторах iOS и tvOS вызывает `core_version()`.
- [x] **CORE-004** Android: AAR `pa-core-android`
  - Что: cargo-ndk → jniLibs (arm64-v8a, armeabi-v7a, x86_64), Gradle-модуль с UniFFI Kotlin.
  - deps: CORE-001 · size: S · parallel: yes · веха: M0 · ref: 02 §1.4
  - done when: instrumented-тест на эмуляторе вызывает ядро.
- [x] **CORE-005** `pa-model`: доменные типы
  - Что: MediaItem, Work, Season, Episode, MediaFile, Track, Album, Artist, Streamer, Video; `MediaKey` (`file:<fp>`, `tmdb:…`, `twitch:vod:<id>`, `twitch:rec:<id>`, `twitch:clip:<slug>`); `PlaybackItem` (source_type, position, duration, updated_at, device_id); serde и версия схемы.
  - deps: REPO-001 · size: M · parallel: no · веха: M0 · ref: 04 §1.8, 06 §5.2, 01 §2.7
  - done when: round-trip-тесты serde; `MediaKey` парсится и печатается без потерь.
- [x] **CORE-006** `pa-runtime`: жизненный цикл и конфигурация
  - Что: tokio-runtime в ядре, init/shutdown/foreground/background, каталоги данных по платформам (на tvOS — Application Support и Caches), колбэк-интерфейсы платформы, настройки (KV в SQLite, миграции, экспорт и импорт).
  - deps: CORE-001, CORE-005 · size: M · parallel: no · веха: M0 · ref: 02 §4, 04 §1.4 п.5
  - done when: тесты жизненного цикла; повторный init после shutdown работает; настройки переживают перезапуск.
- [x] **CORE-007** `pa-secrets`: хранилище секретов
  - Что: трейт SecretStore. Десктоп — крейт `keyring` (DPAPI, Keychain, Secret Service; без них — зашифрованный файл и предупреждение). Мобильные и tvOS — UniFFI-колбэк в Keychain (`AfterFirstUnlock`) и Android Keystore + Tink. Маскирование секретов в логах.
  - deps: CORE-006 · size: M · parallel: yes · веха: M1 · ref: 04 §5.1
  - done when: интеграционные тесты на Linux, Windows и macOS; на Android и iOS — instrumented-тест и XCTest.
- [ ] **CORE-008** `pa-log`, диагностический отчёт, crash-отчёты по opt-in
  - Что: tracing с ротацией, вырезание путей и токенов; экспорт zip-отчёта (логи, версии, конфиг без секретов); перехват panic и сбор нативных крэш-логов локально; отправка — только вручную пользователем (заготовка GitHub issue), без серверов.
  - deps: CORE-006 · size: M · parallel: yes · веха: M1 · ref: 01 §1.5 (приватность), §2.5 п.4, S §1
  - done when: тест проверяет, что в отчёте нет путей пользователя и токенов; без opt-in ничего не сохраняется.
- [ ] **CORE-009** Шина событий и модель ошибок
  - Что: стрим событий ядра в UI (library, scan, jobs, sync, playback) через FRB Stream и UniFFI callback; стабильные коды ошибок, текст сообщения формирует клиент через l10n.
  - deps: CORE-006, CORE-002 · size: S · parallel: no · веха: M0 · ref: 04 §1.5
  - done when: Dart-тест получает события скана; у каждой публичной ошибки есть код.
- [ ] **CORE-010** Smoke-приложения M0 на 7 платформах
  - Что: Flutter (Android, iOS, iPadOS, macOS, Windows, Linux) и tvOS (TV-001) показывают `core_version()` и собираются в CI.
  - deps: CORE-002, CORE-003, CORE-004, UI-001, TV-001, CI-004, CI-005, CI-007 · size: S · parallel: yes · веха: M0 · ref: S §9 этап 0, 04 §6 п.1
  - done when: скриншоты со всех платформ в PR; все job CI зелёные.
- [ ] **CORE-011** `pa-jobs`: персистентная очередь задач
  - Что: таблица `jobs`, состояния queued → waiting_resources → running → paused/retrying → done/failed/cancelled, приоритеты (live-запись выше всего), лимиты параллельности по типу, backoff, пауза по сети, батарее и Wi-Fi-only, события прогресса.
  - deps: CORE-006, LIB-001 · size: M · parallel: yes · веха: M1 · ref: 04 §1.10
  - done when: задача переживает kill процесса и докачивается; property-тест переходов состояний.
- [ ] **CORE-012** `pa-storage`: места хранения, квоты, retention
  - Что: storage_targets (тип, квота, резерв свободного места), оценка размера перед стартом, мониторинг и пауза задач при пересечении резерва, политики «хранить N / X дней / просмотренные через Y», корзина на 7 дней, атомарный перенос tmp → rename, чистка tmp при старте.
  - deps: CORE-011 · size: M · parallel: yes · веха: M4 · ref: 04 §1.10
  - done when: интеграционные тесты квот и retention на временной ФС.
- [ ] **CORE-013** Флейворы сборки `store` и `full`
  - Что: cargo features `store`/`full`, `twitch-archive`, `donation-links`; константа флейвора во всех клиентах (dart-define, Gradle flavors, Xcode configs); CI-тест, что store-бинарь не содержит символов и строк архива Twitch и донат-ссылок.
  - deps: CORE-006 · size: M · parallel: no · веха: M1 · ref: S §4, §6; 06 §2.7 п.4
  - done when: CI проверяет store-артефакты всех платформ (`nm` / `strings`) и падает при находке.
- [ ] **CORE-014** Клиент удалённого конфига без серверов
  - Что: загрузка подписанного JSON из публичного репо (`config/`, через GitHub Pages или raw), проверка подписи minisign встроенным ключом, ETag, кэш, откат на встроенную копию, флаги kill-switch.
  - deps: CORE-006 · size: S · parallel: yes · веха: M2 · ref: S §3 (Twitch), 06 §2.6
  - done when: тесты: неверная подпись отклоняется; офлайн берётся последняя валидная копия.
- [ ] **CORE-015** Платформенные хранилища секретов: Android Keystore и Apple Keychain
  - Что: реализации колбэка `PlatformSecretStore` из CORE-007 — Android (Keystore-ключ + Tink AEAD поверх приватного DataStore) и iOS/iPadOS/tvOS (Keychain, `kSecAttrAccessibleAfterFirstUnlock`); вызов `secrets_open` при старте приложения; доступ к секретам из Flutter (`pa_core` через FRB) на мобильных.
  - deps: CORE-007 · size: M · parallel: yes · веха: M1 · ref: 04 §5.1, docs/dev/secrets.md
  - done when: instrumented-тест на эмуляторе Android и XCTest на симуляторе iOS: запись, чтение, перезапись, удаление, переживает перезапуск процесса.

### 3.4. NET — источники, сетевые протоколы, Plex/Jellyfin/Emby

- [x] **NET-001** `pa-vfs`: трейт Vfs и локальная ФС
  - Что: `Vfs` (list, stat, open → `ByteSource{read_at, size}`), `LocalFs`, общий контракт-тест-сьют, который обязаны проходить все бэкенды.
  - deps: REPO-001, CORE-005 · size: S · parallel: no · веха: M1 · ref: 02 §1.4, 04 §1.3
  - done when: контракт-тесты проходят на LocalFs под Linux, Windows и macOS.
- [ ] **NET-002** libsmb2: сборка и `smb2-sys`
  - Что: вендоринг libsmb2 (LGPL-2.1), сборка динамической библиотеки для всех таргетов (на iOS и tvOS — embedded framework), безопасная Rust-обёртка.
  - deps: NET-001, CI-004 · size: M · parallel: yes · веха: M1 · ref: 02 §3.1, §4
  - done when: на всех таргетах бинарь линкуется динамически (проверка `otool`/`ldd`/`dumpbin`).
- [ ] **NET-003** SMB2/3 бэкенд
  - Что: NTLMSSP, подпись и шифрование SMB3 (`smb3_set_seal`), листинг, чтение с read-ahead и параллельным prefetch (буфер 32–256 МБ, настраиваемый), SMB1 только по явному флагу с предупреждением.
  - deps: NET-002, CI-012 · size: L · parallel: yes · веха: M1 · ref: 02 §4, 04 §5.2
  - done when: контракт-тесты против Samba в CI; поток 80 Мбит/с держится при netem-профиле «Wi-Fi» (30 мс, 1 % потерь).
- [ ] **NET-004** WebDAV бэкенд
  - Что: reqwest + quick-xml (PROPFIND), Basic и Digest, Range, редиректы, самоподписанные сертификаты с TOFU.
  - deps: NET-001, CI-012 · size: M · parallel: yes · веха: M1 · ref: 01 §1.5, 02 §4
  - done when: контракт-тесты против nginx, Apache, `rclone serve` и Nextcloud.
- [ ] **NET-005** NFS v3/v4 бэкенд (libnfs) [P1]
  - Что: libnfs (LGPL, динамически), userspace-клиент без mount.
  - deps: NET-001, CI-004 · size: M · parallel: yes · веха: M1 · ref: 02 §4
  - done when: контракт-тесты против NFS-ganesha (v3 и v4).
- [ ] **NET-006** SFTP и FTP/FTPS бэкенды [P1]
  - Что: russh + russh-sftp (host key TOFU), suppaftp (REST для seek).
  - deps: NET-001 · size: M · parallel: yes · веха: M1 · ref: 02 §4
  - done when: контракт-тесты против OpenSSH и vsftpd.
- [ ] **NET-007** `pa-proxy`: loopback HTTP Range-сервер
  - Что: `127.0.0.1:<port>` со случайным токеном в пути, отдаёт любой ByteSource с поддержкой Range; лимиты соединений; запасной путь для AVPlayer и ExoPlayer.
  - deps: NET-001 · size: S · parallel: yes · веха: M1 · ref: 02 §1.4, 04 §1.2
  - done when: ffprobe и curl читают файл через прокси; запрос без токена получает 403.
- [ ] **NET-008** Адаптеры ByteSource для FFmpeg и mpv
  - Что: FFmpeg `AVIOContext` (read/seek) и mpv `stream_cb` (C ABI) поверх ByteSource.
  - deps: NET-001, PLAY-001 · size: M · parallel: yes · веха: M1 · ref: 02 §4
  - done when: тест: libavformat открывает MKV с WebDAV через адаптер; mpv играет `pa://<id>`.
- [x] **NET-009** mDNS/DNS-SD обнаружение
  - Что: `mdns-sd` для `_smb._tcp`, `_webdav._tcp`, `_nfs._tcp`, `_projectalpha._tcp`; на iOS и tvOS — NWBrowser через колбэк; Info.plist (`NSLocalNetworkUsageDescription`, `NSBonjourServices`); Android MulticastLock.
  - deps: CORE-006 · size: M · parallel: yes · веха: M1 · ref: 02 §4, 04 §1.6, ADR-0019
  - done when: тестовый Samba с Avahi находится на Linux, Android и iOS-симуляторе. Сделано в ядре (`pa-discovery`, колбэки `PlatformBrowser` и `MulticastLock`), проверено на Linux; обвязка и проверка на Android и iOS — NET-020.
- [ ] **NET-010** WS-Discovery, NetBIOS, Plex GDM, Jellyfin UDP
  - Что: обнаружение Windows-шар (WS-Discovery, NetBIOS NS), Plex (UDP 32414), Jellyfin (UDP 7359); multicast на Apple — только после выдачи entitlement (PKG-012).
  - deps: NET-009 · size: M · parallel: yes · веха: M4 · ref: 02 §4, 04 §1.6
  - done when: в LAN-тесте находятся шара Windows и сервер Jellyfin в Docker.
- [ ] **NET-011** Менеджер источников
  - Что: модель источника (тип, адрес, креды в SecretStore, опции, «сканировать в библиотеку», выключение автоскана), статус (онлайн, последний скан, ошибки), тест скорости до источника.
  - deps: NET-001, CORE-007, LIB-001 · size: M · parallel: no · веха: M1 · ref: 01 §2.5 п.3–4, 04 §1.4
  - done when: CRUD-тесты; пароль не попадает ни в SQLite, ни в логи.
- [ ] **NET-012** Plex: вход, серверы, библиотеки
  - Что: plex.tv PIN-вход (код на TV), выбор подключения (local, remote, relay), пользователи Plex Home, библиотеки и метаданные в общей модели.
  - deps: NET-011 · size: M · parallel: yes · веха: M4 · ref: 01 §1.5, 04 §1.5
  - done when: интеграционный тест против Plex Media Server в Docker (claim-токен в секретах CI) или записанные ответы.
- [ ] **NET-013** Plex: воспроизведение, прогресс, транскод по запросу
  - Что: URL direct play, timeline и scrobble, двусторонняя синхронизация «просмотрено» и Continue Watching, плейлисты, транскод по долгому нажатию на Play.
  - deps: NET-012, PLAY-003 · size: M · parallel: yes · веха: M4 · ref: 01 §1.5
  - done when: прогресс, выставленный в нашем клиенте, виден в веб-интерфейсе Plex, и наоборот.
- [ ] **NET-014** Jellyfin: клиент
  - Что: клиент по OpenAPI, Quick Connect, пользователи, библиотеки, Continue Watching (включая удаление из него), избранное, вторичные адреса, отчёты о воспроизведении, DirectPlay/DirectStream/Transcode HLS, trickplay.
  - deps: NET-011, PLAY-003 · size: L · parallel: yes · веха: M4 · ref: 01 §1.5
  - done when: интеграционные тесты против Jellyfin в Docker (фиксированная версия).
- [ ] **NET-015** Emby: клиент
  - Что: на базе Jellyfin-клиента с адаптером различий API.
  - deps: NET-014 · size: M · parallel: yes · веха: M4 · ref: 01 §1.5
  - done when: тесты против Emby в Docker.
- [ ] **NET-016** Subsonic/OpenSubsonic клиент [P1]
  - Что: артисты, альбомы, треки, стриминг, scrobble; совместимость с Navidrome.
  - deps: NET-011 · size: M · parallel: yes · веха: M6 · ref: 01 §1.5 (музыка)
  - done when: тесты против Navidrome в Docker.
- [ ] **NET-017** Облачные хранилища [P1]
  - Что: сначала Yandex.Disk (WebDAV или REST), затем Google Drive, OneDrive и Dropbox через OAuth с PKCE (public client, без client secret в коде; если провайдер требует секрет — свой client_id пользователя).
  - deps: NET-004 · size: L · parallel: yes · веха: M4 · ref: 01 §1.5
  - done when: контракт-тесты Vfs против тестовых аккаунтов (вручную, по чек-листу) и моков.
- [ ] **NET-018** Прочие источники [P2]: UPnP/DLNA, `.strm`, M3U/M3U8
  - Что: UPnP ContentDirectory browse; разбор `.strm` и плейлистов M3U как источников.
  - deps: NET-011 · size: M · parallel: yes · веха: M10 · ref: 01 §1.5
  - done when: тест против minidlna в Docker; M3U с HTTP-ссылками воспроизводится.
- [ ] **NET-019** Офлайн-загрузки из источника на устройство [P1]
  - Что: копирование файла через pa-jobs с докачкой по offset; на iOS для SMB/NFS — только в foreground или `BGProcessingTask`; Android — WorkManager + foreground service `dataSync`; на tvOS загрузок нет.
  - deps: NET-001, CORE-011 · size: M · parallel: yes · веха: M4 · ref: 04 §1.9, 02 §4
  - done when: загрузка 4 ГБ с SMB докачивается после обрыва сети; прогресс приходит в UI.

- [ ] **NET-020** Обвязка обнаружения в LAN на Android и Apple
  - Что: Android — `MulticastLock` (`CHANGE_WIFI_MULTICAST_STATE`) как реализация `pa_discovery::MulticastLock`; iOS и tvOS — `NWBrowser` как `PlatformBrowser`, `Info.plist` (`NSLocalNetworkUsageDescription`, `NSBonjourServices` из `bonjour_services()`); экспорт через FFI.
  - deps: NET-009, CORE-002 · size: S · parallel: yes · веха: M1 · ref: ADR-0019, 02 §4
  - done when: тестовый Samba с Avahi находится на Android и в iOS-симуляторе.

### 3.5. LIB — медиатека: БД, сканер, поиск, прогресс

- [x] **LIB-001** Схема SQLite v1 и миграции
  - Что: File, FileGroup, FileGroupMember, Work, ExternalId, Title, Season, Episode, EpisodeMapping, AnimeEntry, Match, MatchCandidate, FolderRule, DisplayPreference, ProviderCache, Profile, Progress, Favorite, Collection, Playlist, jobs; WAL, foreign keys; refinery-миграции; одна схема для клиента и сервера.
  - deps: CORE-005 · size: M · parallel: no · веха: M1 · ref: 03 §5, 04 §1.3, §1.5
  - done when: миграция с нуля проходит; тест целостности FK; схема задокументирована в `docs/dev/db.md`.
- [ ] **LIB-002** Политика миграций expand→contract
  - Что: правило «обратная совместимость на одну версию» (нужно для отката сервера и дистрибутива), фикстуры БД всех прошлых версий, тест «новая БД + старый код читает».
  - deps: LIB-001 · size: S · parallel: yes · веха: M1 · ref: 05 §4.2 п.6
  - done when: CI прогоняет миграции на фикстурах всех выпущенных версий.
- [x] **LIB-003** `pa-scanner`: поверхностный инкрементальный скан
  - Что: обход через Vfs, запись (path, size, mtime), сравнение для инкрементального прохода, мягкое удаление пропавших с grace-периодом, события прогресса, отмена.
  - deps: LIB-001, NET-001 · size: M · parallel: no · веха: M1 · ref: 04 §1.4
  - done when: 10 000 файлов на Samba в CI сканируются меньше 60 с; повторный скан без изменений — меньше 10 с.
- [ ] **LIB-004** Отпечатки файлов: OSHash и ed2k
  - Что: OSHash (размер + 64 КБ в начале и конце, 2 range-запроса) для всех файлов; ed2k (MD4, блоки по 9 500 КБ) — фоновой задачей, по умолчанию только на сервере и десктопе.
  - deps: LIB-003 · size: S · parallel: yes · веха: M1 · ref: 03 §4.2 [0], [2]
  - done when: тест-векторы OSHash и ed2k совпадают с эталонными реализациями.
- [ ] **LIB-005** Watcher изменений
  - Что: `notify` (inotify, FSEvents, ReadDirectoryChangesW) с дебаунсом для локальных путей; SMB2 CHANGE_NOTIFY там, где поддерживается; для остальных сетевых источников — таймер, скан при открытии и pull-to-refresh.
  - deps: LIB-003 · size: M · parallel: yes · веха: M1 · ref: 04 §1.4 п.3, §1.5
  - done when: новый файл в локальной папке появляется в библиотеке меньше чем за 5 с.
- [ ] **LIB-006** `pa-probe`: анализ контейнера
  - Что: libavformat по первым 2–8 МБ: контейнер, длительность; видео — кодек, разрешение, тип HDR (HDR10, HDR10+, HLG, DV: профиль и compat id); аудио — кодек, каналы, язык, признак Atmos/JOC; субтитры, главы, теги, вложения.
  - deps: LIB-003, PLAY-001 · size: M · parallel: yes · веха: M1 · ref: 03 §4.2 [2], 02 §2.3
  - done when: на тест-сэмплах QA-002 все поля определяются верно.
- [ ] **LIB-007** Структурный анализ папок
  - Что: дисковые структуры (BDMV, VIDEO_TS, ISO) как один элемент; склейка частей (cd1/part1/disc1); extras (`-trailer`, Featurettes, NCOP/NCED, sample, «файл меньше N % длительности соседей»); sidecar-субтитры (`movie.ru.forced.srt`); NFO; локальный арт (poster, fanart, clearlogo).
  - deps: LIB-003 · size: M · parallel: yes · веха: M1 · ref: 03 §4.2 [1], 01 §1.5
  - done when: таблица тест-кейсов A10, A12 и A13 из таксономии проходит.
- [ ] **LIB-008** Поиск FTS5
  - Что: индекс фильмов, сериалов, эпизодов, людей, коллекций, Twitch-VOD и стримеров, музыки; ранжирование; нормализация с транслитом; группировка результатов.
  - deps: LIB-001 · size: M · parallel: yes · веха: M1 · ref: 01 §1.5, 06 §5.3
  - done when: запрос «ирония судьбы» находит `Ironiya.sudby…`; поиск по 50 000 элементам быстрее 50 мс.
- [ ] **LIB-009** Запросы библиотеки: фильтры, сортировки, виды
  - Что: комбинируемые фильтры (жанр, год, просмотрено, разрешение, HDR, аудиоформат, источник), сортировки, сохранение сортировки отдельно для каждого вида, «умные полки» (сохранённые фильтры, закрепляемые на Home), алфавитный индекс, опция «папки сверху».
  - deps: LIB-001 · size: M · parallel: yes · веха: M1 · ref: 01 §3 п.5
  - done when: тесты комбинаций фильтров; ответ меньше 30 мс на 20 000 элементов.
- [ ] **LIB-010** Версии, коллекции, избранное, плейлисты
  - Что: Smart Groups (несколько файлов одного Work: 4K, 1080p, Director's Cut) и версия по умолчанию; коллекции TMDB и пользовательские [P1]; избранное и закрепление на Home; плейлисты видео [P1].
  - deps: LIB-001 · size: M · parallel: yes · веха: M1 · ref: 01 §1.5
  - done when: два файла одного фильма показываются одной карточкой с выбором версии.
- [x] **LIB-011** Прогресс, история, «Продолжить», Up Next
  - Что: запись PlaybackItem, правило «в процессе» (от 2–3 % до 90–95 %, пороги настраиваются), «просмотрено» монотонно, запрос ряда «Продолжить» по `updated_at`, следующий эпизод для Up Next, «Убрать из ряда».
  - deps: LIB-001 · size: M · parallel: yes · веха: M1 · ref: 01 §2.7
  - done when: тесты порогов и Up Next на границе сезонов и для спешлов.
- [ ] **LIB-012** Профили и фильтры родительского контроля (данные)
  - Что: Profile (аватар, хэш PIN на Argon2, детский, лимит рейтинга, белый список библиотек и папок, политика «нет рейтинга — скрыто», скрытие арта); все запросы библиотеки учитывают профиль.
  - deps: LIB-001 · size: M · parallel: yes · веха: M1 · ref: 01 §3 п.2, п.6
  - done when: детский профиль не видит ни одного элемента вне белого списка ни через один API (поиск, «Продолжить», файлы).
- [ ] **LIB-013** Кэш изображений
  - Что: дисковый LRU с лимитом, декодирование и ресайз вне UI-потока, AVIF и WebP, размеры под плотности; на tvOS — в Caches с переживанием очистки.
  - deps: CORE-006 · size: S · parallel: yes · веха: M1 · ref: 04 §1.4 п.4
  - done when: лимит соблюдается; при прокрутке сетки 10 000 постеров нет пропусков кадров из-за декодирования.
- [ ] **LIB-014** Экспорт и импорт знаний: NFO и ID в имени [P1]
  - Что: по команде пользователя записать `.nfo` (формат Kodi/Jellyfin) или переименовать с `{tmdb-…}`; импорт NFO при скане.
  - deps: LIB-007, META-017 · size: M · parallel: yes · веха: M2 · ref: 03 §4.6
  - done when: экспортированный NFO читается Jellyfin в Docker-тесте.
- [ ] **LIB-015** Импорт медиатеки pa_library v0 (релиз 0.1)
  - Что: перенос базы Dart-пакета `pa_library` v0 в схему v1 по таблице соответствия из `docs/dev/db.md` §«Переход с pa_library v0»: источники, файлы, произведения, эпизоды, матчи, прогресс по `media_key`; старый файл только на чтение, после успеха — `*.v0.bak`.
  - deps: LIB-001, LIB-004, LIB-011 · size: M · parallel: yes · веха: M1 · ref: ADR-0013
  - done when: тест на фикстуре v0: все элементы и прогресс распознанных видны в v1, повторный запуск ничего не дублирует.

- [x] **LIB-016** Убрать уязвимый SQLite: refinery → rusqlite_migration
  - Что: refinery 0.10 держит rusqlite ≤0.40 → libsqlite3-sys 0.38 с уязвимым встроенным SQLite (RUSTSEC-2026-0459, CVE-2026-119375; фикс в libsqlite3-sys ≥0.39). Заменить refinery на rusqlite_migration (или свой раннер по `PRAGMA user_version`) и поднять rusqlite до ≥0.42 во всём workspace.
  - deps: LIB-001 · size: S · parallel: yes · веха: M1 · ref: docs/dev/db.md
  - done when: `cargo deny check advisories` зелёный; тесты pa-library и pa-runtime проходят; миграции v1 применяются на новой и существующей БД.
### 3.6. META — распознавание и метаданные

- [x] **META-001** Таксономия, гайд по разметке, формат корпуса
  - Что: отдельный репо `project_alpha-recognition-eval` (данные со смешанными лицензиями в продукт не попадают); категории A1–A17; JSONL-формат (дерево путей, длительность, языки, oshash; разметка: kind, ID, S/E в двух схемах, тип эпизода, extra_type, part, edition); GUIDE.md.
  - deps: — · size: S · parallel: yes · веха: M0 · ref: 03 §1.2, §6.1
  - done when: схема JSONL с валидатором; 50 примеров по всем категориям размечены.
- [x] **META-002** Корпус v0: 2 000 групп
  - Что: публичные RSS-заголовки аниме-релизов, тест-кейсы guessit, anitomy и Sonarr (только как eval-данные), русские релизы, синтетика для A11–A13; сплиты train, dev и закрытый test; git-lfs или DVC.
  - deps: META-001 · size: L · parallel: yes · веха: M2 · ref: 03 §6.1
  - done when: ≥ 2 000 групп, минимум 50 на категорию; валидатор зелёный.
- [x] **META-003** Eval-раннер `pa-eval`
  - Что: прогон пайплайна в режиме record/replay ответов провайдеров (офлайн, детерминированно); метрики: Precision@auto, Coverage, Recall, точность эпизода по схемам, тихие ошибки, ECE, нагрузка на пользователя, производительность; разрез по категориям; отчёт в Markdown и JSON.
  - deps: META-001, REPO-001 · size: M · parallel: yes · веха: M2 · ref: 03 §6.2
  - done when: отчёт по корпусу v0 воспроизводится побайтно при повторном запуске.
- [ ] **META-004** Бейзлайны
  - Что: guessit + наивный поиск TMDB, hunch + TMDB, Jellyfin в Docker (только TMDB, затем с AniDB-плагином) на дереве mkv-заглушек (ffmpeg генерирует нужные длительность и языки).
  - deps: META-002, META-003 · size: M · parallel: yes · веха: M2 · ref: 03 §6.3
  - done when: отчёт «где все проваливаются» по категориям лежит в eval-репо.
- [x] **META-005** Парсер имён: токенизатор и N-best гипотезы
  - Что: форк или вендоринг hunch (MIT) в `pa-recognition/parse`; токенизатор (скобки `[] () {} 【】`, разделители) → классификаторы токенов → `ParsedName` с весами; без кода под LGPL и GPL.
  - deps: REPO-001 · size: L · parallel: yes · веха: M1 · ref: 03 §2.2, §4.2 [3]
  - done when: ≥ 85 % на тест-кейсах guessit (как eval); защита от ReDoS — только линейные регулярки.
- [x] **META-006** Правила для аниме и русских релизов
  - Что: правила в духе anitomy, написанные заново (группа релиза, CRC32, `v2`, батчи, абсолютные номера, `.5`, NCOP/NCED); словари русских студий озвучки и трекеров; двойные названия «Рус / Orig (год)»; «Сезон N, серии 1–8 из 10».
  - deps: META-005 · size: M · parallel: yes · веха: M2 · ref: 03 §1.2 (A1, A4, A6, A7), §3.3
  - done when: поля категорий A1, A4, A6 и A7 корпуса разбираются с точностью ≥ 95 %.
- [x] **META-007** Нормализация и локальный индекс названий
  - Что: NFKC, casefold, варианты ромадзи (`ou`/`ō`/`o`), транслит кириллицы в обе стороны (ГОСТ и бытовой), удаление артиклей; индекс n-gram + BM25 + fuzzy (tantivy или свой) по названиям AniDB titles dump, anime-offline-database, Shikimori и горячему кэшу TMDB.
  - deps: META-005 · size: L · parallel: yes · веха: M2 · ref: 03 §4.2 [4]
  - done when: на корпусе ≥ 90 % верных кандидатов попадают в топ-10 без внешних запросов.
- [x] **META-008** Встроенные маппинги аниме
  - Что: по `docs/legal/anime-mappings.md` — в бандле только производная база из anime-offline-database (ODbL, с атрибуцией, сама под ODbL, воспроизводимая сборка `data/anime-mappings/build.sh`); Anime-Lists XML и Fribb `anime-list-full.json` (без лицензии) не встраиваются и не зеркалятся — приложение скачивает их с upstream прямо на устройство (зашитые URL, ETag, не чаще раза в сутки, проверка целостности и SHA-256, локальный кэш, атомарная замена); слияние слоёв только в памяти устройства; резервный форк — только для anime-offline-database.
  - deps: REPO-001, LEGAL-003 · size: M · parallel: yes · веха: M2 · ref: S §3, 03 §3.2, §9 п.3
  - done when: офлайн-тест маппинга AniDB ↔ TVDB ↔ TMDB на рукописных фикстурах (без копий Anime-Lists/Fribb); обновление применяется атомарно; в репозитории и бандле нет данных без лицензии.
- [x] **META-009** Трейт Provider и клиент TMDB
  - Что: сетевой слой вне sans-IO ядра; TMDB: поиск, детали, переводы ru-RU, альтернативные названия, episode groups, external ids, изображения, коллекции; rate limit, ProviderCache с TTL; ключ проекта по умолчанию (по DEC-008) и поле «свой ключ».
  - deps: CORE-006 · size: M · parallel: yes · веха: M1 · ref: 03 §3.1, §4.7, S §3
  - done when: тесты на записанных ответах; 429 от TMDB обрабатывается backoff-ом; атрибуция TMDB выводится в UI-016.
- [x] **META-010** Клиенты AniList и Shikimori
  - Что: AniList GraphQL (адаптивный лимит 30–90 запросов в минуту, связи сезонов); Shikimori (User-Agent с именем приложения, 5 запросов в секунду и 90 в минуту, русские названия и описания по MAL ID).
  - deps: META-009 · size: M · parallel: yes · веха: M2 · ref: 03 §3.2
  - done when: тесты на записанных ответах; лимиты соблюдаются под нагрузочным тестом.
- [ ] **META-011** AniDB: HTTP и UDP с аккаунтом пользователя
  - Что: HTTP API (метаданные, жёсткое кэширование, дамп названий не чаще раза в сутки); UDP API (`FILE` по ed2k + size, очередь с лимитом 1 пакет в 4 с, обработка бана); только на сервере или десктопе и только по opt-in.
  - deps: META-009, LIB-004 · size: M · parallel: yes · веха: M2 · ref: 03 §3.2, §4.3
  - done when: тест на эмуляторе UDP-сервера; при конфликте хэша и текстового матча побеждает хэш, пользователь получает уведомление.
- [ ] **META-012** Дополнительные провайдеры [P1]
  - Что: TVDB v4 (только ключ пользователя), TVmaze (daily-шоу, CC BY-SA), Fanart.tv (clearlogo и clearart, ключ проекта плюс личный ключ), OpenSubtitles moviehash → ID.
  - deps: META-009 · size: M · parallel: yes · веха: M2 · ref: 03 §3.1
  - done when: тесты на записанных ответах; daily-шоу (A11) матчатся по дате.
- [ ] **META-013** Базовый матчер для M1: название + год → TMDB
  - Что: простой путь для M1: лучшая гипотеза парсера → поиск TMDB → выбор по названию, году и типу; результат пишется в Match с `decided_by=scorer_v0`.
  - deps: META-005, META-009, LIB-007 · size: S · parallel: yes · веха: M1 · ref: 03 §4.4
  - done when: на западном подмножестве корпуса точность ≥ 85 % (бейзлайн для сравнения).
- [ ] **META-014** Скорер v0 и совместный вывод по группе
  - Что: признаки пары (группа, кандидат): похожесть названия, год ±1, тип, согласованность числа и диапазона эпизодов, медиана длительности, язык аудио, хэш-попадания, правила пользователя; ручные веса; решение для группы целиком с ограничениями (эпизоды уникальны, подряд, без дыр).
  - deps: META-007, META-013, LIB-006 · size: L · parallel: no · веха: M2 · ref: 03 §4.2 [5]
  - done when: на A2, A3 и A8 корпуса ошибок минимум на 30 % меньше, чем у META-013.
- [ ] **META-015** Модуль нумерации эпизодов
  - Что: каноническое пространство (AniDB для аниме, TMDB для остального); проекции в TVDB aired/DVD/absolute, TMDB episode groups и TheXEM; `.5`, рекапы и NCOP/NCED уходят по типам AniDB; порядок отображения выбирается на уровне серии.
  - deps: META-008, META-014 · size: L · parallel: yes · веха: M2 · ref: 03 §4.2 [6], §4.3
  - done when: точность эпизода на A1–A4 и A16 ≥ 97 % в обеих схемах.
- [ ] **META-016** Пороги, состояния матча, объяснения, API инбокса
  - Что: `p ≥ 0.92` — авто; `0.70–0.92` — авто с пометкой «проверьте»; ниже — LLM, если включён, иначе ручная очередь; `explanation_json` с топ-сигналами; MatchCandidate для UI.
  - deps: META-014 · size: M · parallel: yes · веха: M2 · ref: 03 §4.2 [7], §4.6
  - done when: пороги подобраны на dev-сплите под тихие ошибки ≤ 1 %; API инбокса покрыт тестами.
- [ ] **META-017** FolderRule и шаблоны релизов из исправлений
  - Что: исправление пользователя распространяется на группу, создаёт правило папки (offset, схема, тип) и шаблон релиза; правила синхронизируются (SYNC-001).
  - deps: META-016 · size: M · parallel: yes · веха: M2 · ref: 03 §4.6
  - done when: новый файл в `Anime/Frieren/` после исправления получает ту же серию и маппинг без запросов к сети.
- [ ] **META-018** Обученный и откалиброванный скорер
  - Что: логистическая регрессия или GBM на признаках корпуса, изотоническая калибровка, веса встроены в крейт, `model_version`, пересчёт матчей при обновлении модели.
  - deps: META-003, META-014, META-019 · size: L · parallel: yes · веха: M2 · ref: 03 §4.2 [5]
  - done when: ECE ≤ 0.03 на test-сплите; тихие ошибки ≤ 1 % при Coverage ≥ 90 %.
- [ ] **META-019** Корпус v1: 10 000 групп и инструмент разметки
  - Что: ≥ 150 групп на категорию; двойная разметка 10 % с расчётом Cohen's κ; простой веб-разметчик (ядро в WASM или локальный сервер); дамп библиотек волонтёров (`pa-corpus-dump`: find + ffprobe + oshash, маскирование имён пользователей и домашних путей).
  - deps: META-002 · size: L · parallel: yes · веха: M2 · ref: 03 §6.1
  - done when: κ ≥ 0.8; test-сплит закрыт и не используется для подбора параметров.
- [ ] **META-020** LLM-fallback по opt-in
  - Что: только локальная модель (OpenAI-совместимый endpoint пользователя: llama.cpp, Ollama) или API-ключ пользователя; обезличенные токены (имена файлов и 1–2 папки), ранжирование ≤ 10 наших кандидатов, ответ — признак для скорера, а не итоговое решение.
  - deps: META-016 · size: M · parallel: yes · веха: M2 · ref: S §3, 03 §4.5
  - done when: по умолчанию выключено; тест: полный путь не уходит в запрос; на группах с `p < 0.70` Coverage растёт без роста тихих ошибок.
- [ ] **META-021** Музыкальные метаданные
  - Что: теги (lofty: ID3v2, Vorbis, MP4, APE), MusicBrainz (1 запрос в секунду, User-Agent), Cover Art Archive, сопоставление релизов; AcoustID — только по opt-in с ключом пользователя или некоммерческим ключом проекта (по LEGAL-002).
  - deps: META-009, LEGAL-002 · size: M · parallel: yes · веха: M6 · ref: 03 §3.4
  - done when: тестовая коллекция из 200 альбомов матчится по MBID на ≥ 95 %.
- [ ] **META-022** Открытый датасет «хэш → ID» в git [P2]
  - Что: формат и экспорт подтверждённых пользователем матчей (oshash/ed2k → ID и маппинг) в виде PR в публичный датасет, только по opt-in; k-анонимность; импорт датасета в индекс; без серверов.
  - deps: META-017 · size: M · parallel: yes · веха: M10 · ref: S §10, 03 §4.8
  - done when: экспорт не содержит путей и имён файлов (тест); импорт ускоряет матч известных релизов.
- [ ] **META-023** Парсер: замер на тест-кейсах guessit и anitomy в eval-репо и доводка до ≥ 85 %
  - Что: выделено из META-005 (первый срез парсера — крейт `pa-recognition::parse`). Конвертер тест-кейсов guessit (LGPL) и anitomy (MPL) в `parse`-блоки корпуса — только в eval-репо, в продукт не попадают; отчёт по полям (title, year, season, episodes, absolute, release_group, resolution, source); доводка правил без копирования кода. После закрытия отметить и META-005.
  - deps: META-002, META-005 · size: M · parallel: yes · веха: M2 · ref: 03 §2.2, §6.1
  - done when: ≥ 85 % кейсов guessit по сравниваемым полям, отчёт лежит в eval-репо; регрессионный порог добавлен в CI eval-репо.
- [ ] **META-024** Адаптер парсера для `pa-eval`: метрики разбора имён
  - Что: `Recognizer`-адаптер и/или отдельная метрика «Парсинг» (03 §6.2) в `pa-eval` поверх `pa_recognition::parse`: точность по полям `parse`-блоков корпуса для top-1 и N-best, разрез по категориям, отчёт в Markdown и JSON.
  - deps: META-003, META-005 · size: S · parallel: yes · веха: M2 · ref: 03 §6.2
  - done when: отчёт `pa-eval` содержит раздел «Парсинг»; на `examples.jsonl` воспроизводит числа теста `corpus_examples` крейта `pa-recognition`.
- [ ] **META-025** Сверка draft-записей корпуса v0
  - Что: перевести `draft`-записи корпуса v0 (`tools/recognition-eval/corpus`) в `verified`: позиции `tmdb` сериалов (сейчас из эфирного порядка TVmaze) и `tmdb`/`tvdb_absolute` аниме (правила `titles.yaml`) сверить с API TMDB/TVDB через клиент META-009; добавить вторую схему OVA/спешлам, где она есть; расхождения — в `titles.yaml` и `notes`; затем пересчитать бейзлайны.
  - deps: META-002, META-009 · size: M · parallel: yes · веха: M2 · ref: 03 §6.1
  - done when: все записи A1–A4, A9 и A16 `verified` (или `disputed` с `notes`); `corpus-validate --strict` без ошибок.

### 3.7. PLAY — общий плеерный слой

- [x] **PLAY-001** LGPL-сборка FFmpeg для всех таргетов
  - Что: скрипты `tools/ffmpeg/`: shared, `--disable-gpl --disable-nonfree`, dav1d, libass, hwaccel (videotoolbox, mediacodec, d3d11va, vaapi); энкодеры — только нужные (aac, ac3); исходники и патчи публикуются; проверка `avutil_license()`.
  - deps: — · size: L · parallel: yes · веха: M0 · ref: 02 §3.1
  - done when: артефакты для Linux, Windows, macOS, iOS, tvOS и Android собираются; лицензия «LGPL version 2.1 or later».
- [x] **PLAY-002** LGPL-сборка libmpv
  - Что: mpv с `-Dgpl=false`, libplacebo, libass (libdovi — PLAY-014; подключение в Android-APK — PLAY-013); для Linux, Windows и macOS, а также Android (fallback-движок).
  - deps: PLAY-001 · size: M · parallel: yes · веха: M1 · ref: 02 §2.2, §3.1
  - done when: `mpv_client_api_version` доступен; в выводе конфигурации нет GPL.
- [x] **PLAY-003** Контракт `PlayerEngine` и выбор движка
  - Что: `pa-playback`: open(source, startPos), дорожки, seek, скорость, события, статистика; capability-матрица (контейнеры, кодеки, HDR) и логика Auto (Android: Media3 → libmpv; Apple: AVFoundation → свой → MPVKit) с ручным переопределением; mock-движок для UI-тестов.
  - deps: CORE-005, CORE-006 · size: M · parallel: no · веха: M1 · ref: 02 §2.8
  - done when: контракт задокументирован; mock проходит общий набор тестов движка.
- [ ] **PLAY-004** Политика дорожек и аудиообработки
  - Что: предпочтительные языки аудио и субтитров по профилю (Auto, Original, конкретный язык), forced и SDH, запоминание выбора на серию; даунмикс с нормализацией, LFE в стерео, Volume Boost до 300 %, задержка аудио.
  - deps: PLAY-003, LIB-012 · size: M · parallel: yes · веха: M1 · ref: 01 §1.4 (Audio, Language), §1.5
  - done when: таблица тест-кейсов выбора дорожек проходит на mock-движке.
- [ ] **PLAY-005** Сессия воспроизведения
  - Что: resume с позиции, «Начать сначала», continuous playback и Up Next с обратным отсчётом, запись прогресса каждые 5–10 с, на паузе, при выходе и уходе в фон.
  - deps: PLAY-003, LIB-011 · size: M · parallel: yes · веха: M1 · ref: 01 §2.7
  - done when: kill процесса во время воспроизведения теряет не больше 10 с позиции.
- [ ] **PLAY-006** Рендер субтитров в ядре: libass, PGS, VobSub
  - Что: libass → RGBA-оверлей (шрифты из вложений MKV, fallback-шрифты с кириллицей и CJK); PGS и VobSub через битмап-декодеры FFmpeg; композиция с учётом PQ в HDR (яркость 200–300 нит, настраивается).
  - deps: PLAY-001 · size: L · parallel: yes · веха: M1 · ref: 02 §2.6
  - done when: golden-тесты кадров для ASS (стили, караоке), PGS и VobSub.
- [ ] **PLAY-007** Внешние субтитры и стиль
  - Что: автозагрузка sidecar-файлов, задержка с понятным знаком, размер, шрифт, позиция и фон, режим «системный стиль» (tvOS, iOS, Android captioning) по умолчанию, запоминание позиции на серию.
  - deps: PLAY-003 · size: M · parallel: yes · веха: M1 · ref: 01 §3 п.7
  - done when: тесты настроек; системный стиль применяется на Android и iOS.
- [ ] **PLAY-008** Поиск и загрузка субтитров
  - Что: OpenSubtitles REST (ключ проекта плюс аккаунт пользователя) по moviehash и по ID; ASS сохраняется как ASS; второй провайдер (Subdl или Podnapisi) [P1].
  - deps: PLAY-007, LIB-004 · size: M · parallel: yes · веха: M1 · ref: 01 §1.5, §3 п.7
  - done when: тест на записанных ответах; загруженный ASS сохраняет стили.
- [ ] **PLAY-009** Trickplay-превью
  - Что: генерация спрайтов по I-кадрам (FFmpeg): на сервере — фоновой задачей, на клиенте — лениво и по опции; чтение trickplay Jellyfin и BIF; API «кадр по времени».
  - deps: PLAY-001, LIB-006 · size: M · parallel: yes · веха: M1 · ref: 01 §1.5 (live previews)
  - done when: для 2-часового фильма с SMB превью доступно меньше чем за 60 с; seek-превью показывается меньше чем за 100 мс.
- [ ] **PLAY-010** Модель stats overlay и адаптивный буфер
  - Что: вход и выход (кодек, профиль, HDR-метаданные MaxCLL/MaxFALL, профиль DV), HW или SW путь, буфер, сеть против битрейта, dropped frames, статус AFR, аудиопуть (passthrough, декод, LPCM); экспорт отчёта в один клик; адаптивный read-ahead и подсказка о транскоде, если источник — сервер.
  - deps: PLAY-003 · size: M · parallel: yes · веха: M1 · ref: 01 §3 п.3–4
  - done when: все движки заполняют одну модель (контрактный тест); отчёт экспортируется из CORE-008.
- [ ] **PLAY-011** Пропуск интро и титров [P1]
  - Что: маркеры из глав, Plex и Jellyfin, а также серверное определение интро по аудио-отпечатку между эпизодами сезона; режимы: кнопка, авто через 3 с, мгновенно, выкл.
  - deps: PLAY-005, LIB-006 · size: L · parallel: yes · веха: M4 · ref: 01 §1.4 (Playback), §1.5
  - done when: на тестовом сезоне интро найдено в ≥ 90 % эпизодов с точностью ±2 с.
- [ ] **PLAY-012** Google Cast и AirPlay [P1]
  - Что: Cast sender на Android и iOS (только в флейворах без ограничений F-Droid — Cast SDK проприетарный); AirPlay нативно на Apple; на Cast отдаётся HLS от сервера или файл через прокси.
  - deps: PLAY-003 · size: M · parallel: yes · веха: M5 · ref: 01 §1.5
  - done when: воспроизведение на Chromecast и Apple TV по AirPlay с синхронизацией прогресса.
- [ ] **PLAY-013** Наш libmpv в Android-сборке приложения вместо jar media_kit
  - Что: локальная копия `media_kit_libs_android_video` через `dependency_overrides`: Java-обвязка остаётся, скачивание готовых jar убирается; `jniLibs` наполняются `tools/mpv/bundle.sh android` из артефактов `libmpv-android-*` (PLAY-002); `check-native-licences.sh` проверяет APK.
  - deps: PLAY-002 · size: S · parallel: yes · веха: M1 · ref: tools/mpv/README.md
  - done when: release-APK содержит только наши LGPL `libmpv.so`/FFmpeg, видео воспроизводится на устройстве, проверка лицензий зелёная.
- [ ] **PLAY-014** libdovi в сборке libmpv
  - Что: libdovi (MIT, Rust, cargo-c) для всех таргетов `tools/mpv`, libplacebo с `-Dlibdovi=enabled`; сейчас DV-reshaping идёт встроенным кодом libplacebo.
  - deps: PLAY-002 · size: M · parallel: yes · веха: M2 · ref: 02 §2.2
  - done when: `BUILDINFO.txt` показывает libdovi на всех таргетах, гейт лицензий зелёный.

### 3.8. PLAY-AND — плеер Android и Android TV

- [x] **PLAY-AND-001** Спайк: встраивание плеера во Flutter без потери HDR
  - Что: в `spikes/android-player` сравнить PlatformView (SurfaceView, Hybrid Composition) и отдельную Activity с нативным SurfaceView; проверить HDR10, DV и AFR на Shield или Chromecast GTV; оверлей OSD на Flutter или нативный.
  - deps: — · size: M · parallel: yes · веха: M0 · ref: 02 §1.3, §2.8
  - done when: ADR-0008 с замерами и выбранным вариантом.
- [ ] **PLAY-AND-002** Плагин `pa_player_android` на Media3
  - Что: ExoPlayer по выбранной в спайке схеме, реализация `PlayerEngine` через UniFFI, события и треки, связь с Flutter OSD.
  - deps: PLAY-AND-001, CORE-004, PLAY-003, UI-001 · size: L · parallel: no · веха: M1 · ref: 02 §2.2, §7
  - done when: instrumented-тест воспроизводит MKV H.264 и HEVC; seek и смена дорожек работают.
- [ ] **PLAY-AND-003** Media3 DataSource поверх ByteSource
  - Что: DataSource через UniFFI для SMB, NFS и WebDAV; запасной путь — pa-proxy.
  - deps: PLAY-AND-002, NET-001 · size: M · parallel: yes · веха: M1 · ref: 02 §4
  - done when: 4K-remux с Samba воспроизводится без рывков на эмуляторе и на реальном устройстве.
- [ ] **PLAY-AND-004** Аудио: FFmpeg-расширение и passthrough
  - Что: FFmpeg audio extension (LGPL-сборка из PLAY-001) для TrueHD и DTS; passthrough через AudioTrack (AC3, E_AC3, E_AC3_JOC, DOLBY_TRUEHD, DTS, DTS_HD) по AudioCapabilities HDMI; настройка «авто / всегда декодировать».
  - deps: PLAY-AND-002, PLAY-001 · size: M · parallel: yes · веха: M1 · ref: 02 §2.4
  - done when: ресивер показывает Dolby Atmos (TrueHD) с Shield; на устройстве без passthrough — корректный LPCM.
- [ ] **PLAY-AND-005** HDR, Dolby Vision и AFR
  - Что: HDR10, HDR10+, HLG и DV через MediaCodec; без DV-декодера — откат на базу HDR10; P7 на Shield; AFR через `Surface.setFrameRate` и `setVideoChangeFrameRateStrategy`; опциональная смена разрешения и частоты через `preferredDisplayModeId` (как в Kodi).
  - deps: PLAY-AND-002 · size: M · parallel: yes · веха: M1 · ref: 02 §2.3, §2.7
  - done when: на Chromecast GTV телевизор переключается в DV и 23.976 Гц; stats overlay показывает режим.
- [ ] **PLAY-AND-006** Оверлей субтитров libass и PGS
  - Что: RGBA-оверлей из PLAY-006 поверх SurfaceView с синхронизацией по времени плеера; PGS — через Media3 или наш рендер.
  - deps: PLAY-AND-002, PLAY-006 · size: M · parallel: yes · веха: M1 · ref: 02 §2.6
  - done when: ASS с караоке синхронизирован с точностью ±1 кадр на тестовом сэмпле.
- [ ] **PLAY-AND-007** Запасной движок libmpv на Android
  - Что: libmpv (из PLAY-002) как второй `PlayerEngine` для форматов, которые не берёт Media3; выбор по логике Auto.
  - deps: PLAY-AND-002, PLAY-002 · size: M · parallel: yes · веха: M1 · ref: 02 §2.8
  - done when: AVI (MPEG-4 ASP) и WMV из QA-002 играют через fallback.
- [ ] **PLAY-AND-008** MediaSession, фон и PiP
  - Что: MediaSessionService, уведомление, фоновое аудио, Bluetooth-кнопки, `enterPictureInPictureMode` и автоматический PiP при уходе с экрана.
  - deps: PLAY-AND-002 · size: M · parallel: yes · веха: M1 · ref: 02 §5.3, 06 §6.4
  - done when: управление с гарнитуры и из уведомления работает; PiP переживает поворот экрана.
- [ ] **PLAY-AND-009** Интеграция с Android TV [P1]
  - Что: Watch Next / Continue Watching channel, синхронизация с LIB-011, deep links из карточек лаунчера.
  - deps: PLAY-AND-002, LIB-011 · size: M · parallel: yes · веха: M4 · ref: 01 §1.5 (платформенные функции)
  - done when: в Google TV появляется ряд «Продолжить» с нашим контентом.
- [ ] **PLAY-AND-010** Stats overlay и матрица устройств
  - Что: заполнение модели PLAY-010 (DecoderCounters, BandwidthMeter, формат выхода); ручной чек-лист на Shield, Chromecast GTV, Fire TV и среднем телефоне.
  - deps: PLAY-AND-002, PLAY-010, QA-002 · size: M · parallel: yes · веха: M1 · ref: 01 §3 п.3, 02 §2.3–2.4
  - done when: чек-лист пройден, результаты в `docs/dev/device-matrix.md`.

### 3.9. PLAY-DSK — плеер Windows, Linux и macOS (Flutter-десктоп)

- [ ] **PLAY-DSK-001** Спайк: libmpv во Flutter-десктопе
  - Что: render API в Texture (SDR, v1) против нативного дочернего окна (HWND, X11, Wayland subsurface, NSView) с `vo=gpu-next` для HDR; влияние на OSD и фокус.
  - deps: — · size: M · parallel: yes · веха: M0 · ref: 02 §2.8 (встраивание во Flutter)
  - done when: ADR-0009 с замерами на Windows, Linux (Wayland и X11) и macOS.
- [ ] **PLAY-DSK-002** Плагин `pa_player_desktop`
  - Что: хост libmpv (Rust или C++) по схеме из ADR, `stream_cb` для ByteSource, `PlayerEngine` через FRB, события и треки.
  - deps: PLAY-DSK-001, PLAY-002, PLAY-003, NET-008, UI-001 · size: L · parallel: no · веха: M1 · ref: 02 §2.2, §2.8
  - done when: на Linux, Windows и macOS воспроизводятся MKV, MP4 и TS из QA-002, с SMB и WebDAV.
- [ ] **PLAY-DSK-003** HDR на десктопе
  - Что: Windows — d3d11, `target-colorspace-hint`, определение режима HDR; Linux — Wayland color-management (KDE Plasma 6, GNOME 48+); macOS — EDR; тон-маппинг libplacebo и libdovi для DV P5/P8 на SDR-экранах.
  - deps: PLAY-DSK-002 · size: L · parallel: yes · веха: M1 · ref: 02 §2.3
  - done when: HDR10-сэмпл выводится в HDR на Windows 11 с HDR-монитором и на KDE Plasma 6; DV P5 без зелёно-фиолетовых цветов.
- [ ] **PLAY-DSK-004** HW-декод и passthrough
  - Что: d3d11va/nvdec (Windows), vaapi/vulkan/nvdec (Linux), videotoolbox (macOS), `auto-safe` с откатом на dav1d; passthrough: WASAPI exclusive + IEC 61937 (`audio-spdif`), ALSA `hdmi:`/`iec958:`, PipeWire; на macOS — CoreAudio hog для AC3 и DTS.
  - deps: PLAY-DSK-002 · size: M · parallel: yes · веха: M1 · ref: 02 §2.4–2.5
  - done when: ресивер показывает TrueHD Atmos с Windows и Linux; stats показывает HW-путь.
- [ ] **PLAY-DSK-005** Окна, полный экран и ввод
  - Что: полный экран, несколько мониторов, «затемнить другие мониторы», горячие клавиши (Space, ←/→, ↑/↓, F, J/K/L), drag & drop, «Открыть с помощью» (ассоциации файлов), мини-плеер «поверх всех» (PiP).
  - deps: PLAY-DSK-002 · size: M · parallel: yes · веха: M1 · ref: 01 §2.4, 06 §6.7
  - done when: чек-лист на трёх ОС пройден.
- [ ] **PLAY-DSK-006** Системные медиаконтролы
  - Что: SMTC (Windows), MPRIS2 (Linux), MPNowPlayingInfoCenter (macOS); запрет скринсейвера во время воспроизведения.
  - deps: PLAY-DSK-002 · size: S · parallel: yes · веха: M1 · ref: 02 §5.3, 01 §1.5
  - done when: медиаклавиши и системные виджеты управляют плеером на трёх ОС.
- [ ] **PLAY-DSK-007** Субтитры, зум, деинтерлейсинг и stats в mpv
  - Что: применение настроек PLAY-004 и PLAY-007 к mpv; режимы зума (Fit, Fill, Crop, 4:3); bwdif; заполнение модели stats из свойств mpv.
  - deps: PLAY-DSK-002, PLAY-010 · size: M · parallel: yes · веха: M1 · ref: 01 §1.5 (плеер)
  - done when: тесты свойств mpv на тест-сэмплах.
- [ ] **PLAY-DSK-008** AFR на десктопе [P2]
  - Что: смена частоты дисплея: Windows — `ChangeDisplaySettingsEx`; Linux — xrandr или wlr-output-management; `display-fps-override`; опция выключена по умолчанию.
  - deps: PLAY-DSK-002 · size: M · parallel: yes · веха: M10 · ref: 02 §2.7
  - done when: 23.976-контент переключает монитор на 24 Гц и возвращает частоту после выхода.
- [ ] **PLAY-DSK-009** Замеры спайка libmpv на Windows, macOS и реальном Linux-композиторе
  - Что: повторить методику `spikes/desktop-player` (метрики `SPIKE_RESULT`) на Windows 11 (ANGLE/DXGI-текстура и дочерний HWND), macOS (`CVPixelBuffer` и `NSView` под прозрачным `FlutterView`) и на GNOME/KDE Wayland с реальным монитором; дописать таблицы в ADR-0009.
  - deps: PLAY-DSK-001 · size: M · parallel: yes · веха: M1 · ref: ADR-0009, 02 §2.8
  - done when: в ADR-0009 есть замеры для Windows, macOS и Wayland на реальном композиторе; подтверждено или опровергнуто, что Flutter-OSD работает поверх `NSView` на macOS.

### 3.10. PLAY-APL — плеер iOS, iPadOS, macOS и tvOS

- [ ] **PLAY-APL-001** `AlphaPlayerKit` и MPVKit (этап 1)
  - Что: Swift package: MPVKit только в LGPL-варианте (не `-GPL`, проверка в CI), рендер в Metal-слой, `PlayerEngine` через UniFFI; пакет используют и Flutter-плагин, и tvOS-приложение.
  - deps: CORE-003, PLAY-003 · size: L · parallel: no · веха: M5 · ref: 02 §2.2, §3.1, §7
  - done when: MKV из QA-002 играют на симуляторах iOS и tvOS; лицензионная проверка в CI.
- [ ] **PLAY-APL-002** Путь AVPlayer
  - Что: MP4 и HLS (remux от нашего сервера и Jellyfin) через AVPlayer — нативный HDR, DV и Match Content.
  - deps: PLAY-APL-001 · size: M · parallel: yes · веха: M5 · ref: 02 §2.2, §2.8
  - done when: HLS-remux от SRV-010 воспроизводится в DV на Apple TV 4K.
- [ ] **PLAY-APL-003** Мост ByteSource для Apple-движков
  - Что: ByteSource → AVIO (MPVKit, свой движок) и pa-proxy для AVPlayer.
  - deps: PLAY-APL-001, NET-007, NET-008 · size: S · parallel: yes · веха: M5 · ref: 02 §4
  - done when: файл с SMB играет во всех трёх движках.
- [ ] **PLAY-APL-004** Flutter-плагин `pa_player_apple` (iOS, iPadOS, macOS)
  - Что: UIViewController или NSViewController поверх Flutter; PiP (AVPictureInPictureController с AVSampleBufferDisplayLayer), AirPlay, Now Playing и MPRemoteCommandCenter, AVAudioSession для фона.
  - deps: PLAY-APL-001, UI-001 · size: M · parallel: yes · веха: M5 · ref: 02 §2.8, §5.3
  - done when: на iPhone и Mac воспроизведение, PiP и управление с экрана блокировки работают.
- [ ] **PLAY-APL-005** Свой движок, шаг 1: FFmpeg → VideoToolbox → AVSampleBufferDisplayLayer (SDR)
  - Что: демуксинг FFmpeg (MKV, MP4, TS) → CMSampleBuffer → VTDecompressionSession (H.264, HEVC Main10, AV1 на A17 Pro/M3+) → AVSBDL; синхронизация через AVSampleBufferRenderSynchronizer; программный fallback.
  - deps: PLAY-APL-001, PLAY-001 · size: L · parallel: no · веха: M5 · ref: 02 §2.2 (путь Infuse), §7
  - done when: SDR-сэмплы из QA-002 воспроизводятся без рассинхрона больше 40 мс на iPhone, Mac и Apple TV.
- [ ] **PLAY-APL-006** Свой движок, шаг 2: HDR10, HLG, DV P5 и P8.1
  - Что: CMFormatDescription с `dvcC`/`dvvC`, HDR-метаданные, EDR на iOS и macOS.
  - deps: PLAY-APL-005 · size: L · parallel: no · веха: M5 · ref: 02 §2.3
  - done when: Apple TV 4K переключает телевизор в DV для P5 и P8.1 из MKV.
- [ ] **PLAY-APL-007** Свой движок, шаг 3: DV P7 → P8.1 на лету
  - Что: отбросить EL, переписать RPU через libdovi (MIT), воспроизвести как P8.1; stats честно показывает, что FEL потерян.
  - deps: PLAY-APL-006 · size: M · parallel: yes · веха: M5 · ref: 02 §2.3
  - done when: P7 MEL и FEL сэмплы воспроизводятся в DV-режиме на Apple TV.
- [ ] **PLAY-APL-008** Аудио в своём движке
  - Что: AVSampleBufferAudioRenderer; E-AC-3 JOC нативно; TrueHD и DTS-HD → FFmpeg-декод → LPCM 7.1; в stats — «TrueHD → LPCM 7.1, метаданные Atmos потеряны».
  - deps: PLAY-APL-005 · size: M · parallel: yes · веха: M5 · ref: 02 §2.4, 01 §3 п.9
  - done when: ресивер получает 7.1 LPCM от Apple TV; E-AC-3 JOC отображается как Atmos.
- [ ] **PLAY-APL-009** AFR на tvOS и macOS
  - Что: `AVDisplayManager.preferredDisplayCriteria = AVDisplayCriteria(refreshRate:formatDescription:)`, ожидание `isDisplayModeSwitchInProgress == false` перед выводом.
  - deps: PLAY-APL-006 · size: S · parallel: yes · веха: M5 · ref: 02 §2.7
  - done when: 23.976 и 25 к/с переключают режим телевизора при включённом Match Content.
- [ ] **PLAY-APL-010** Субтитры в своём движке
  - Что: оверлей libass, PGS и VobSub (CALayer) из PLAY-006, композиция с учётом PQ.
  - deps: PLAY-APL-005, PLAY-006 · size: M · parallel: yes · веха: M5 · ref: 02 §2.6
  - done when: golden-кадры совпадают с десктопным рендером.
- [ ] **PLAY-APL-011** Логика Auto и stats overlay на Apple
  - Что: выбор AVFoundation → свой движок → MPVKit по capability-матрице; заполнение модели PLAY-010 всеми тремя движками.
  - deps: PLAY-APL-002, PLAY-APL-005, PLAY-010 · size: M · parallel: yes · веха: M5 · ref: 02 §2.8
  - done when: регрессионная матрица файлов выбирает ожидаемый движок.
- [ ] **PLAY-APL-012** Матрица устройств Apple
  - Что: ручной и полуавтоматический прогон на iPhone, iPad, Mac (M-серия), Apple TV 4K (2021/2022): HDR10, DV P5/P8.1/P7, TrueHD Atmos, DTS-HD MA, AFR.
  - deps: PLAY-APL-007, PLAY-APL-008, PLAY-APL-009, QA-002 · size: M · parallel: yes · веха: M5 · ref: 02 §2.3–2.4
  - done when: результаты в `docs/dev/device-matrix.md`, блокеров нет.

### 3.11. UI — Flutter-приложение (телефон, планшет, десктоп, общие экраны)

- [x] **UI-001** Каркас Flutter-приложения
  - Что: `clients/flutter`: структура по фичам (`lib/features/<f>/`), go_router с файлом `routes.dart` у каждой фичи, state management по ADR (рекомендация — Riverpod), подключение `pa_core`, флейворы store и full (dart-define, Gradle flavors, Xcode schemes), общие состояния (пустое, skeleton, ошибка источника, ретрай).
  - deps: CORE-002 · size: M · parallel: no · веха: M0 · ref: 02 §7, 01 §4.9 п.10
  - done when: приложение собирается на 5 Flutter-платформах в обоих флейворах; widget-тест каркаса.
- [ ] **UI-002** Адаптивная навигация
  - Что: телефон — tab bar «Главная · Библиотека · Twitch · Музыка · Поиск», профиль и настройки через аватар; планшет и десктоп — sidebar; TV — верхнее меню или rail (через TV-002); брейкпоинты compact < 600, medium < 1024, expanded < 1440, large.
  - deps: UI-001 · size: M · parallel: no · веха: M1 · ref: 01 §2.5–2.6, §4.3, §4.10 п.4
  - done when: golden-тесты каркаса на 4 плотностях; вкладку Twitch можно скрыть настройкой.
- [ ] **UI-003** Тема из токенов и шрифты
  - Что: подключение `tokens.dart` (DSN-003), тёмная тема по умолчанию и светлая опционально, типографика по плотностям, Inter Variable с кириллицей, табличные цифры для таймкодов, поддержка масштаба текста до 200 %.
  - deps: UI-001, DSN-003, DSN-005 · size: M · parallel: yes · веха: M1 · ref: 01 §4.4–4.5
  - done when: golden-тесты в двух темах; ни одна карточка не ломается при масштабе текста 200 %.
- [ ] **UI-004** Онбординг
  - Что: выбор режима (только приложение / свой сервер / Plex, Jellyfin, Emby), язык, создание профиля, добавление первого источника с автообнаруженными вариантами.
  - deps: UI-002, NET-011 · size: M · parallel: yes · веха: M1 · ref: S §1 (режимы A/B/C), 01 §2.2
  - done when: integration-тест проходит онбординг с WebDAV из docker-compose.
- [ ] **UI-005** Экран «Источники» и формы
  - Что: список со статусом (онлайн, последний скан, ошибки, «Пересканировать»), добавление (обнаруженные — сверху, ручные типы — ниже), формы SMB, NFS, WebDAV, SFTP, FTP, Plex, Jellyfin, Emby; выбор папок для скана, прогресс индексации, тест скорости.
  - deps: UI-002, NET-011, LIB-003 · size: M · parallel: yes · веха: M1 · ref: 01 §2.2 (добавление источника), §2.5 п.3
  - done when: widget-тесты форм; ошибки подключения выводятся понятным текстом по кодам из CORE-009.
- [ ] **UI-006** Файловый браузер источника
  - Что: второй путь к контенту: источник → папки → файл → Detail или плеер; сортировка, «папки сверху», опция «показывать имена файлов».
  - deps: UI-005 · size: S · parallel: yes · веха: M1 · ref: 01 §2.1
  - done when: из браузера файлов открывается тот же Detail, что из библиотеки.
- [ ] **UI-007** Главная
  - Что: общий ряд «Продолжить» (фильмы, эпизоды, Twitch-VOD; бейдж типа), Up Next, ряды категорий, «Недавно добавленные», избранное, умные полки, источники; баннер handoff «Продолжить на этом устройстве» (через SYNC).
  - deps: UI-002, LIB-009, LIB-011 · size: M · parallel: yes · веха: M1 · ref: 01 §2.2 (Home), §2.7
  - done when: golden-тесты; ряд «Продолжить» обновляется по событию без перезагрузки экрана.
- [ ] **UI-008** Сетка библиотеки и контекстные меню
  - Что: постеры 2:3, комбинируемые фильтр-чипы, сортировка, алфавитный индекс, режим списка, бейджи (просмотрено, прогресс, «новое»); контекстное меню по долгому нажатию или правой кнопке (Play, «Просмотрено», избранное, плейлист, «Изменить метаданные», загрузка, удаление).
  - deps: UI-002, LIB-009 · size: M · parallel: yes · веха: M1 · ref: 01 §2.2 (Grid), §2.3, §3 п.5
  - done when: 10 000 элементов прокручиваются со скоростью 60 fps на среднем Android (профиль в QA-006).
- [ ] **UI-009** Detail фильма и страницы людей
  - Что: hero-backdrop со скримом, clearlogo, метаданные, тех-бейджи без логотипов Dolby и DTS, внешние рейтинги, Play/Resume («с 1:23:45, iPhone, 5 мин назад»), выбор версии, ссылка на трейлер, актёры и съёмочная группа, extras, коллекция, техническая информация о файле; страницы людей [P1].
  - deps: UI-008, LIB-010 · size: M · parallel: yes · веха: M1 · ref: 01 §2.2 (Detail), §4.5 (бейджи)
  - done when: golden-тесты на 4 плотностях; начальный фокус на Play/Resume.
- [ ] **UI-010** Detail сериала, сезоны и эпизоды
  - Что: табы сезонов (включая Specials), эпизоды 16:9 с прогрессом, Hide Spoilers (размытие описаний и кадров непросмотренных), выбор порядка эпизодов (DisplayPreference), кнопка «Resume S2E5».
  - deps: UI-009 · size: M · parallel: yes · веха: M1 · ref: 01 §2.2, §1.4 (Hide Spoilers), 03 §4.2 [6]
  - done when: при смене порядка с TMDB на AniDB эпизоды перестраиваются (тест на фикстуре).
- [ ] **UI-011** Поиск
  - Что: группы результатов (фильмы, сериалы, эпизоды, люди, коллекции, стримеры, музыка), история поиска, голосовой ввод [P2].
  - deps: UI-002, LIB-008 · size: S · parallel: yes · веха: M1 · ref: 01 §1.5
  - done when: результаты появляются меньше чем за 100 мс после ввода на 50 000 элементах.
- [ ] **UI-012** Инбокс «Нужна проверка» и «Исправить матч»
  - Что: карточки с уверенностью и альтернативами, «Почему?» (топ-сигналы), поиск по всем провайдерам, вставка ID или URL (TMDB, TVDB, AniDB, MAL, Shikimori), применение ко всей группе и создание правила папки.
  - deps: UI-009, META-016, META-017 · size: M · parallel: yes · веха: M2 · ref: 03 §4.6, 01 §1.5
  - done when: integration-тест: исправление применяется к 24 эпизодам и создаёт FolderRule.
- [ ] **UI-013** OSD плеера
  - Что: Flutter-оверлей поверх нативной поверхности (по ADR-0008 и 0009): scrubber с trickplay и маркерами глав, «осталось» и «закончится в», треки, скорость (удержание — 2×), зум, пропуск ±N, Skip Intro, карточка Next Up, панель stats; жесты на мобильных (двойной тап, яркость и громкость, pinch); автоскрытие через 4 с.
  - deps: UI-001, PLAY-003, PLAY-009 · size: L · parallel: no · веха: M1 · ref: 01 §2.2 (OSD), §2.3, §4.7
  - done when: widget-тесты на mock-движке; на Android и десктопе OSD работает поверх реального видео.
- [ ] **UI-014** Панели дорожек и субтитров
  - Что: аудио, субтитры, видео — выбор, загрузка из OpenSubtitles, задержки, стиль субтитров; на TV — верхняя панель с вкладками.
  - deps: UI-013, PLAY-004, PLAY-007, PLAY-008 · size: M · parallel: yes · веха: M1 · ref: 01 §2.2 (OSD), §3 п.7
  - done when: выбор дорожки сохраняется для серии и применяется к следующему эпизоду.
- [ ] **UI-015** Профили и родительский контроль
  - Что: выбор профиля при запуске (опционально), аватары, PIN-пад, детский профиль (белый список, лимит рейтинга, скрытие арта, скрытие Twitch), App Lock и Search Lock.
  - deps: UI-002, LIB-012 · size: M · parallel: yes · веха: M1 · ref: 01 §1.5 (родительский контроль), §2.5 п.2, §3 п.2, п.6
  - done when: integration-тест: детский профиль не может открыть настройки без PIN.
- [ ] **UI-016** Настройки и «О программе»
  - Что: разделы General, Sync, Metadata, Collections, Playback, Audio, Language, Parental, Twitch, Music, Server, «Состояние», «О программе»; в «О программе» — лицензии (cargo-about, flutter licenses, SwiftPM), атрибуция TMDB (логотип и фраза), Twitch и MusicBrainz; донат-ссылки только во full-флейворе.
  - deps: UI-002, CORE-013 · size: M · parallel: yes · веха: M1 · ref: 01 §1.4, S §4
  - done when: тест: в store-флейворе на экране «О программе» нет донат-ссылок.
- [ ] **UI-017** Экран «Состояние» (диагностика)
  - Что: логи, экспорт отчёта, тест скорости до источника, версии ядра и движков, crash-отчёты (opt-in, отправка вручную).
  - deps: UI-016, CORE-008, PLAY-010 · size: S · parallel: yes · веха: M1 · ref: 01 §2.5 п.4, §3 п.3
  - done when: отчёт экспортируется в один клик на всех платформах.
- [ ] **UI-018** Экран загрузок [P1]
  - Что: очередь pa-jobs (прогресс, пауза, отмена), офлайн-копии, занятое место, автоочистка.
  - deps: UI-002, CORE-011, NET-019 · size: M · parallel: yes · веха: M4 · ref: 01 §2.6 (Downloads), 04 §1.9
  - done when: widget-тесты; прогресс обновляется по событиям.
- [ ] **UI-019** Pairing по QR и коду
  - Что: добавление источника, сервера или настроек на TV с телефона или десктопа в той же LAN (без облака); ввод текста на TV с телефона.
  - deps: UI-002, SYNC-005 · size: M · parallel: yes · веха: M3 · ref: 01 §3 п.12, S §2
  - done when: пароль SMB, введённый на телефоне, попадает на Android TV и сохраняется в SecretStore TV, а не в логи и не в sync-журнал.
- [x] **UI-020** Локализация ru и en
  - Что: gen-l10n (ARB), ключи с префиксом фичи, плюрализация для ru (one, few, many), форматы дат и чисел; CI-проверка отсутствующих ключей; псевдолокализация.
  - deps: UI-001 · size: M · parallel: yes · веха: M1 · ref: CLAUDE.md (язык), 01 §4.4
  - done when: CI падает на непереведённом ключе; длинные русские строки не обрезаются в golden-тестах.
- [x] **UI-023** Исправить баги со скриншотов дизайна 2026-11-14
  - Что: (1) плеер на телефоне в портрете — нижняя панель управления переполняется на 150 px; (2) на телефоне нижняя навигация подсвечивает «Главная» на экране настроек; (3) страница лицензий (showLicensePage) пустая — проверить регистрацию LicenseRegistry; (4) перевести строки `pa_player` (OSD, мини-плеер, «Сейчас играет») через собственный API строк пакета с ru/en.
  - deps: UI-020 · size: M · parallel: yes · веха: M1 · ref: docs/dev/release-0.1-contract.md
  - done when: layout-тест плеера в портрете на телефоне без overflow; навигация подсвечивает активный раздел; лицензии видны; RU-скриншоты плеера на русском.
- [ ] **UI-021** Доступность
  - Что: Semantics для TalkBack, VoiceOver, Narrator и Orca; порядок фокуса клавиатурой; контраст; Reduce Motion (кроссфейд вместо переходов).
  - deps: UI-003 · size: M · parallel: yes · веха: M1 · ref: 01 §1.5 (VoiceOver/TalkBack — P0), §4.7
  - done when: аудит QA-009 без блокеров на ключевых экранах.
- [ ] **UI-022** Особенности платформ
  - Что: десктоп — меню-бар macOS, нативные заголовки окон (Windows 11, GNOME, KDE), один экземпляр, deep links `projectalpha://`; мобильные — sidebar на iPad, Android share target, iOS Files и web-upload [P1], виджеты [P2].
  - deps: UI-002 · size: M · parallel: yes · веха: M1 · ref: 01 §2.3–2.4, §4.10 п.3
  - done when: чек-лист на каждой платформе пройден.

### 3.12. TV — Android TV (Flutter), режим Big Picture и tvOS (SwiftUI)

- [x] **TV-001** Каркас tvOS-приложения на SwiftUI
  - Что: `clients/apple/TVApp` (Xcode-проект с SwiftPM), подключение PACore, экран с `core_version()`, store-флейвор.
  - deps: CORE-003 · size: S · parallel: yes · веха: M0 · ref: 02 §1.3, §7
  - done when: собирается в CI и запускается в симуляторе tvOS.
- [ ] **TV-002** Фокус-движок для Android TV во Flutter
  - Что: обёртка над Focus, FocusTraversalGroup и Shortcuts: геометрическая навигация, память последнего элемента в ряду, фокус-гайды между рядами разной ширины, начальный фокус на Play, «Назад» сначала к началу ряда или меню, долгое Select — контекстное меню; автотест «нет ловушек фокуса» (обход D-pad, клавиатурой и геймпадом).
  - deps: UI-002 · size: L · parallel: no · веха: M1 · ref: 01 §4.2
  - done when: автотест обхода проходит на всех экранах M1.
- [ ] **TV-003** TV-плотность компонентов
  - Что: масштаб при фокусе (постер 1.10, 16:9 1.08, кнопка 1.05 с инверсией), тень и блик (без tilt на D-pad), затемнение через 6–10 с бездействия, safe area 60/80, сетки 5–8 колонок с отступом 40, типографика TV.
  - deps: TV-002, DSN-002 · size: M · parallel: yes · веха: M1 · ref: 01 §4.2–4.4
  - done when: golden-тесты TV-плотности; ручная проверка на телевизоре 1080p и 4K.
- [ ] **TV-004** Специфика Android TV и производительность
  - Что: Leanback launcher intent, баннер, `touchscreen required=false`, медиаклавиши пульта, профиль производительности (blur выключен на слабых устройствах), профилирование на Chromecast GTV и Fire TV Stick.
  - deps: TV-002 · size: M · parallel: yes · веха: M1 · ref: 01 §4.6 (fallback), 02 §1.3
  - done when: прокрутка Home на Fire TV Stick 4K без пропусков кадров больше 5 %.
- [ ] **TV-005** Режим Big Picture на десктопе
  - Что: полный 10-foot UI с геймпадом на тех же TV-компонентах; переключение из меню и по ключу запуска.
  - deps: TV-002, PLAY-DSK-005 · size: M · parallel: yes · веха: M4 · ref: 01 §4.2 (перенос на десктоп)
  - done when: всё приложение проходится геймпадом Xbox на Windows и Linux.
- [ ] **TV-006** tvOS: хранилище и навигационный каркас
  - Что: БД в Application Support, картинки в Caches, восстановление после очистки (быстрый пересканинг или снапшот с сервера или другого устройства); верхний tab bar «Главная · Фильмы · Сериалы · Twitch · Музыка · Поиск · Настройки».
  - deps: TV-001, LIB-001 · size: M · parallel: no · веха: M5 · ref: 02 §4, 04 §1.4 п.5, 01 §2.6
  - done when: XCTest — после удаления Caches библиотека восстанавливается без потери прогресса.
- [ ] **TV-007** tvOS: Главная, сетка и Detail
  - Что: экраны на системном focus engine (`.card`, параллакс постеров), паритет с UI-007–UI-010, hero с затемнением, табы сезонов.
  - deps: TV-006, LIB-009, LIB-011 · size: L · parallel: yes · веха: M5 · ref: 01 §2.2
  - done when: UI-тесты XCUITest навигации пультом; ручная проверка на Apple TV.
- [ ] **TV-008** tvOS: источники и pairing с iPhone
  - Что: формы источников, разрешение Local Network, ввод данных с телефона через pairing.
  - deps: TV-006, NET-011, SYNC-005 · size: M · parallel: yes · веха: M5 · ref: 01 §2.2 (добавление источника), §3 п.12
  - done when: SMB-источник добавляется с iPhone без ввода пароля на пульте.
- [ ] **TV-009** tvOS: OSD плеера поверх AlphaPlayerKit
  - Что: нижняя панель со scrubber и превью, свайп вниз — панель Info, Subtitles, Audio, Video; клики по краям тачпада (Skip Mode), Skip Intro, Next Up.
  - deps: TV-006, PLAY-APL-001 · size: L · parallel: yes · веха: M5 · ref: 01 §2.2 (Player OSD)
  - done when: все действия доступны с Siri Remote и с пульта без тачпада.
- [ ] **TV-010** tvOS: Top Shelf и иконки
  - Что: расширение Top Shelf (sectioned, ряд «Продолжить»), layered app icon, статичное изображение Top Shelf.
  - deps: TV-007, DSN-006 · size: M · parallel: yes · веха: M5 · ref: 01 §1.5 (Top Shelf), §5.2 п.3
  - done when: Top Shelf показывает актуальный ряд «Продолжить».
- [ ] **TV-011** tvOS: профили, родительский контроль, настройки, «Состояние»
  - Что: выбор профиля, PIN, App Lock (в Infuse на tvOS его нет), настройки, диагностика.
  - deps: TV-006, LIB-012 · size: M · parallel: yes · веха: M5 · ref: 01 §3 п.2, п.6
  - done when: XCUITest PIN-сценариев.
- [ ] **TV-012** tvOS: раздел Twitch
  - Что: вход через DCF по QR, Live now, избранное, VOD с главами, чат справа только для чтения, «Ответить» — карточка на телефон.
  - deps: TV-009, TWI-004, TWI-010 · size: L · parallel: yes · веха: M7 · ref: 06 §6.5, 01 §2.6
  - done when: live и VOD с чатом работают на Apple TV.
- [ ] **TV-013** tvOS: раздел «Музыка»
  - Что: Now Playing в стиле Apple Music на tvOS (обложка слева, тексты справа), режим заставки, глобальная кнопка Play/Pause.
  - deps: TV-009, MUS-003 · size: M · parallel: yes · веха: M6 · ref: 01 §4.11
  - done when: альбом играет gapless; тексты синхронизированы.

### 3.13. SYNC — синхронизация без облака

- [x] **SYNC-001** `pa-sync`: модель, HLC, LWW и outbox
  - Что: гибридные логические часы, `op_id` (UUIDv7), операции (progress.upsert, watched/unwatch, favorite, rating, плейлисты, folder_rule, настройки без секретов, избранное Twitch); LWW по полю, «просмотрено» монотонно; outbox в SQLite с идемпотентностью и ретраями.
  - deps: CORE-005, LIB-001 · size: M · parallel: no · веха: M3 · ref: 04 §1.8
  - done when: property-тест (proptest): применение операций в любом порядке даёт одно состояние.
- [x] **SYNC-002** Протокол синхронизации
  - Что: `docs/api/sync-protocol.md`; сообщения hello{device_id, cursor}, pb.update, pb.takeover, op, ack, change, pull; версионированный непрозрачный payload (готовность к E2E); одинаков для P2P, хаба на сервере и iroh.
  - deps: SYNC-001 · size: M · parallel: no · веха: M3 · ref: 04 §1.8.1
  - done when: спецификация и тесты кодека сообщений; совместимость версий ±1.
- [ ] **SYNC-003** Эфемерная сессия воспроизведения
  - Что: heartbeat каждые 5 с и сразу на pause, seek, stop и уходе в фон; позиция при старте — максимум по HLC; pull с таймаутом 500–800 мс и тост «Перейти к 41:11?»; takeover — пауза на другом устройстве того же профиля (настраивается).
  - deps: SYNC-002, PLAY-005 · size: M · parallel: yes · веха: M3 · ref: 04 §1.8.1
  - done when: интеграционный тест на двух процессах: продолжение с точностью ≤ 2 с.
- [ ] **SYNC-004** Сопоставление медиа между устройствами
  - Что: ключ — внешние ID + S/E; иначе отпечаток контента (OSHash); музыка — MBID или хэш тегов и длительности; Twitch — свои ключи; одна шара по разным путям (`smb://nas` и `smb://192.168.1.5`).
  - deps: SYNC-001, LIB-004 · size: M · parallel: yes · веха: M3 · ref: 04 §1.8, 06 §5.2
  - done when: тест: один файл на двух устройствах с разными путями даёт один ключ.
- [ ] **SYNC-005** Pairing устройств («семья устройств»)
  - Что: общие ключи через QR или 6-значный код в LAN, список устройств, отзыв устройства, ротация ключей; без облака.
  - deps: SYNC-001, CORE-007 · size: M · parallel: yes · веха: M3 · ref: S §2
  - done when: отозванное устройство больше не может подключиться (тест).
- [ ] **SYNC-006** P2P-транспорт в LAN и выбор хаба
  - Что: mDNS `_alpha-sync._tcp` (ADR-0019; `_projectalpha-sync` длиннее 15 символов RFC 6763), WebSocket между устройствами, шифрование (Noise или TLS с ключами из pairing); хабом становится любое включённое устройство (приоритет: сервер, десктоп, ТВ, телефон).
  - deps: SYNC-002, SYNC-005, NET-009 · size: L · parallel: yes · веха: M3 · ref: S §2
  - done when: три устройства в LAN сходятся; при выключении хаба выбирается новый меньше чем за 10 с.
- [ ] **SYNC-007** Синхронизация через хранилище
  - Что: `.project_alpha/state/<device_id>.jsonl` в папке SMB или WebDAV — append-only журнал на каждое устройство, слияние по HLC, компакция в снапшоты, блокировки не нужны (каждое устройство пишет свой файл).
  - deps: SYNC-001, NET-003, NET-004 · size: M · parallel: yes · веха: M3 · ref: S §2
  - done when: тест с тремя устройствами, работающими офлайн и онлайн в произвольном порядке, сходится.
- [x] **SYNC-008** Хаб синхронизации на сервере
  - Что: WebSocket-эндпоинт в SRV; сервер авторитетен для своей библиотеки; fan-out не дольше 200 мс; журнал `server_seq`; клиенты одновременно держат сервер и P2P, дубликаты гасятся по op_id и HLC.
  - deps: SYNC-002, SRV-001 · size: M · parallel: yes · веха: M4 · ref: 04 §1.8.1
  - done when: нагрузочный тест: 50 устройств, 10 сообщений в секунду, задержка p95 меньше 200 мс.
- [ ] **SYNC-009** Профили в синхронизации
  - Что: ключ записи `(profile, media_key)`; takeover только внутри профиля; синхронизация профилей и хэшей PIN между устройствами.
  - deps: SYNC-003, LIB-012 · size: S · parallel: yes · веха: M3 · ref: 04 §1.8.1, 01 §2.7
  - done when: прогресс двух профилей одного фильма не конфликтует (тест).
- [ ] **SYNC-010** Синхронизация списка источников [P1]
  - Что: конфигурация источников без паролей; пароли — опционально в E2E-контейнере (libsodium secretbox, ключ из pairing).
  - deps: SYNC-005, NET-011 · size: M · parallel: yes · веха: M3 · ref: 04 §5.1, §1.8 (E2E)
  - done when: тест: пароль не появляется в открытом виде ни в журнале, ни в файлах `.project_alpha/state`.
- [ ] **SYNC-011** Trakt [P1]
  - Что: вход по device code, scrobble, история, watchlist, оценки, импорт истории; условия для open source-клиента — по LEGAL-002 (client secret или свой client_id пользователя).
  - deps: SYNC-001, LEGAL-002 · size: M · parallel: yes · веха: M3 · ref: S §2, 01 §1.5
  - done when: тест на записанных ответах; scrobble доходит в тестовый аккаунт.
- [ ] **SYNC-012** Тесты сходимости и задержки handoff
  - Что: симуляция N устройств с офлайн-периодами, перестановками и дублями сообщений; замер задержки «пауза на A → баннер на B» в LAN.
  - deps: SYNC-006, SYNC-007 · size: M · parallel: yes · веха: M3 · ref: 04 §1.8.1
  - done when: задержка p95 ≤ 1 с в LAN; 10 000 случайных сценариев сходятся.

### 3.14. SRV — медиасервер (режим B)

- [x] **SRV-001** Каркас сервера
  - Что: `server/` на axum и tokio, конфиг, SQLite WAL через pa-library (тот же код, что в клиенте), `/health`, graceful shutdown, структурные логи.
  - deps: LIB-001, CORE-006 · size: M · parallel: no · веха: M4 · ref: 04 §1.5
  - done when: сервер стартует с пустой БД; интеграционный тест `/health`.
- [ ] **SRV-002** OpenAPI и контрактные тесты
  - Что: OpenAPI 3.1 через utoipa, `/api/v1`; `docs/api/openapi.json` генерируется, CI проверяет дрейф; версия протокола и совместимость клиент ↔ сервер ±1 major.
  - deps: SRV-001 · size: S · parallel: yes · веха: M4 · ref: 04 §1.5 (API), §4.2
  - done when: CI падает, если код и спецификация расходятся; контракт-тесты для N-1.
- [ ] **SRV-003** Аутентификация и pairing клиентов
  - Что: локальные пользователи (админ и домочадцы), Argon2, PASETO v4.public (access на 15 мин, refresh с ротацией), PIN-профили, rate limit на логин, pairing клиента по QR или коду (без облачного аккаунта), полностью офлайн.
  - deps: SRV-001, SYNC-005 · size: M · parallel: yes · веха: M4 · ref: 04 §1.5 (Auth), §5.3
  - done when: тесты перебора (блокировка), ротации и отзыва токенов.
- [x] **SRV-004** TLS и mDNS-анонс
  - Что: самоподписанный сертификат, анонс `_projectalpha._tcp` (TXT: id, v, name, fp), pinning отпечатка в клиенте (TOFU); HTTP без TLS — только по явной опции.
  - deps: SRV-001, NET-009 · size: S · parallel: yes · веха: M4 · ref: 04 §1.6, §5.2
  - done when: клиент находит сервер и отклоняет подменённый сертификат.
- [ ] **SRV-005** Библиотеки на сервере
  - Что: скан и watcher локальных дисков, воркер метаданных с очередью и rate limit, фоновый ed2k, AniDB UDP с аккаунтом владельца, генерация trickplay.
  - deps: SRV-001, LIB-005, META-013 · size: M · parallel: yes · веха: M4 · ref: 04 §1.5, 03 §7.2
  - done when: библиотека на 10 000 файлов индексируется и распознаётся; повторный старт не пересканирует всё заново.
- [ ] **SRV-006** Media API и клиент своего сервера в ядре
  - Что: библиотеки, элементы, детали, изображения (ресайз и кэш), поиск, прогресс, профили; клиент в `pa-remote` — сервер как источник в общей модели.
  - deps: SRV-002, SRV-003 · size: L · parallel: yes · веха: M4 · ref: 04 §1.5, §1.3
  - done when: Flutter-клиент показывает библиотеку сервера тем же UI, что и режим A.
- [ ] **SRV-007** Direct play
  - Что: отдача файлов по HTTP Range, доступ только внутри корней библиотек (защита от path traversal), короткоживущие токены в URL.
  - deps: SRV-006 · size: S · parallel: yes · веха: M4 · ref: 04 §1.5 (транскодинг п.1), §5.3
  - done when: фаззинг путей не выходит за корень; seek в 4K-remux работает быстрее 1 с в LAN.
- [ ] **SRV-008** Транскод-менеджер
  - Что: FFmpeg — внешний процесс (своя сборка), аргументы только из whitelist; решение direct play / remux / transcode по профилю возможностей клиента; HLS fMP4 с сегментами 4–6 с, плейлист заранее, перезапуск с `-ss` при seek, остановка при простое, лимит параллельности.
  - deps: SRV-007, DEC-005 · size: L · parallel: no · веха: M4 · ref: 04 §1.5, §5.3
  - done when: seek в любую точку транскодируемого потока быстрее 3 с; простаивающие процессы завершаются.
- [ ] **SRV-009** HW-ускорение и тон-маппинг
  - Что: VAAPI и QSV (Intel), NVENC (NVIDIA), AMF (AMD, Windows), VideoToolbox (macOS), V4L2 и RKMPP (ARM); автодетект и тестовый прогон из админки; HDR → SDR (tonemap_opencl, tonemap_vaapi или libplacebo).
  - deps: SRV-008 · size: L · parallel: yes · веха: M4 · ref: 04 §1.5, 05 §4.3
  - done when: Intel N100 держит ≥ 2 потока HEVC 4K HDR → 1080p SDR; NVENC проверен на реальной карте.
- [ ] **SRV-010** Remux для AVPlayer и транскод аудио
  - Что: MKV → HLS fMP4 (`-c copy`) для AVPlayer; TrueHD и DTS → E-AC-3 или AAC для мобильных устройств.
  - deps: SRV-008 · size: M · parallel: yes · веха: M4 · ref: 04 §1.5 (транскодинг п.2–3), 02 §2.2
  - done when: Apple TV через AVPlayer играет remux с DV (вместе с PLAY-APL-002).
- [ ] **SRV-011** API задач и события WebSocket
  - Что: `POST /api/v1/jobs` и управление задачами; события job.progress, job.done, scan.progress, now playing, library updated.
  - deps: SRV-006, CORE-011 · size: M · parallel: yes · веха: M4 · ref: 04 §1.10
  - done when: клиент ставит задачу на сервер и видит прогресс в реальном времени.
- [ ] **SRV-012** «Оптимизировать для загрузки» [P1]
  - Что: фоновая задача транскода в MP4 (4 или 8 Мбит/с), клиент забирает готовый файл.
  - deps: SRV-011, SRV-008 · size: S · parallel: yes · веха: M4 · ref: 04 §1.9
  - done when: телефон скачивает оптимизированную копию и играет её офлайн.
- [ ] **SRV-013** Удалённый доступ через iroh
  - Что: iroh endpoint сервера (node id = публичный ключ), HTTP поверх QUIC-стрима; по умолчанию публичные relay n0, есть встроенный relay (опция) и relay пользователя; клиент в `pa-remote` выбирает LAN или iroh автоматически; лимит битрейта через relay.
  - deps: SRV-006 · size: L · parallel: yes · веха: M4 · ref: S §2, 04 §1.7
  - done when: воспроизведение через мобильную сеть (CGNAT) работает; прямое соединение устанавливается там, где возможно.
- [ ] **SRV-014** Веб-админка сервера
  - Что: SPA в `server/web`, встроена через rust-embed (стек — по ADR, рекомендация TypeScript + Svelte или Preact без SSR, общий с DIST-014): мастер первого запуска, пользователи, библиотеки, задачи, транскод и HW, удалённый доступ, логи, уведомление о новой версии (`releases.json`).
  - deps: SRV-006, DSN-003 · size: L · parallel: yes · веха: M4 · ref: 04 §1.5 (API), §4.7
  - done when: e2e-тест Playwright мастера первого запуска.
- [ ] **SRV-015** Хранилище и retention на сервере
  - Что: storage targets из `pa-storage`, квоты, корзина, дашборд занятости (по категориям, топ каналов по объёму, что удалится следующим).
  - deps: SRV-011, CORE-012 · size: M · parallel: yes · веха: M4 · ref: 04 §1.10
  - done when: при пересечении резерва задачи ставятся на паузу и приходит уведомление.
- [ ] **SRV-016** [full] Twitch-задачи на сервере
  - Что: вход владельца через DCF в админке, токен в зашифрованном хранилище, мониторинг эфиров и запись по правилам (TWI-009, TWI-018).
  - deps: SRV-011, TWI-009, TWI-018 · size: M · parallel: yes · веха: M7 · ref: 04 §5.1.1, 06 §4.2
  - done when: сервер сам начинает запись в течение 2 минут после старта эфира.
- [ ] **SRV-017** Нагрузочные тесты и безопасность сервера
  - Что: 5 одновременных direct play 4K, 2 транскода на N100, память в простое не больше 150 МБ на ARM; cargo-fuzz для Range, HLS и путей; по умолчанию сервер не слушает WAN, UPnP выключен.
  - deps: SRV-008 · size: M · parallel: yes · веха: M4 · ref: 04 §5.3
  - done when: отчёт нагрузки в `docs/dev/`; fuzz-цели в nightly CI.
- [ ] **SRV-018** Сборки сервера
  - Что: бинарники для linux gnu и musl (x64, arm64), служба Windows, launchd на macOS, systemd unit, Docker multi-arch (amd64 и arm64) в GHCR с cosign; примеры с `/dev/dri` и NVIDIA CDI.
  - deps: SRV-001, CI-011 · size: M · parallel: yes · веха: M4 · ref: 04 §4.4, 05 §4.3
  - done when: `docker run` и `systemctl start` поднимают сервер; образ подписан.

### 3.15. MUS — музыкальный плеер

- [x] **MUS-001** Музыкальная библиотека: схема и скан тегов
  - Что: Artist, ReleaseGroup, Release, Recording, Track (MBID), AudioFile, жанры, композиторы, вид «папки»; чтение тегов (ID3v2, Vorbis, MP4, APE через lofty), встроенные обложки и `folder.jpg`, мультидиск.
  - deps: LIB-001, LIB-003 · size: M · parallel: yes · веха: M6 · ref: 03 §5 (музыка), 01 §4.11
  - done when: коллекция из 5 000 треков индексируется; сборники разбираются правильно.
- [ ] **MUS-002** Воспроизведение v1 через libmpv в аудиорежиме
  - Что: `gapless-audio`, `prefetch-playlist`, `replaygain=track|album` на десктопе и Android; на Apple — MPVKit audio.
  - deps: PLAY-002, MUS-001 · size: M · parallel: no · веха: M6 · ref: 02 §5.2 (вариант A)
  - done when: тестовый gapless-альбом без пауз на стыках (проверка по сэмплам).
- [ ] **MUS-003** Очередь и плейлисты
  - Что: «Сейчас играет», Play Next, Add to Queue, «Далее из: …», история, shuffle и repeat, перетаскивание, «Сохранить как плейлист»; плейлисты и умные плейлисты [P1], импорт и экспорт M3U/M3U8; музыка — глобальное состояние: запуск видео ставит её на паузу.
  - deps: MUS-002 · size: M · parallel: yes · веха: M6 · ref: 01 §4.11 (очередь), §1.5
  - done when: unit-тесты модели очереди; очередь переживает перезапуск приложения.
- [ ] **MUS-004** UI музыкальной библиотеки
  - Что: Home музыки (недавнее, часто слушаете, миксы), Library (плейлисты, артисты, альбомы, треки, жанры, композиторы, папки), чипы Hi-Res, Lossless, «Загружено»; страница альбома (бейдж формата, диски), страница артиста.
  - deps: MUS-001, UI-002 · size: L · parallel: yes · веха: M6 · ref: 01 §4.11
  - done when: golden-тесты на 4 плотностях.
- [ ] **MUS-005** Now Playing и мини-плеер
  - Что: полноэкранный Now Playing (обложка или размытый фон, transport, выбор вывода, строка «FLAC · 24/96 · bit-perfect»); мини-плеер: капсула над tab bar на телефоне, нижняя панель на десктопе, плашка на TV.
  - deps: MUS-003, MUS-004 · size: M · parallel: yes · веха: M6 · ref: 01 §4.11
  - done when: мини-плеер виден во всех разделах, кроме видеоплеера.
- [ ] **MUS-006** Тексты песен
  - Что: USLT и SYLT, `.lrc` рядом с треком, LRCLIB с устройства (User-Agent, локальный кэш), построчная подсветка, переход по тапу на строку, полноэкранный режим.
  - deps: MUS-005 · size: M · parallel: yes · веха: M6 · ref: 01 §4.11, 03 §3.4
  - done when: LRC синхронизирован с точностью ±100 мс.
- [ ] **MUS-007** Системная интеграция аудио
  - Что: Android — Media3 MediaLibraryService (фон, уведомление, Bluetooth); iOS — AVAudioSession `.playback`, MPNowPlayingInfoCenter, MPRemoteCommandCenter; десктоп — общие с видео SMTC, MPRIS и macOS Now Playing.
  - deps: MUS-002, PLAY-AND-008, PLAY-DSK-006 · size: M · parallel: yes · веха: M6 · ref: 02 §5.3
  - done when: управление с экрана блокировки и гарнитуры на всех платформах.
- [ ] **MUS-008** Свой аудиодвижок `pa-audio`
  - Что: FFmpeg-декод → rubato → кольцевой буфер → cpal (shared); настоящий gapless с учётом delay и padding (iTunSMPB, LAME header), crossfade 0–12 с (внутри альбома выключается), ReplayGain с pre-amp и защитой от клиппинга, R128 для Opus.
  - deps: MUS-002, PLAY-001 · size: L · parallel: no · веха: M6 · ref: 02 §5.1–5.2 (вариант B)
  - done when: стык треков совпадает сэмпл в сэмпл с эталоном на тестовых файлах.
- [ ] **MUS-009** Bit-perfect вывод
  - Что: WASAPI exclusive (Windows), CoreAudio hog mode и смена nominal sample rate (macOS), ALSA `hw:` или PipeWire с `allowed-rates` (Linux), Android 14+ `MIXER_BEHAVIOR_BIT_PERFECT` для USB-DAC; индикатор в UI.
  - deps: MUS-008 · size: L · parallel: yes · веха: M6 · ref: 02 §5.2 (bit-perfect)
  - done when: loopback-тест или USB-DAC с индикацией частоты подтверждает bit-perfect на трёх десктопных ОС.
- [ ] **MUS-010** DSD
  - Что: DSF и DFF → PCM 176.4 кГц по умолчанию; DoP на десктопе в эксклюзивном режиме; native DSD через ASIO — [P2].
  - deps: MUS-009 · size: M · parallel: yes · веха: M6 · ref: 02 §5.1
  - done when: DSD64 проигрывается в PCM и через DoP на совместимом DAC.
- [ ] **MUS-011** Обёртки `pa-audio` для систем
  - Что: Media3 SimpleBasePlayer (Android) и AVAudioSourceNode (Apple), чтобы сохранить системную интеграцию из MUS-007.
  - deps: MUS-008, MUS-007 · size: M · parallel: yes · веха: M6 · ref: 02 §5.3
  - done when: Bluetooth-кнопки и экран блокировки работают со своим движком.
- [ ] **MUS-012** Музыка из Plex, Jellyfin и Subsonic, офлайн-альбомы [P1]
  - Что: удалённые музыкальные библиотеки в общей модели; загрузка альбомов и плейлистов через pa-jobs.
  - deps: MUS-001, NET-014, NET-016, CORE-011 · size: M · parallel: yes · веха: M6 · ref: 01 §1.5 (музыка)
  - done when: альбом из Navidrome скачивается и играет офлайн.
- [ ] **MUS-013** Android Auto [P2]
  - Что: дерево MediaLibrarySession, медиашаблон, прохождение ревью Google Play.
  - deps: MUS-007 · size: M · parallel: yes · веха: M10 · ref: 02 §5.3
  - done when: проходит Desktop Head Unit; ревью Play одобрено.
- [ ] **MUS-014** CarPlay Audio [P2]
  - Что: CPListTemplate и CPNowPlayingTemplate; entitlement `com.apple.developer.carplay-audio` (заявка в PKG-012).
  - deps: MUS-007, PKG-012 · size: M · parallel: yes · веха: M10 · ref: 02 §5.3
  - done when: работает в симуляторе CarPlay; entitlement получен.
- [ ] **MUS-015** Продолжение музыки на другом устройстве, скробблинг, эквалайзер [P1/P2]
  - Что: синхронизация очереди и позиции (по аналогии со Spotify Connect, через SYNC); Last.fm и ListenBrainz с ключом пользователя [P2]; эквалайзер [P2].
  - deps: MUS-003, SYNC-003 · size: M · parallel: yes · веха: M6 · ref: 01 §4.11, §1.5
  - done when: очередь продолжается на втором устройстве с той же позиции.

### 3.16. TWI — Twitch

- [x] **TWI-001** `pa-twitch`: клиент Helix
  - Что: users, channels/followed, streams/followed, streams, channels, videos, clips, games и games/top, search, schedule, chat emotes, badges, color, settings; token bucket по заголовкам `Ratelimit-*`; курсорная пагинация с дедупликацией.
  - deps: CORE-006 · size: M · parallel: yes · веха: M7 · ref: 06 §1.1, §1.4
  - done when: тесты на записанных ответах; 429 обрабатывается корректно.
- [x] **TWI-002** Вход через Device Code Flow
  - Что: публичный client_id без секрета; QR и `user_code`; опрос токена; refresh заранее (меньше 10 мин до истечения) и строго по одному; повторный вход после 30 дней неактивности; отдельный токен на каждое устройство в SecretStore; минимальные scope.
  - deps: TWI-001, CORE-007 · size: M · parallel: yes · веха: M7 · ref: 06 §1.3, 04 §5.1.1
  - done when: тест гонки refresh; токены не появляются в логах и sync-журнале.
- [ ] **TWI-003** Прототип режима «без логина»
  - Что: локальное избранное плюс публичные данные через GQL против обязательного входа через DCF; оценка рисков и отчёт для DEC-006; проверка пунктов «[ПРОВЕРИТЬ]» из 06.
  - deps: TWI-001, TWI-004 · size: M · parallel: yes · веха: M7 · ref: S §6, 06 §1.3
  - done when: отчёт в `docs/adr/`, решение DEC-006 принято.
- [ ] **TWI-004** Воспроизведение: GQL → usher и удалённый конфиг
  - Что: PlaybackAccessToken → master playlist для live и VOD; `supported_codecs` по возможностям устройства; выбор качества и audio_only; client-id и хэши persisted queries — в `config/twitch/remote-config.json` (подпись, CORE-014); флаги `twitch.gql_playback`, `chapters_gql`, `chat_replay`, `archive`, `player=embed`; kill-switch по версии приложения.
  - deps: TWI-001, CORE-014 · size: M · parallel: yes · веха: M7 · ref: 06 §2.1, §2.6–2.7
  - done when: live и VOD играют на десктопе; при смене конфига функция выключается без релиза.
- [ ] **TWI-005** Запасной режим: официальный embed
  - Что: WebView с `player.twitch.tv` на iOS, Android и десктопе (параметр `parent` — страница-обёртка на GitHub Pages); на tvOS недоступен.
  - deps: TWI-004 · size: M · parallel: yes · веха: M7 · ref: 06 §2.2
  - done when: при флаге `player=embed` live и VOD играют с синхронизацией позиции через JS API.
- [ ] **TWI-006** Live: низкая задержка и реклама «как есть»
  - Что: prefetch-сегменты, индикатор задержки, кнопка «К эфиру», DVR в пределах окна плейлиста, «Смотреть с начала эфира» через VOD текущего стрима; распознавание рекламы по EXT-X-DATERANGE, плашка «Реклама · 0:15», запрет перемотки во время рекламы; никакой блокировки рекламы.
  - deps: TWI-004, PLAY-003 · size: M · parallel: yes · веха: M7 · ref: 06 §2.4, §6.4
  - done when: тест подтверждает, что рекламные сегменты не отбрасываются; задержка в low-latency-режиме меньше 5 с.
- [x] **TWI-007** Модель данных Twitch в библиотеке
  - Что: Streamer, Category, Favourite, LiveSession, Video, CategorySegment, Clip, TwitchAsset, ChatLog, RecordRule; миграции; секция «Стримы» (стример — как сериал, VOD — как эпизод).
  - deps: LIB-001, TWI-001 · size: M · parallel: yes · веха: M7 · ref: 06 §5.1, §5.3
  - done when: миграции и тесты запросов; поиск FTS5 индексирует VOD и стримеров.
- [ ] **TWI-008** Главы и категории VOD
  - Что: свой таймлайн из `channel.update`, `stream.online` и `stream.offline` (EventSub WebSocket или поллинг) для мониторимых каналов; GQL `moments` — по возможности, с вечным кэшем; запасной вариант — текущая категория с пометкой «≈»; фильтр VOD по категории.
  - deps: TWI-007, TWI-004 · size: M · parallel: yes · веха: M7 · ref: 06 §2.3
  - done when: фильтр «VOD, где была игра X» находит VOD по главам, а не по текущей категории канала.
- [ ] **TWI-009** Уведомления «X в эфире» без облака
  - Что: EventSub WebSocket с токеном пользователя (≈ 10 каналов из-за лимита стоимости) плюс поллинг `streams/followed` раз в 60–120 с на сервере, десктопе и Android (опционально в фоне); локальные уведомления; на iOS — только при активном приложении или BGAppRefresh, это честно указано в UI.
  - deps: TWI-002, TWI-007 · size: M · parallel: yes · веха: M7 · ref: 06 §1.2, S §6
  - done when: уведомление приходит не позже чем через 2 минуты после начала эфира на десктопе и сервере.
- [ ] **TWI-010** Live-чат: абстракция ChatSource
  - Что: анонимный IRC по WebSocket (`justinfan…`, только чтение); с логином — EventSub `channel.chat.message` и `POST /helix/chat/messages`; нормализованная модель ChatMessage; события модерации (удаление, бан, таймаут, режимы чата).
  - deps: TWI-002 · size: M · parallel: yes · веха: M7 · ref: 06 §3.1, §3.3
  - done when: тесты разбора IRCv3 и EventSub на записанных сообщениях.
- [ ] **TWI-011** Эмоуты и бейджи
  - Что: Twitch (включая анимированные), 7TV (актуальная версия API — проверить; zero-width; EventAPI), BTTV, FFZ; кэш emote-map на 1 ч и дисковый кэш; анимация декодируется один раз на эмоут.
  - deps: TWI-010 · size: M · parallel: yes · веха: M7 · ref: 06 §3.2
  - done when: чат крупного канала со 100 сообщениями в секунду не загружает CPU больше чем на 15 % на среднем телефоне.
- [ ] **TWI-012** Отрисовка чата
  - Что: кольцевой буфер на 500–1000 сообщений, батчинг отрисовки каждые 100–200 мс, пауза при прокрутке вверх, прореживание на TV; блок-лист, фильтр слов, подсветка упоминаний, «скрыть ботов»; ввод с автодополнением @ и эмоутов.
  - deps: TWI-011, UI-001 · size: M · parallel: yes · веха: M7 · ref: 06 §3.3–3.4
  - done when: widget-тесты; на TV-плотности чат читается с 3 м (шрифт ≥ 28 pt).
- [ ] **TWI-013** UI раздела Twitch (Flutter) и брендинг
  - Что: главная модуля (Live now, избранное с кольцом LIVE, «Продолжить», новые VOD с пометкой «удалится через N дней», скачанное, «Скоро в эфире», категории), Following, страница стримера (VOD с полосой глав, хайлайты, клипы, расписание), поиск; модуль называется «Стримы», есть «Открыть на Twitch», атрибуция; sub-only VOD — с замком и deeplink.
  - deps: TWI-007, TWI-008, UI-007, DEC-006 · size: L · parallel: yes · веха: M7 · ref: 06 §6.2–6.3, §1.5, 01 §2.6
  - done when: golden-тесты на 4 плотностях; без логина работает локальное избранное (или DCF — по DEC-006).
- [ ] **TWI-014** Плеер Twitch
  - Что: чат сбоку, снизу или поверх видео в зависимости от форм-фактора, режим «театр», скорость 0.5–3×, PiP, «только звук» через музыкальный мини-плеер, тост рейда (без автоперехода).
  - deps: TWI-013, TWI-006, TWI-012 · size: M · parallel: yes · веха: M7 · ref: 06 §6.4, §3.4
  - done when: ручной чек-лист на телефоне, планшете, десктопе и Android TV.
- [ ] **TWI-015** Ключи синхронизации и выбор источника
  - Что: `twitch:vod:<id>` для онлайн-просмотра и для файла; `twitch:rec:<stream_id>` с маппингом смещения на VOD; для live — «сейчас смотрю канал X»; порядок источников: файл на устройстве → сервер в LAN → сервер через iroh → онлайн.
  - deps: TWI-007, SYNC-003 · size: M · parallel: yes · веха: M7 · ref: 06 §5.2, 04 §1.8
  - done when: VOD, начатый онлайн на телефоне, продолжается из файла на сервере на TV с точностью ≤ 2 с.
- [ ] **TWI-016** [full] Чат-replay VOD онлайн
  - Что: GQL `VideoCommentsByOffsetOrCursor` за флагом, формат JSON TwitchDownloader, синхронизация по таймкоду (бинарный поиск), задержка ±N с.
  - deps: TWI-012, TWI-004 · size: M · parallel: yes · веха: M7 · ref: 06 §2.5, §3.4
  - done when: при перемотке чат перерисовывается и совпадает с позицией видео.
- [ ] **TWI-017** [full] Загрузчик VOD
  - Что: HLS-сегменты в 4–8 потоков, докачка по битовой карте сегментов, muted-сегменты помечаются, частичная загрузка (только глава), ремукс в MKV по умолчанию или MP4, ffmetadata (теги `TWITCH_*`, главы), вложения (обложка, аватар), сайдкары (`.chat.json.gz`, `.info.json`, `-thumb.jpg`), проверка ffprobe, атомарный rename; код только под cargo feature `twitch-archive`.
  - deps: TWI-004, CORE-011, CORE-012, CORE-013 · size: L · parallel: yes · веха: M7 · ref: 06 §4.1
  - done when: 3-часовой VOD скачивается с докачкой после kill; длительность сходится с Helix в пределах 2 %.
- [ ] **TWI-018** [full] Запись live
  - Что: старт по `stream.online` или поллингу; опрос плейлиста, дедупликация по MEDIA-SEQUENCE, файлы по 1–2 ч, обработка DISCONTINUITY; параллельная запись чата с program-date-time; главы из `channel.update`; финализация; опциональная замена записи официальным VOD.
  - deps: TWI-017, TWI-009, TWI-010 · size: L · parallel: yes · веха: M7 · ref: 06 §4.2
  - done when: 6-часовой эфир записан без пропусков при одном обрыве сети; чат синхронизирован с точностью ±2 с.
- [ ] **TWI-019** [full] Хранилище, имена файлов, импорт, правила записи
  - Что: раскладка `<login> [<user_id>]/ГГГГ/…` с настраиваемым шаблоном и санитизацией для Windows и SMB; дедупликация по video_id, stream_id и slug; импорт архивов yt-dlp, streamlink и TwitchDownloader; UI правил записи с retention (по умолчанию 14 дней или 10 эфиров) и предупреждением о ToS при включении.
  - deps: TWI-017 · size: M · parallel: yes · веха: M7 · ref: 06 §4.3, §2.7
  - done when: импорт папки TwitchDownloader сопоставляет файлы по video_id и подтягивает главы.
- [ ] **TWI-020** Канареечный тест Twitch в nightly CI
  - Что: проверка live, VOD, глав и чат-replay на публичном канале; при сбое — автоматический issue с типом ошибки (`gql_hash_invalid`, `integrity_required`, `usher_403`) и черновик правки remote-config.
  - deps: TWI-004, TWI-010 · size: S · parallel: yes · веха: M7 · ref: 06 §2.6–2.7 п.5
  - done when: искусственно испорченный хэш вызывает открытие issue.
- [ ] **TWI-021** Twitch на Android TV
  - Что: вход по QR, «Войти с телефона» (код уходит на телефон через локальный sync), живое превью при фокусе дольше 1 с, инфо-панель свайпом вниз, «Ответить» — карточка на телефоне.
  - deps: TWI-013, TV-002, SYNC-006 · size: M · parallel: yes · веха: M7 · ref: 06 §6.5
  - done when: весь сценарий проходится пультом; сообщение в чат отправляется с телефона.
- [ ] **TWI-022** Мультипросмотр 2–4 стримов [P2]
  - Что: сетка из нескольких live на десктопе и TV, переключение звука.
  - deps: TWI-014 · size: M · parallel: yes · веха: M10 · ref: 06 §6.4
  - done when: 4 потока 720p на десктопе без рассинхрона.

### 3.17. DIST — NAS-дистрибутив (Fedora bootc)

- [x] **DIST-001** Прототип образа
  - Что: `distro/Containerfile` FROM `quay.io/fedora/fedora-bootc:45@sha256:…` (пин по digest), Samba, Avahi, wsdd, firewalld, smartd, заглушки наших сервисов; ручная проверка `bootc switch`, `upgrade` и `rollback` на VM; ADR по фазе 0 (Go для установщика, нативный медиасервер или Quadlet).
  - deps: — · size: M · parallel: yes · веха: M8 · ref: 05 §1.4, §8 (фаза 0)
  - done when: VM переключается на образ и откатывается назад; ADR принят.
- [x] **DIST-002** Сборка образа в CI и политика подписи
  - Что: `podman build` → rechunk → `bootc container lint` → push в GHCR (`:<ver>`, `:beta`, `:stable`) → cosign (ключ в секрете или KMS) → SBOM (syft); на устройстве `policy.json` (по умолчанию reject, sigstoreSigned для нашего образа) и `registries.d` с sigstore attachments.
  - deps: DIST-001 · size: M · parallel: no · веха: M8 · ref: 05 §4.2 п.4, §6.2
  - done when: тест в CI — неподписанный образ отклоняется устройством.
- [ ] **DIST-003** Брендинг ОС
  - Что: имя по DEC-004, `os-release` (`ID_LIKE=fedora`), `dnf swap fedora-logos generic-logos`, grub, plymouth и motd без Fedora; формулировка «основано на Fedora Linux».
  - deps: DIST-001, DEC-004 · size: S · parallel: yes · веха: M8 · ref: 05 §7
  - done when: `grep -ri fedora` по брендинговым местам образа чист (кроме допустимой фактической ссылки).
- [x] **DIST-004** Установщик: обнаружение дисков (Go)
  - Что: `lsblk -J` + udev + `/sys/class/nvme/*/model`; имена вида «Samsung 990 PRO 1TB — /dev/nvme0n1»; десятичные размеры; фильтр loop, zram, sr и загрузочной флешки; признаки существующих данных (PTTYPE, blkid, mdraid); SMART.
  - deps: DIST-001 · size: M · parallel: yes · веха: M8 · ref: 05 §2.4 (чтение дисков)
  - done when: `go test` на записанных выводах lsblk и udev с 10 конфигурациями железа.
- [ ] **DIST-005** Установщик: разметка, `bootc install`, конфигурация, файл ответов
  - Что: GPT и UEFI (ESP 1 GiB, /boot 2 GiB, root на btrfs с `@var`, опционально раздел данных); `bootc install to-filesystem` из containers-storage с `--target-imgref ghcr.io/…/alpha-os:stable` и `--enforce-container-sigpolicy`; hostname, пользователь (yescrypt, wheel), NetworkManager connection, `/var/lib/alpha/install.json`; режим `--answers answers.json`.
  - deps: DIST-004 · size: L · parallel: no · веха: M8 · ref: 05 §2.4
  - done when: неинтерактивная установка в QEMU даёт загружаемую систему, которая отслеживает реестр, а не локальный путь.
- [ ] **DIST-006** Установщик: TUI (bubbletea)
  - Что: экран 1 (hostname, пользователь, пароль дважды, сеть DHCP или статика, выбор интерфейса); экран 2 (диск, «остаток под данные», опция LUKS); подтверждение вводом имени диска; прогресс; перезагрузка; ru и en.
  - deps: DIST-005 · size: M · parallel: yes · веха: M8 · ref: 05 §2.1, §2.3
  - done when: golden-тесты TUI (teatest); ручная установка на реальное железо.
- [ ] **DIST-007** Live-ISO
  - Что: `distro/installer/Containerfile` и `image-builder … generic-iso` с `--bootc-installer-payload-ref`; меньше 1.9 GiB (minimal-база, только нужные подпакеты linux-firmware, без документации и локалей, zstd); TUI стартует на tty1.
  - deps: DIST-005, DIST-002 · size: M · parallel: yes · веха: M8 · ref: 05 §6.2–6.3
  - done when: ISO собирается в CI, размер меньше 1.9 GiB, загружается в QEMU с OVMF.
- [ ] **DIST-008** QEMU-тесты установки в CI
  - Что: OVMF с Secure Boot и swtpm; установка по файлу ответов на виртуальный NVMe и 3 HDD → проверки по SSH (`bootc status`, health-эндпоинты, `smbclient`).
  - deps: DIST-007 · size: M · parallel: yes · веха: M8 · ref: 05 §6.2 (test)
  - done when: сценарий зелёный на каждом PR в `distro/`.
- [ ] **DIST-009** `alpha-updated`: агент обновлений
  - Что: check, download-only, apply, rollback через bootc; статус `bootc status --format=json`; API на unix-сокете (GET и POST update, rollback, настройки channel, auto и окна обслуживания); отключение `bootc-fetch-apply-updates.timer`.
  - deps: DIST-002 · size: M · parallel: yes · веха: M8 · ref: 05 §4.1–4.2
  - done when: go-тесты на моке bootc; ручная проверка на VM.
- [ ] **DIST-010** Снапшоты `/var` и откат состояния
  - Что: read-only снапшот `@var` перед apply, хранение 3 последних, маркер для отката `@var` при откате ОС; совместимость с expand→contract миграциями (LIB-002).
  - deps: DIST-009, LIB-002 · size: M · parallel: yes · веха: M8 · ref: 05 §4.2 п.6, §2.4
  - done when: после отката ОС база медиасервера возвращается к снапшоту и открывается старой версией.
- [ ] **DIST-011** greenboot и тесты обновления и отката
  - Что: `required.d` (сеть, alpha-admin `/healthz`, медиасервер `/health`, smbd, root смонтирован rw), `wanted.d` (пулы, GPU — отказ диска с данными не откатывает ОС), `red.d` (диагностика в `/var/lib/alpha/failed-updates` и уведомление); QEMU-сценарии «N-1 → N» и «плохое обновление → автооткат».
  - deps: DIST-009, DIST-008 · size: M · parallel: yes · веха: M8 · ref: 05 §4.2 п.5, §6.2
  - done when: оба сценария зелёные в CI.
- [ ] **DIST-012** Каналы и поэтапная раскатка
  - Что: подписанный манифест `channels/<ch>.json` (minisign) хостится на GitHub Pages (SITE-004); когорты `hash(device_id) mod 100`; переход по digest; `halt` как стоп-кран; окно обслуживания с учётом активных сессий медиасервера.
  - deps: DIST-009, SITE-004 · size: M · parallel: yes · веха: M8 · ref: 05 §4.2 п.3, п.8
  - done when: тест: устройство вне когорты не обновляется; `halt` останавливает раскатку.
- [ ] **DIST-013** `alpha-admin`: бэкенд (Go)
  - Что: аутентификация (пользователи ОС и роли), REST поверх D-Bus (systemd, NetworkManager, firewalld, UDisks2, logind), прокси к alpha-updated, самоподписанный TLS.
  - deps: DIST-001 · size: L · parallel: yes · веха: M8 · ref: 05 §5.1
  - done when: go-тесты на моках D-Bus; e2e в QEMU.
- [ ] **DIST-014** `alpha-admin`: веб-интерфейс
  - Что: вход, дашборд (CPU, RAM, температуры, диски), сеть, пользователи, обновления (версия, release notes, применить или откатить, канал), «Расширенный режим (Cockpit)» только для LAN; управление обновлениями ОС из клиентского приложения через API медиасервера.
  - deps: DIST-013, DIST-009, DSN-003 · size: L · parallel: yes · веха: M8 · ref: 05 §5.1, §8 (фаза 2)
  - done when: e2e Playwright: обновление и откат из UI в QEMU.
- [ ] **DIST-015** Сервис хранилища и мастер пулов
  - Что: выбор по цели («Максимум места», «Зеркало», «Баланс», «Надёжно») с расчётом ёмкости по логике `btrfs filesystem usage`; btrfs single, raid1, raid1c3, raid10; mdadm raid5 и raid6 с btrfs поверх; предупреждения SMART и SMR; сабволюмы media, photos, backups; systemd `.mount`; автоподключение библиотеки медиасервера.
  - deps: DIST-013 · size: L · parallel: yes · веха: M8 · ref: 05 §3.1–3.2
  - done when: QEMU-тест создаёт RAID5 из 3 дисков через API, переживает перезагрузку, библиотека видна серверу.
- [ ] **DIST-016** Обслуживание и уведомления
  - Что: smartd (короткий тест еженедельно, длинный ежемесячно), btrfs scrub, mdadm check по таймерам; `alpha-notify` (веб-UI, push в клиентское приложение через медиасервер, email по SMTP, ntfy или Telegram опционально); пороги заполнения 85 и 95 %.
  - deps: DIST-015 · size: M · parallel: yes · веха: M8 · ref: 05 §3.3
  - done when: деградация RAID в QEMU (отключённый диск) даёт уведомление меньше чем за 5 минут.
- [ ] **DIST-017** Мастер замены диска
  - Что: определение сбойного диска (модель, серийный номер, слот через ID_PATH и ledctl), выбор нового диска, `btrfs replace` или `mdadm --add`, прогресс ребилда.
  - deps: DIST-015, DIST-016 · size: M · parallel: yes · веха: M8 · ref: 05 §3.3 (замена диска)
  - done when: QEMU-сценарий замены диска в RAID1 и RAID5 зелёный.
- [ ] **DIST-018** Samba, пользователи и firewall
  - Что: генерация `smb.conf.d`, шары Media и Users/<name>, синхронизация пользователей (pdbedit), vfs_fruit и Time Machine, wsdd, Avahi (`<host>.local`); firewalld, зона home, только нужные порты; SSH выключен по умолчанию.
  - deps: DIST-013 · size: M · parallel: yes · веха: M8 · ref: 05 §5.2
  - done when: в QEMU-тесте `smbclient` с паролем пользователя админки работает; macOS видит шару.
- [ ] **DIST-019** Медиасервер в образе и VAAPI
  - Что: пакет `alpha-media` из SRV-018, systemd unit, группы render и video, `/var/lib/alpha-media`; Intel (intel-media-driver, libvpl) и AMD (mesa-va-drivers-freeworld) — по DEC-005; CI-проверка совпадения версий Mesa freeworld и Fedora.
  - deps: DIST-002, SRV-018, DEC-005 · size: M · parallel: yes · веха: M8 · ref: 05 §4.3, §5.2
  - done when: на Intel N100 и AMD APU `vainfo` показывает HEVC, сервер транскодирует аппаратно.
- [ ] **DIST-020** Вариант образа с NVIDIA [фаза 2]
  - Что: `Containerfile.nvidia` со стадией akmods, подпись kmod своим ключом (KMS), MOK-флоу в установщике и UI; CI не публикует образ, если kmod не собрался под новое ядро.
  - deps: DIST-019 · size: L · parallel: yes · веха: M8 · ref: 05 §4.3
  - done when: NVENC работает на реальной карте с Secure Boot после регистрации MOK.
- [ ] **DIST-021** Веб-установщик для NAS без монитора [фаза 2]
  - Что: HTTP-фронтенд того же бэкенда установщика, PIN на экране TUI, адрес `http://<ip>`.
  - deps: DIST-006, DIST-014 · size: M · parallel: yes · веха: M10 · ref: 05 §2.2 (E), §2.3
  - done when: установка с телефона в той же LAN; без PIN доступ запрещён.
- [ ] **DIST-022** LUKS2 + TPM2 [фаза 2]
  - Что: шифрование системного диска и пулов (`systemd-cryptenroll`, PCR 7), recovery-ключ показывается один раз и в виде QR; по умолчанию выключено.
  - deps: DIST-005 · size: M · parallel: yes · веха: M10 · ref: 05 §2.4 (шифрование)
  - done when: QEMU со swtpm загружается без ввода пароля; recovery-ключ открывает диск.
- [ ] **DIST-023** Режим «Медиа-архив»: mergerfs + SnapRAID [фаза 2]
  - Что: диски разного размера добавляются по одному; `snapraid sync` ежедневно, `scrub -p 5` еженедельно; отображение в мастере пулов.
  - deps: DIST-015 · size: M · parallel: yes · веха: M10 · ref: 05 §3.1–3.2
  - done when: QEMU-тест восстанавливает файлы после потери одного диска.
- [ ] **DIST-024** Хостинг ISO
  - Что: Cloudflare R2 за `download.<domain>`, GitHub Release с `.sha256` и `.sig` (файл меньше 2 GiB), зеркало SourceForge, опционально torrent.
  - deps: DIST-007, SITE-006 · size: S · parallel: yes · веха: M8 · ref: 05 §6.3, S §8
  - done when: страница загрузки ведёт на R2; контрольная сумма и подпись проверяются по инструкции.
- [ ] **DIST-025** (owner) Раннер с реальным железом
  - Что: self-hosted runner с Intel N100, AMD и NVIDIA для ночных тестов дистрибутива и транскодирования; только для main и `workflow_dispatch`.
  - deps: DIST-008 · size: M · parallel: yes · веха: M8 · ref: 05 §6.2, §8 (фаза 2)
  - done when: ночной прогон установки и обновления на железе зелёный.
- [ ] **DIST-026** Переход на новую major-версию Fedora (45 → 46)
  - Что: runbook и первый реальный переход: смена `FROM`, CI, 2–4 недели в beta, затем stable.
  - deps: DIST-011 · size: M · parallel: yes · веха: M10 · ref: 05 §4.2 п.9, §9
  - done when: устройства beta-канала обновились на 46 без откатов.
- [ ] **DIST-027** Бэкапы и снапшоты пулов [P2]
  - Что: снапшоты по расписанию, «Предыдущие версии» в Windows через `vfs_shadow_copy2`, бэкап на USB-диск, в S3 или B2, на другой NAS.
  - deps: DIST-015 · size: L · parallel: yes · веха: M10 · ref: 05 §8 (фаза 3)
  - done when: файл восстанавливается из «Предыдущих версий» в Windows.

### 3.18. DSN — дизайн-система и ассеты

- [x] **DSN-001** Инструмент и структура `design/`
  - Что: выбор Figma или Penpot (для open source рекомендуется Penpot; источник правды — JSON в git, а не файл макетов); каталоги tokens, icons/svg, brand, illustrations, motion, placeholders; `LICENSES.md` для шрифтов и иконок; Git LFS.
  - deps: — · size: S · parallel: yes · веха: M0 · ref: 01 §5.1–5.2
  - done when: ADR о дизайн-инструменте; структура создана.
- [x] **DSN-002** Токены W3C DTCG
  - Что: primitive (палитра, отступы 4–100, радиусы, тени), semantic (`bg/base`, `text/*`, `accent`, `accent/live`, `focus/*`, `badge/*`, `scrim`, `glass/thin|regular|thick`, `motion/*`), component (`poster/focus/scale` и т. п.), профили плотности (TV, desktop, tablet, phone).
  - deps: DSN-001 · size: M · parallel: yes · веха: M0 · ref: 01 §4.3–4.7, §4.10 п.1–2
  - done when: JSON проходит валидацию DTCG; значения из 01 §4 перенесены.
- [ ] **DSN-003** Пайплайн Style Dictionary и контраст-линт
  - Что: генерация `tokens.dart`, `Tokens.swift`, `tokens.css` (админки и сайт) в CI; проверка актуальности сгенерированного; проверка контраста WCAG AA для текстовых пар.
  - deps: DSN-002, REPO-001 · size: S · parallel: yes · веха: M0 · ref: 01 §5.2 (CI-шаги)
  - done when: изменение токена без перегенерации роняет CI; пара с контрастом меньше 4.5:1 роняет CI.
- [x] **DSN-004** Набор иконок и конвейер
  - Что: основа — Lucide (ISC) или Phosphor (MIT), доработка (штрих 1.75–2 px на 24 px); svgo и линтер (stroke, viewBox); генерация `Icon.*` для Flutter и asset catalog (или шаблоны SF Symbols) для Swift.
  - deps: DSN-001 · size: M · parallel: yes · веха: M1 · ref: 01 §4.8, §5.2
  - done when: 60 базовых иконок MVP доступны на обеих платформах; линтер в CI.
- [x] **DSN-005** Шрифты
  - Что: Inter Variable (OFL) для не-Apple платформ, проверка кириллицы (запасные варианты — Manrope или Golos UI); на Apple — системный SF; лицензии в `LICENSES.md`; табличные цифры.
  - deps: DSN-001 · size: S · parallel: yes · веха: M1 · ref: 01 §4.4
  - done when: образец типографики ru и en утверждён; шрифты подключены во Flutter.
- [x] **DSN-006** Бренд, иконки приложения, плейсхолдеры
  - Что: логотип; мастер иконки 1024+; layered image для tvOS (2–5 слоёв) и Liquid Glass (Icon Composer); Android adaptive (foreground, background, monochrome) и баннер TV 320×180; Windows ICO и ассеты MSIX; Linux hicolor, `.desktop` и AppStream; собственные плейсхолдеры постеров и обложек; иллюстрации пустых состояний.
  - deps: DSN-001, DEC-004 · size: L · parallel: yes · веха: M5 · ref: 01 §5.2 п.3
  - done when: все платформенные ассеты генерируются из мастер-файлов скриптом.
  - Итог: знак, словесный знак, акцент `color.lune`, Android (adaptive, themed, TV-баннер), Linux, Windows ICO, веб и плейсхолдеры — `design/brand/`; Apple-слои, MSIX, AppStream и пустые состояния перенесены в DSN-016.
- [ ] **DSN-007** Иконки источников и сервисов
  - Что: нейтральные иконки протоколов; Plex, Jellyfin, Emby, Twitch и TMDB — только официальные ассеты и строго по их brand guidelines (проверка в LEGAL-009).
  - deps: DSN-004, LEGAL-009 · size: S · parallel: yes · веха: M4 · ref: 01 §4.8, 06 §1.5
  - done when: на каждый brand-ассет есть ссылка на гайдлайн в `LICENSES.md`.
- [ ] **DSN-008** Макеты: TV как референс
  - Что: Home, сетка, Detail фильма и сериала, OSD плеера, настройки, источники, профили, «Состояние»; затем адаптации для телефона, планшета и десктопа; без ассетов и текстов Infuse.
  - deps: DSN-002 · size: L · parallel: yes · веха: M1 · ref: 01 §2, §4.10 п.6
  - done when: макеты в репо (экспорт или ссылка), владелец утвердил.
- [ ] **DSN-009** Макеты: музыка и Twitch
  - Что: Now Playing, альбом, артист, очередь, тексты, мини-плеер; главная Twitch, канал, VOD с полосой глав, плеер с чатом на всех плотностях.
  - deps: DSN-008 · size: M · parallel: yes · веха: M6 · ref: 01 §4.11, §2.6, 06 §6
  - done when: макеты утверждены владельцем.
- [ ] **DSN-010** Спецификация компонентов
  - Что: состояния default, focused, pressed, disabled и selected для каждого компонента из 01 §4.9 на каждой плотности; handoff-документ.
  - deps: DSN-008 · size: M · parallel: yes · веха: M1 · ref: 01 §4.9, §5.2 п.6
  - done when: `docs/dev/components.md` покрывает все компоненты MVP.
- [ ] **DSN-011** [M9] Полировка фокуса и параллакса
  - Что: tilt по тачпаду на tvOS, блик, layered images для hero и Top Shelf, затемнение при бездействии; коэффициенты подбираются на реальном TV.
  - deps: TV-003, TV-007 · size: M · parallel: yes · веха: M9 · ref: 01 §4.2
  - done when: коэффициенты зафиксированы в токенах; одобрено при проверке на телевизоре.
- [ ] **DSN-012** [M9] Материалы Glass и Liquid Glass, динамический акцент
  - Что: Glass для не-Apple платформ (blur 30–60, насыщенность, тонировка, блик по краю) с автоматическим fallback по профилю производительности; на Apple — нативный `glassEffect`; доминантный цвет арта с проверкой WCAG; Ambient-подсветка в плеере.
  - deps: UI-003 · size: L · parallel: yes · веха: M9 · ref: 01 §4.5–4.6
  - done when: на Fire TV Stick blur выключается автоматически; golden-тесты обоих режимов.
- [ ] **DSN-013** [M9] Движение (motion)
  - Что: spring для фокуса, переходы между страницами, hero-переход (постер → Detail, обложка → Now Playing), появление OSD, кроссфейд арта с debounce; соблюдение Reduce Motion.
  - deps: UI-003 · size: M · parallel: yes · веха: M9 · ref: 01 §4.7
  - done when: при включённом Reduce Motion нет параллакса и tilt (тест).
- [ ] **DSN-014** Скриншоты для сторов
  - Что: только public domain или CC BY контент (Blender Open Movies) и собственные плейсхолдеры, без арта TMDB; размеры под все сторы.
  - deps: DSN-008, UI-009 · size: M · parallel: yes · веха: M10 · ref: 01 §5.2 (правовые правила)
  - done when: комплекты для App Store, Google Play, Microsoft Store, Flathub и F-Droid готовы.
- [ ] **DSN-015** [M9] Аудит визуальной консистентности
  - Что: сравнение Flutter-экранов и tvOS SwiftUI с макетами на 4 плотностях, исправление расхождений, обновление golden-тестов.
  - deps: CI-009, DSN-012, TV-007 · size: M · parallel: yes · веха: M9 · ref: 01 §4.10
  - done when: список расхождений закрыт; golden-тесты зелёные.
- [ ] **DSN-016** Бренд: Apple-слои, MSIX, AppStream и иллюстрации пустых состояний
  - Что: остаток DSN-006, не нужный для релиза 0.1: layered image для tvOS (2–5 слоёв) и Liquid Glass (Icon Composer `.icon`), ассеты MSIX (Square44/150, Wide310, StoreLogo, scale-100…400), AppStream metainfo с иконками, иллюстрации пустых состояний; всё — из мастеров `design/brand/src` тем же генератором.
  - deps: DSN-006 · size: M · parallel: yes · веха: M5 · ref: 01 §5.2 п.3, design/brand/README.md
  - done when: ассеты генерируются `just design brand`, `brand:check` в CI зелёный.

### 3.19. PKG — упаковка и каналы публикации

- [ ] **PKG-001** Раскладка артефактов, подписи и `releases.json`
  - Что: имена артефактов, SHA256SUMS, подписи minisign или cosign, SBOM; манифест `releases.json` (версии компонентов, каналы, минимальная версия протокола) на GitHub Pages.
  - deps: REPO-010 · size: S · parallel: no · веха: M1 · ref: 04 §4.6–4.7
  - done when: клиент и сервер проверяют совместимость по `releases.json`.
- [ ] **PKG-002** Linux: AppImage, deb, rpm
  - Что: AppImage с zsync (`gh-releases-zsync`), deb и rpm через nfpm; libmpv и FFmpeg в бандле как LGPL-библиотеки.
  - deps: PKG-001, UI-001 · size: M · parallel: yes · веха: M1 · ref: 02 §6.1, 04 §4.7
  - done when: пакеты ставятся и запускаются на Ubuntu 24.04, Fedora 45 и Arch (CI-контейнеры).
- [ ] **PKG-003** Flathub
  - Что: манифест (`--share=network`, порталы для файлов, модули libmpv и FFmpeg), верификация издателя, PR в flathub.
  - deps: PKG-002 · size: M · parallel: yes · веха: M10 · ref: 02 §6.1, 04 §4.7
  - done when: приложение опубликовано на Flathub.
- [ ] **PKG-004** AUR
  - Что: пакеты `project-alpha-bin` и `-git` (имя — по DEC-004), автообновление PKGBUILD из релизного job.
  - deps: PKG-002 · size: S · parallel: yes · веха: M10 · ref: 02 §6.1
  - done when: `makepkg -si` проходит в CI-контейнере Arch.
- [ ] **PKG-005** Windows: установщик, автообновление, подпись
  - Что: MSIX и EXE (Inno Setup или WiX); Velopack для автообновления (ADR: один механизм для всех десктопов); подпись через SignPath Foundation (заявка владельца, условия проверить) или Certum Open Source.
  - deps: PKG-001, DEC-007 · size: M · parallel: yes · веха: M1 · ref: 02 §6.1, S §8
  - done when: подписанный установщик не вызывает предупреждение SmartScreen «неизвестный издатель»; обновление с beta на stable работает.
- [ ] **PKG-006** winget
  - Что: манифест в `winget-pkgs`, автоматизация через `wingetcreate` в релизном job.
  - deps: PKG-005 · size: S · parallel: yes · веха: M10 · ref: 02 §6.1
  - done when: `winget install` ставит текущую версию.
- [ ] **PKG-007** [store] Microsoft Store
  - Что: регистрация физлица (бесплатно), MSIX store-флейвора (подпись Store), листинг, политика конфиденциальности.
  - deps: PKG-005, LEGAL-007 · size: M · parallel: yes · веха: M10 · ref: 02 §6.1, S §6
  - done when: приложение опубликовано, донат-ссылок нет.
- [ ] **PKG-008** macOS: DMG, нотаризация, обновления, Homebrew
  - Что: Developer ID, hardened runtime, notarytool (API key), DMG; Velopack или Sparkle 2 по ADR; Homebrew cask (неподписанные casks удаляются с 01.09.2026).
  - deps: PKG-001, PKG-012, DEC-007 · size: M · parallel: yes · веха: M5 · ref: 02 §6.1, 04 §4.7
  - done when: Gatekeeper пропускает DMG без обходов; `brew install --cask` работает.
- [ ] **PKG-009** Android: APK в GitHub Releases
  - Что: подпись upload keystore из секретов, ABI split, совместимость с Obtainium.
  - deps: PKG-001, PLAY-AND-002 · size: S · parallel: yes · веха: M1 · ref: 02 §6.2 (этап 0)
  - done when: APK ставится поверх предыдущей версии; подпись проверена `apksigner`.
- [ ] **PKG-010** F-Droid
  - Что: метаданные в структуре fastlane, воспроизводимая сборка из исходников (включая FFmpeg и Rust), без проприетарных зависимостей (флейвор без Cast SDK), anti-features (Twitch — NonFreeNet).
  - deps: PKG-009 · size: L · parallel: yes · веха: M10 · ref: 02 §6.1, S §8
  - done when: MR в fdroiddata принят, сборка на серверах F-Droid проходит.
- [ ] **PKG-011** (owner) [store] Google Play
  - Что: аккаунт за $25, Play App Signing, internal → closed testing (≥ 12 тестеров, 14 дней) → production; отдельное ревью для Android TV; store-флейвор (без загрузок Twitch и донат-ссылок).
  - deps: PKG-009, DEC-007, LEGAL-001, TV-004, QA-011 · size: L · parallel: yes · веха: M10 · ref: 02 §6.1–6.2, S §4
  - done when: приложение для телефонов и TV опубликовано в production.
- [ ] **PKG-012** (owner) Apple Developer Program и конвейер TestFlight
  - Что: аккаунт за $99/год (когда готовы ≥ 2 Apple-таргета), App Store Connect API key в секретах, xcodebuild archive или fastlane, TestFlight для iOS, iPadOS, macOS и tvOS; сразу подать заявки на entitlement multicast networking и CarPlay audio.
  - deps: DEC-007 · size: M · parallel: yes · веха: M5 · ref: 02 §6.2 (этап 2), §4, §5.3
  - done when: сборки всех четырёх таргетов в TestFlight; заявки на entitlement отправлены.
- [ ] **PKG-013** Пакет для ревью App Store
  - Что: privacy nutrition label («данные не собираются»), export compliance, описание раздела Twitch без загрузок, демо-режим для ревьюера (встроенные короткие CC BY-ролики, источник не нужен).
  - deps: PKG-012, LEGAL-007, PLAY-APL-012, TV-011 · size: M · parallel: yes · веха: M10 · ref: 06 §2.7, S §4
  - done when: iOS, macOS и tvOS прошли ревью.
- [ ] **PKG-014** Матрица флейворов store и full по каналам
  - Что: таблица канал → флейвор (store: App Store, Google Play, Microsoft Store; full: GitHub, F-Droid, Flathub, AUR, winget, ISO, Docker); CI-матрица сборок; проверки CORE-013 на всех store-артефактах.
  - deps: CORE-013, PKG-001 · size: S · parallel: yes · веха: M1 · ref: S §6
  - done when: каждый релизный артефакт помечен флейвором; проверка store-сборок обязательна в релизе.
- [ ] **PKG-015** [store] Чаевые через IAP или удаление экрана донатов
  - Что: по DEC-003 — расходуемые покупки на $1, $5 и $10 (StoreKit 2, Play Billing), ничего не открывающие; иначе в store-флейворе экрана донатов нет.
  - deps: DEC-003, PKG-011, PKG-012 · size: M · parallel: yes · веха: M10 · ref: S §4
  - done when: покупки проходят в sandbox; F-Droid-сборка не содержит Play Billing.
- [ ] **PKG-016** Пакет соответствия LGPL
  - Что: исходники и патчи FFmpeg, mpv, libsmb2, libnfs и libass плюс скрипты сборки прикладываются к каждому релизу (или лежат в `project_alpha-thirdparty`); инструкции по перелинковке; `THIRD_PARTY_NOTICES` в бандлах.
  - deps: PLAY-001, PKG-001 · size: M · parallel: yes · веха: M1 · ref: 02 §3.1
  - done when: по инструкции из релиза можно пересобрать и подменить libavcodec в десктоп-сборке.
- [ ] **PKG-017** Автообновление десктопа: каналы и UX
  - Что: каналы stable и beta, проверка подписи обновления, UI «Доступно обновление» с changelog; для Store, Flathub и AUR обновление отдаётся каналу.
  - deps: PKG-002, PKG-005, PKG-008 · size: M · parallel: yes · веха: M5 · ref: 04 §4.7
  - done when: обновление с подделанной подписью отклоняется (тест).

### 3.20. QA — качество, тестовые данные, безопасность

- [x] **QA-001** Тест-стратегия
  - Что: `docs/dev/testing.md`: уровни тестов, что обязательно в PR, golden-тесты, device lab, ручные чек-листы; ответственные за платформы.
  - deps: REPO-001 · size: S · parallel: yes · веха: M0 · ref: 04 §4, 01 §4.10 п.5
  - done when: документ принят; ссылка в CONTRIBUTING.
- [x] **QA-002** Тестовые медиа-сэмплы
  - Что: `tests/media/` в LFS, только свободные лицензии: Blender Open Movies; синтетические сэмплы ffmpeg (все контейнеры и кодеки, HDR10, HDR10+, HLG, DV-метаданные, многоканальное аудио); субтитры ASS, PGS, VobSub; gapless-альбом; манифест с лицензиями.
  - deps: REPO-001 · size: M · parallel: yes · веха: M0 · ref: 01 §5.2, 02 §2
  - done when: манифест описывает каждый файл и его лицензию; общий размер набора меньше 2 ГБ.
- [ ] **QA-003** Матрица воспроизведения (smoke)
  - Что: автоматический прогон «файлы × платформы × движки»: открыть, играть 10 с, seek, сменить дорожку, без ошибок; отчёт-таблица.
  - deps: QA-002, PLAY-AND-002, PLAY-DSK-002 · size: M · parallel: yes · веха: M1 · ref: 02 §2.2
  - done when: матрица зелёная на Linux, Windows и эмуляторе Android; позже — на Apple (M5).
- [ ] **QA-004** E2E-тесты Flutter
  - Что: integration_test: онбординг → WebDAV из docker-compose → скан → Detail → воспроизведение (mock-движок) → прогресс в «Продолжить».
  - deps: UI-005, CI-012 · size: M · parallel: yes · веха: M1 · ref: —
  - done when: e2e выполняется в CI на Linux-десктопе и эмуляторе Android.
- [ ] **QA-005** Фаззинг
  - Что: cargo-fuzz для парсера имён, NFO, M3U и LRC, HLS-плейлистов, ответов SMB и WebDAV, сообщений синхронизации; ночной прогон.
  - deps: META-005, NET-004, SYNC-002 · size: M · parallel: yes · веха: M3 · ref: 04 §5.3
  - done when: ночной фаззинг 30 мин на цель без паник; найденные кейсы попадают в регрессионные тесты.
- [ ] **QA-006** Бюджеты производительности UI
  - Что: сетка на 10 000 элементов — 60 fps на среднем телефоне и Android TV-стике; холодный старт меньше 2 с на десктопе; профили и пороги в CI (там, где измеримо).
  - deps: UI-008 · size: M · parallel: yes · веха: M1 · ref: 02 §1.1, 01 §4.6 (fallback)
  - done when: отчёт по замерам; регрессия выше 20 % помечает PR.
- [ ] **QA-007** Тесты на плохой сети
  - Что: netem-профили (задержка, потери, ограничение полосы); плеер не встаёт в буферизацию при запасе полосы 2× к битрейту; регрессионный тест жалобы на Infuse.
  - deps: NET-003, PLAY-010 · size: M · parallel: yes · веха: M1 · ref: 01 §3 п.4
  - done when: тесты в CI; индикатор «сеть против битрейта» верен.
- [ ] **QA-008** Релизный чек-лист
  - Что: `docs/dev/release-checklist.md`: миграции и откат, совместимость sync и API ±1, проверки store-флейвора, ручные проверки HDR, DV, passthrough и AFR, лицензии, changelog.
  - deps: CI-011 · size: S · parallel: yes · веха: M1 · ref: 04 §4.2, 05 §6.2
  - done when: чек-лист пройден на первом beta-релизе.
- [ ] **QA-009** Аудит доступности и локализации
  - Что: проход каждого экрана с TalkBack, VoiceOver, Narrator и Orca и клавиатурой; масштаб текста 200 %; псевдолокализация и длинные русские строки; проверка плюрализации.
  - deps: UI-020, UI-021 · size: M · parallel: yes · веха: M1 · ref: 01 §1.5, §4.4
  - done when: блокеров нет; найденные проблемы оформлены задачами.
- [ ] **QA-010** Модель угроз и ревью безопасности
  - Что: учётные данные источников, сервер в LAN, pairing, iroh, токены Twitch, удалённый конфиг; ревью по чек-листу; сроки ответа по SECURITY.md.
  - deps: SRV-003, SYNC-005, CORE-007 · size: M · parallel: yes · веха: M4 · ref: 04 §5
  - done when: `docs/dev/threat-model.md` готов; найденные риски оформлены задачами.
- [ ] **QA-011** (owner) Лаборатория устройств и бета-программа
  - Что: список реальных устройств (Shield, Chromecast GTV, Fire TV, Apple TV 4K, HDR-телевизор, ресивер с Atmos, USB-DAC, Intel N100) и ответственных; бета-каналы (TestFlight, закрытое тестирование Play с ≥ 12 тестерами, beta для десктопа); сбор отзывов через Discussions.
  - deps: REPO-001 · size: M · parallel: yes · веха: M1 · ref: 02 §6.1, 01 §3 п.10
  - done when: `docs/dev/device-matrix.md` с владельцами устройств; набрано ≥ 12 бета-тестеров.

### 3.21. DOC — документация и ADR

- [x] **DOC-001** Базовый набор ADR
  - Что: `docs/adr/0000-template.md` и ADR 0001 (Rust-ядро, UniFFI и FRB), 0002 (Flutter и SwiftUI для tvOS), 0003 (нативные поверхности и движки плеера), 0004 (без серверов проекта; схемы синхронизации), 0005 (лицензия и DCO, по DEC-002), 0006 (раскладка монорепо и владение каталогами), 0007 (флейворы store и full).
  - deps: — · size: M · parallel: yes · веха: M0 · ref: S §3, §5–7, CLAUDE.md
  - done when: ADR 0001–0007 в статусе «принят» (0005 — после DEC-002).
- [ ] **DOC-002** Настройка среды разработчика
  - Что: `docs/dev/setup.md`: bootstrap на Linux, macOS и Windows, just-команды, сборка каждой платформы, эмуляторы и симуляторы, Apple-сборки без платного аккаунта.
  - deps: REPO-008 · size: S · parallel: yes · веха: M0 · ref: 04 §4.3
  - done when: новый контрибьютор собирает проект по документу (проверка «с нуля» в devcontainer).
- [x] **DOC-003** Обзор архитектуры
  - Что: `docs/dev/architecture.md`: ядро, клиенты, сервер, дистрибутив; режимы A, B и C; потоки данных; граф крейтов (генерируется).
  - deps: DOC-001 · size: M · parallel: yes · веха: M1 · ref: 04 §1.1, 02 §2.8
  - done when: схемы актуальны для M1; граф крейтов обновляется в CI.
- [ ] **DOC-004** Документация API
  - Что: рендер OpenAPI (Redoc, статически), `sync-protocol.md`, схема удалённого конфига Twitch, формат `releases.json` и манифестов каналов.
  - deps: SRV-002, SYNC-002 · size: S · parallel: yes · веха: M4 · ref: 04 §1.5, §1.8
  - done when: страницы публикуются на сайте (SITE-003).
- [ ] **DOC-005** Пользовательская документация: приложение
  - Что: ru и en: установка по платформам, источники (SMB, NFS, WebDAV, Plex, Jellyfin, Emby), именование файлов и исправление матчей (`{tmdb-…}`, NFO), профили и родительский контроль, синхронизация (LAN, папка, сервер), FAQ.
  - deps: UI-005 · size: M · parallel: yes · веха: M1 · ref: 01, 03 §4.6, S §2
  - done when: каждая функция M1–M3 описана; скриншоты без чужого арта.
- [ ] **DOC-006** Пользовательская документация: сервер
  - Что: Docker, бинарники, HW-ускорение по видам GPU, удалённый доступ (iroh, свой relay, Tailscale), резервное копирование БД.
  - deps: SRV-013 · size: M · parallel: yes · веха: M4 · ref: 04 §1.5–1.7
  - done when: установка по документу на чистый Ubuntu проходит.
- [ ] **DOC-007** Пользовательская документация: NAS-ОС
  - Что: запись ISO, установка, мастер пулов, замена диска, обновления и откат, Samba, NVIDIA.
  - deps: DIST-007 · size: M · parallel: yes · веха: M8 · ref: 05
  - done when: установка по документу на реальное железо проходит.
- [ ] **DOC-008** Пользовательская документация: Twitch и музыка
  - Что: вход, различия store- и full-сборок, архив и предупреждение о ToS, уведомления; bit-perfect, DSD, тексты песен.
  - deps: TWI-013, MUS-005 · size: S · parallel: yes · веха: M7 · ref: 06 §2.7, 02 §5
  - done when: страницы опубликованы на сайте.
- [ ] **DOC-009** Runbooks
  - Что: релиз; ротация ключей (cosign, MOK, ключ подписи обновлений, Android keystore, minisign удалённого конфига); переход на новую major-версию Fedora; инцидент «Twitch GQL сломался» (правка удалённого конфига, kill-switch).
  - deps: CI-011 · size: M · parallel: yes · веха: M7 · ref: 05 §7 (MOK), 06 §2.6
  - done when: каждый runbook прогнан хотя бы раз «на сухую».
- [ ] **DOC-010** Отчёт о качестве распознавания
  - Что: методология, корпус, метрики и сравнение с Jellyfin, Plex и Infuse по категориям; публикация на сайте.
  - deps: META-004, META-018 · size: M · parallel: yes · веха: M10 · ref: 03 §6.3
  - done when: отчёт воспроизводится скриптом из eval-репо.

### 3.22. LEGAL — юридические проверки

- [ ] **LEGAL-001** (owner) Письмо в TMDB
  - Что: некоммерческий open source с добровольными донатами и публикацией в сторах — запросить письменное подтверждение условий и атрибуции.
  - deps: — · size: S · parallel: yes · веха: M0 · ref: S §5, 03 §3.1
  - done when: письменный ответ сохранён в `docs/legal/`.
- [x] **LEGAL-002** Таблица условий провайдеров
  - Что: AniDB (регистрация UDP и HTTP клиента), AniList, Shikimori, OpenSubtitles, Fanart.tv, MusicBrainz, AcoustID, LRCLIB, Trakt (client secret в open source), TVDB, 7TV, BTTV, FFZ; что разрешено с ключом проекта и что только с ключом пользователя.
  - deps: — · size: M · parallel: yes · веха: M1 · ref: 03 §3, §3.5, 06 §3.2
  - done when: `docs/legal/providers.md`; регистрации клиентов (AniDB, Twitch dev, OpenSubtitles) выполнены владельцем.
- [x] **LEGAL-003** Лицензии данных маппингов аниме
  - Что: Anime-Lists и Fribb — лицензия не указана: открыть issue с запросом; anime-offline-database (ODbL, share-alike) — условия встраивания в бандл.
  - deps: — · size: S · parallel: yes · веха: M1 · ref: 03 §3.2, §4.7
  - done when: решение по каждому источнику записано; META-008 опирается на него.
- [ ] **LEGAL-004** (owner) Патенты кодеков, Dolby и DTS
  - Что: консультация по E-AC-3, TrueHD, DTS-HD, софтверному декодированию HEVC и RPM Fusion в образе; правила для бейджей без логотипов.
  - deps: — · size: M · parallel: yes · веха: M4 · ref: 02 §3.2, 05 §7
  - done when: заключение в `docs/legal/`; передано в DEC-005.
- [x] **LEGAL-005** Проверка названия и товарных знаков
  - Что: поиск конфликтов (EUIPO, USPTO, WIPO), свободные домены и хэндлы; без «Infuse», «Fedora» и «Twitch» в названии.
  - deps: — · size: S · parallel: yes · веха: M0 · ref: S §5, §11.4, 06 §1.5
  - done when: короткий список из 3–5 чистых имён передан в DEC-004.
- [ ] **LEGAL-006** (owner) Twitch: ToS, Developer Agreement, регистрация приложения
  - Что: заключение по воспроизведению через GQL и загрузкам (только full), тексты предупреждений; регистрация приложения в dev-консоли Twitch (публичный client_id).
  - deps: — · size: M · parallel: yes · веха: M7 · ref: 06 §1.5, §2.6
  - done when: заключение в `docs/legal/`; client_id выдан.
- [x] **LEGAL-007** Политика конфиденциальности
  - Что: проект данные не собирает; перечень запросов, которые устройство делает напрямую (TMDB, Twitch, OpenSubtitles и т. д.); crash-отчёты — только по opt-in и вручную; версии для сторов и сайта.
  - deps: — · size: S · parallel: yes · веха: M1 · ref: S §5, 04 §5.4
  - done when: страница опубликована; ссылки из приложения и сторов.
- [ ] **LEGAL-008** Политика донатов в сторах
  - Что: формулировки экрана «О программе» для store-флейвора, правила IAP-чаевых (Apple 3.1.1, платёжная политика Google) по решению DEC-003.
  - deps: DEC-003 · size: S · parallel: yes · веха: M10 · ref: S §4
  - done when: тексты согласованы и использованы в PKG-015 и UI-016.
- [ ] **LEGAL-009** Аудит лицензий зависимостей и ассетов
  - Что: cargo-about, flutter licenses и SwiftPM → `THIRD_PARTY_NOTICES`; запрет GPL в клиентах; совместимость с MPL; проверка ассетов (нет Infuse, Firecore, SF вне Apple), лицензии иконок и шрифтов, brand guidelines Plex, Jellyfin, Emby, Twitch и TMDB.
  - deps: CI-003 · size: M · parallel: yes · веха: M1 · ref: 02 §3, 01 §4.8, §5.2, CLAUDE.md
  - done when: notices генерируются автоматически в каждом релизе; чек-лист ассетов пройден.
- [ ] **LEGAL-010** Юридические вопросы дистрибутива
  - Что: соблюдение товарного знака Fedora, перечень пакетов RPM Fusion, принятие NVIDIA EULA пользователем, ссылки на SRPM и NEVRA в SBOM.
  - deps: DIST-001 · size: M · parallel: yes · веха: M8 · ref: 05 §7
  - done when: заключение в `docs/legal/distro.md`; ISO соответствует ему.

### 3.23. SITE — сайт

- [ ] **SITE-001** Каркас сайта
  - Что: GitHub Pages (репо — по DEC-001), статический генератор (Astro или mdBook — по ADR), ru и en, токены из `tokens.css`.
  - deps: DEC-001, DSN-003 · size: M · parallel: yes · веха: M1 · ref: S §9 (этап 10), §7
  - done when: сайт собирается в CI и публикуется.
- [ ] **SITE-002** Страница «Скачать»
  - Что: автоопределение платформы, ссылки на все каналы, контрольные суммы и подписи, инструкция по проверке.
  - deps: SITE-001, PKG-001 · size: M · parallel: yes · веха: M10 · ref: S §9 (этап 10)
  - done when: для каждой платформы показывается правильная кнопка; ссылки проверяются в CI.
- [ ] **SITE-003** Документация и API на сайте
  - Что: `docs/user` и рендер API (DOC-004), поиск по документации.
  - deps: SITE-001, DOC-005 · size: S · parallel: yes · веха: M4 · ref: —
  - done when: документация обновляется при мерже в main.
- [ ] **SITE-004** Хостинг подписанных манифестов
  - Что: `releases.json`, каналы дистрибутива (`channels/*.json`), зеркало удалённого конфига Twitch, appcast; подписи; CORS для клиентов.
  - deps: SITE-001 · size: S · parallel: yes · веха: M1 · ref: 04 §4.7, 05 §4.1
  - done when: клиент, сервер и `alpha-updated` читают манифесты с Pages и проверяют подписи.
- [ ] **SITE-005** Донаты и публичный roadmap
  - Что: страница со всеми площадками донатов и прозрачностью расходов; страница прогресса, генерируемая из `docs/ROADMAP.md` (процент по вехам и workstream).
  - deps: SITE-001, COMM-001, REPO-012 · size: S · parallel: yes · веха: M1 · ref: S §4, 01 §3 п.11
  - done when: прогресс обновляется автоматически после мержа.
- [ ] **SITE-006** (owner) Домен и поддомен загрузок
  - Что: домен (по DEC-004), HTTPS, `download.<domain>` → Cloudflare R2.
  - deps: DEC-004 · size: S · parallel: yes · веха: M8 · ref: 05 §6.3, S §4
  - done when: домен и поддомен работают по HTTPS.

### 3.24. COMM — сообщество и пожертвования

- [ ] **COMM-001** (owner) FUNDING.yml и площадки донатов
  - Что: аккаунты GitHub Sponsors, Open Collective (по возможности с фискальным хостом), Liberapay, Ko-fi, Boosty; `.github/FUNDING.yml`.
  - deps: — · size: S · parallel: yes · веха: M0 · ref: S §4, §7
  - done when: кнопка Sponsor в репозитории ведёт на все площадки.
- [ ] **COMM-002** GitHub Discussions и запросы функций
  - Что: категории (Q&A, Ideas с голосованием, Show and tell, Announcements, «Ошибки распознавания»); процесс «идея → голосование → статус → задача в ROADMAP».
  - deps: DEC-001 · size: S · parallel: yes · веха: M0 · ref: 01 §3 п.11
  - done when: шаблоны обсуждений созданы; процесс описан в CONTRIBUTING.
- [x] **COMM-003** README на русском и английском
  - Что: что это, статус, скриншоты (без чужого арта), ссылки на ROADMAP, скачивание, донаты, документацию; бейджи CI.
  - deps: REPO-001 · size: S · parallel: yes · веха: M0 · ref: S §1
  - done when: README отражает M0; ссылки проверяются в CI.
- [ ] **COMM-004** Good first issues из ROADMAP
  - Что: скрипт создаёт GitHub issues для S-задач с закрытыми deps и меткой `good first issue`, со ссылкой на строку ROADMAP; синхронизация статуса.
  - deps: REPO-012 · size: S · parallel: yes · веха: M1 · ref: —
  - done when: в репо есть ≥ 10 актуальных good first issues.
- [ ] **COMM-005** (owner) Каналы общения и анонсы
  - Что: выбор площадок (Matrix, Telegram, Discord), правила; шаблон анонса релиза; публикация changelog в Discussions.
  - deps: DEC-001 · size: S · parallel: yes · веха: M1 · ref: S §7
  - done when: ссылки в README; первый анонс опубликован.
- [ ] **COMM-006** Переводы силами сообщества
  - Что: Weblate (libre-план для open source) или Crowdin OSS: подключение ARB-файлов, документации и строк tvOS; правила ревью переводов.
  - deps: UI-020 · size: S · parallel: yes · веха: M10 · ref: —
  - done when: изменения из Weblate приходят PR-ами и проходят CI.

---

## 4. Критический путь

Оценка по size (S = 1, M = 5, L = 15 рабочих дней) и графу deps. Задачи на критическом пути берут первыми и не блокируют посторонними правками.

1. **Фундамент (M0, ~3–4 недели):** REPO-001 → CORE-001 → CORE-002 → UI-001 → CI-005 → CORE-010. Параллельно CORE-003, CORE-004 и TV-001 (smoke на Apple и Android), PLAY-001 (FFmpeg LGPL).
2. **Плеер-MVP (M1, самая длинная цепочка — ядро → медиатека → OSD):** REPO-001 → CORE-005 → LIB-001 → LIB-003 → LIB-006 → PLAY-009 → UI-013; плюс PLAY-001 → PLAY-002 → PLAY-DSK-002 → QA-003 и PLAY-AND-001 → PLAY-AND-002. **LIB-001 и PLAY-003** — общие контракты: их мержат как можно раньше, от них зависит большинство полос.
3. **Распознавание (M2):** META-005 → META-007 → META-014 → META-018, нужен также корпус META-002 → META-019. Корпус — L-задачи с ручной работой, их стоит начинать в первую неделю после M0.
4. **Синхронизация (M3):** LIB-001 → SYNC-001 → SYNC-002 → SYNC-006 → SYNC-012.
5. **Сервер (M4):** LIB-001 → SYNC-001 → SYNC-005 → SRV-003 → SRV-006 → SRV-013; транскод SRV-007 → SRV-008 (блокирован DEC-005, поэтому LEGAL-004 запускается рано).
6. **Самая длинная цепочка проекта — Apple (M5, ~14+ недель):** CORE-006 → PLAY-003 → PLAY-APL-001 → PLAY-APL-005 → PLAY-APL-006 → PLAY-APL-007 → PLAY-APL-012 → PKG-013. Свой движок (FFmpeg → VideoToolbox → AVSBDL) по оценке 02 занимает 2–4 месяца, поэтому PLAY-APL-001 стартует сразу после PLAY-003, не дожидаясь M4.
7. **Twitch (M7):** CORE-006 → TWI-001 → TWI-004 → TWI-017 → TWI-018 → SRV-016. Ядро Twitch (TWI-001…012) не зависит от UI, его можно делать с M1.
8. **NAS-ОС (M8, независимая ветка):** DIST-001 → DIST-004 → DIST-005 → DIST-007 → DIST-008 → DIST-011. Связь с остальным проектом — только DIST-019 (пакет сервера) и SITE-004/006.
9. **Публикация (M10):** внешние сроки, которые нельзя ускорить кодом (§7): ответ TMDB, 14 дней закрытого теста Google Play, ревью App Store, F-Droid и Flathub, подпись Windows.

## 5. Параллельные полосы после M0

Каждая полоса работает в своих каталогах (§0.4), поэтому полосы почти не конфликтуют. Владелец может одновременно запустить по одному агенту на полосу (≈ 12–16 агентов), а в широких полосах — 2–4 агента на независимые задачи.

| # | Полоса | Стартовые задачи (deps закрыты после M0) | Дальше | Агентов |
|---|---|---|---|---|
| A | Сеть | NET-001 → NET-002, NET-004, NET-006, NET-007, NET-009 | NET-003, NET-005, NET-008, NET-010, NET-011 | 2–4 |
| B | Медиатека | LIB-001 (один агент, серийно) → LIB-003, LIB-008, LIB-009, LIB-010, LIB-011, LIB-012, LIB-013 | LIB-004…007 | 1 → 4 |
| C | Распознавание | META-002, META-003, META-005, META-009 | META-006, META-007, META-010, META-013 | 2–3 |
| D | Плеерное ядро | PLAY-002, PLAY-003 (серийно), PLAY-006 | PLAY-004, PLAY-005, PLAY-007…010 | 2 |
| E | Плеер десктопа | PLAY-DSK-002 (после PLAY-002, PLAY-003, NET-008) | PLAY-DSK-003…007 | 1–2 |
| F | Плеер Android | PLAY-AND-002 (после PLAY-003) | PLAY-AND-003…010 | 1–2 |
| G | Flutter UI | UI-002 (серийно) → UI-003, UI-005, UI-008, UI-011, UI-020, UI-022 | UI-007, UI-009, UI-010, UI-013…017 | 2–4 |
| H | TV | TV-002 → TV-003, TV-004 | TV-005 | 1 |
| I | Синхронизация | SYNC-001 (после LIB-001) → SYNC-002, SYNC-004, SYNC-005 | SYNC-006…012 | 1–2 |
| J | Сервер | SRV-001 (после LIB-001) → SRV-002, SRV-004 | SRV-003, SRV-005…018 | 1–2 |
| K | Twitch-ядро | TWI-001 → TWI-002, TWI-004, TWI-007 | TWI-008…012, TWI-017 | 1–2 |
| L | NAS-ОС (Go, podman) | DIST-002, DIST-004, DIST-013 | DIST-005…018 | 2–3 |
| M | Apple (нужен macOS) | PLAY-APL-001 (после PLAY-003), TV-006 | PLAY-APL-002…011, TV-007…011 | 1–2 |
| N | Дизайн | DSN-002…005, DSN-008, DSN-010 | DSN-006, DSN-009 | 1–2 |
| O | Инфраструктура и бумаги | CI-006, CI-012, PKG-001, PKG-014, PKG-016, QA-001, QA-002, DOC-002, DOC-003, LEGAL-002, LEGAL-003, LEGAL-007, LEGAL-009 | CI-009…011, PKG-002…005 | 2 |
| P | Музыка (после LIB-003 и PLAY-002) | MUS-001, MUS-002 | MUS-003…012 | 1 |

Правила координации полос:
- **Контракты мержатся первыми и серийно:** CORE-005 (модель), LIB-001 (схема БД), PLAY-003 (`PlayerEngine`), SYNC-002 (протокол), SRV-002 (OpenAPI), UI-002 (каркас навигации), TV-002 (фокус-движок). Пока они в работе, зависящие задачи не стартуют.
- Изменения контракта после мержа оформляются отдельной задачей с `parallel: no` и пометкой в PR, какие полосы затронуты.
- Для задач, которым нужно железо (HDR, passthrough, AFR, NVIDIA, Apple TV), агент готовит код и автотесты, а проверку на устройстве закрывает владелец по чек-листу из QA-011.

## 6. Первая волна: стартовать сразу

У этих задач нет открытых deps; они в разных каталогах и не конфликтуют друг с другом.

1. **REPO-001** — каркас монорепо (первым смержить: после него открывается вторая волна).
2. **DOC-001** — ADR 0001–0007.
3. **PLAY-001** — LGPL-сборка FFmpeg для всех таргетов.
4. **PLAY-AND-001** — спайк встраивания плеера в Android с HDR и AFR.
5. **PLAY-DSK-001** — спайк встраивания libmpv во Flutter-десктоп.
6. **DIST-001** — прототип образа Fedora bootc.
7. **META-001** — таксономия, гайд по разметке и формат корпуса.
8. **DSN-001** — инструмент дизайна и структура `design/`.
9. **LEGAL-002** — таблица условий провайдеров метаданных и сервисов.
10. **LEGAL-003** — лицензии данных маппингов аниме.
11. **LEGAL-005** — проверка названия и товарных знаков.
12. **LEGAL-007** — политика конфиденциальности.

**Параллельно владелец:** DEC-001, DEC-002, DEC-007, LEGAL-001 (письмо в TMDB), LEGAL-004, LEGAL-006, COMM-001.

**Вторая волна (сразу после мержа REPO-001):** CORE-001, CORE-005, CI-001, REPO-003, REPO-006…009, REPO-012, META-003, META-005, QA-001, QA-002, DSN-002, COMM-003. После CORE-001 — CORE-002…004; после CORE-002 — UI-001; после CORE-003 — TV-001.

## 7. Внешние сроки: запускать заранее

| Что | Почему долго | Задача | Когда начать |
|---|---|---|---|
| Письменный ответ TMDB | Переписка, сроки не контролируем | LEGAL-001 | Неделя 1 |
| Юридическое заключение по кодекам | Блокирует транскод на сервере и образ ОС | LEGAL-004 → DEC-005 | M1 |
| Заключение по ToS Twitch, регистрация client_id | Нужно до первой публичной сборки с Twitch | LEGAL-006 | M1–M2 |
| Подпись Windows через SignPath | Заявка и проверка проекта | PKG-005 | После первой публичной сборки (M1) |
| Entitlement multicast и CarPlay от Apple | Выдаются по заявке, недели | PKG-012 | Сразу после покупки аккаунта Apple |
| Закрытый тест Google Play | ≥ 12 тестеров и 14 дней обязательны для новых личных аккаунтов | QA-011 → PKG-011 | За 1 месяц до релиза на Android |
| Ревью F-Droid и Flathub | Ручное ревью, воспроизводимые сборки | PKG-010, PKG-003 | За 1–2 месяца до M10 |
| Ответы AniDB, Anime-Lists, Fribb | Энтузиастские проекты, отвечают медленно | LEGAL-002, LEGAL-003 | Неделя 1 |
| Переход на Fedora 46 | Окно в 1–2 месяца на каждую major-версию | DIST-026 | Сразу после выхода новой версии Fedora |

## 8. Сводка по количеству задач

| Workstream | Задач | Workstream | Задач |
|---|---|---|---|
| DEC | 8 | SYNC | 12 |
| REPO | 13 | SRV | 18 |
| CI | 12 | MUS | 15 |
| CORE | 15 | TWI | 22 |
| NET | 20 | DIST | 27 |
| LIB | 15 | DSN | 15 |
| META | 25 | PKG | 17 |
| PLAY | 12 | QA | 11 |
| PLAY-AND | 10 | DOC | 10 |
| PLAY-DSK | 8 | LEGAL | 10 |
| PLAY-APL | 12 | SITE | 6 |
| UI | 22 | COMM | 6 |
| TV | 13 | **Всего** | **350** (342 задачи + 8 решений владельца) |
