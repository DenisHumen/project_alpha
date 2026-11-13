# Политика конфиденциальности / Privacy Policy

> Черновик для LEGAL-007 · версия 1 · 2026-11-13. Вступает в силу с первой публичной сборки.
> «project_alpha» — рабочее название. После DEC-004 его заменят на итоговое имя приложения и NAS-ОС.
> Ответы для анкет App Store и Google Play: [store-privacy.md](store-privacy.md).
> **Это не юридическая консультация.** Перед публикацией в сторах текст стоит показать юристу.
>
> *English version below — [jump to English](#english).*

---

## Русская версия

### 1. Коротко

- **Мы не собираем ваши данные.** У проекта нет серверов, нет учётных записей, нет аналитики, рекламы и трекинга. Разработчики project_alpha не получают и не видят ваши медиатеки, историю просмотров, поисковые запросы, IP-адреса и пароли.
- Всё хранится **на ваших устройствах**: в приложении, на вашем медиасервере или NAS, в ваших сетевых папках.
- Чтобы найти обложки, описания, субтитры и тексты песен, приложение **само обращается к публичным сервисам** (TMDB, AniList и др.). Эти сервисы видят ваш IP-адрес и сам запрос. Их список — в §4.
- Отчёты о сбоях сохраняются **только если вы это включили**. Отправить отчёт можете **только вы, вручную**.

### 2. К чему относится политика

К приложениям project_alpha на всех платформах: iOS, iPadOS, tvOS, macOS, Android, Android TV, Windows, Linux, а также к медиасерверу project_alpha и NAS-ОС на его основе. Все они — свободное ПО под лицензией MPL-2.0. Исходный код открыт: <https://github.com/DenisHumen/project_alpha>, и любое утверждение этой политики можно проверить по коду.

Политика не относится к сторонним сервисам, к которым обращается приложение (§4), к магазинам приложений (Apple, Google, Microsoft) и к сайтам площадок пожертвований. У каждого из них своя политика.

### 3. Что хранится на ваших устройствах

| Данные | Где хранится | Зачем |
|---|---|---|
| Медиатека: пути к файлам, названия, распознанные ID (TMDB, AniDB и др.), обложки, кэш метаданных | Локальная база SQLite и кэш приложения или вашего сервера | Показ медиатеки |
| Прогресс и история просмотра и прослушивания, избранное, профили | Там же | «Продолжить», синхронизация между вашими устройствами |
| Настройки | Там же | Работа приложения |
| Адреса ваших источников (SMB, NFS, WebDAV, Plex, Jellyfin, Emby, ваш сервер) | Там же | Подключение к источникам |
| **Пароли, токены и API-ключи** (сетевые папки, Plex/Jellyfin/Emby, Twitch, Trakt, OpenSubtitles, AniDB, ваши ключи TMDB, TVDB, LLM и др.) | **Системное защищённое хранилище:** Keychain (iOS, iPadOS, tvOS, macOS), Android Keystore, Windows Credential Manager/DPAPI, Secret Service (Linux). Если на Linux его нет — зашифрованный файл, и приложение предупредит об этом | Вход в ваши сервисы. В логи и отчёты секреты не попадают: они маскируются |
| Логи | Папка приложения, с ротацией | Диагностика; пути и токены из логов вырезаются |
| Отчёты о сбоях | Папка приложения, **только при включённой настройке** | Диагностика (§6) |

Удалить всё можно, удалив приложение или его данные в настройках системы, а на сервере — удалив папку данных сервера.

### 4. Что приложение отправляет напрямую сторонним сервисам

Запросы идут **с вашего устройства или с вашего сервера прямо в сервис**, минуя любые серверы проекта (их нет). Каждый сервис видит ваш **IP-адрес**, **User-Agent** (имя и версия приложения) и **содержимое запроса**. Как сервис обрабатывает эти данные, определяет его собственная политика конфиденциальности. Большинство интеграций можно выключить в настройках, а некоторые работают только после того, как вы их включите.

| Сервис | Когда | Что передаётся (кроме IP и User-Agent) |
|---|---|---|
| **TMDB** (The Movie Database) | Распознавание фильмов и сериалов, карточки, поиск | Название и год, разобранные из имени файла или папки, ID TMDB, текст вашего поиска, язык; ключ API проекта или ваш |
| **AniList** | Распознавание аниме | Название, ID |
| **Shikimori** | Распознавание аниме, русские названия | Название, ID |
| **AniDB** | Аниме, если вы ввели свой аккаунт AniDB | ed2k-хэш файла и размер, ID; ваш логин AniDB |
| **TVDB**, **TVmaze**, **Fanart.tv** | Дополнительные метаданные и арт (TVDB — только с вашим ключом) | Название, ID; ключ |
| **OpenSubtitles** (и выбранный вами второй провайдер субтитров) | Поиск и загрузка субтитров | Хэш файла (moviehash) и размер, ID фильма или серии, язык; ваш логин OpenSubtitles, если указан |
| **MusicBrainz**, **Cover Art Archive** | Метаданные музыки и обложки | Исполнитель, альбом, трек, ID MusicBrainz из тегов |
| **AcoustID** | **Только если вы включили** распознавание музыки по звуку | Акустический отпечаток трека и длительность |
| **LRCLIB** | Синхронизированные тексты песен | Исполнитель, трек, альбом, длительность |
| **Twitch** (API Helix, видео, чат) и сервисы эмодзи **7TV**, **BTTV**, **FFZ** | Раздел Twitch | Ваш токен Twitch после входа по коду устройства (пароль Twitch приложение не видит), ID каналов и VOD, сообщения чата, которые вы отправляете |
| **Trakt** | **Только если вы подключили** свой аккаунт Trakt | История и прогресс просмотра, оценки, список «посмотреть позже» — в ваш аккаунт Trakt |
| **Plex**, **Jellyfin**, **Emby** | Если вы добавили такой сервер | Запросы к **вашему** серверу; для Plex — также вход через plex.tv |
| **Релеи iroh** (публичные релеи n0 или ваш собственный) | Удалённый доступ к **вашему** серверу, если вы его включили | Соединение зашифровано от устройства до сервера; релей видит только IP-адреса и зашифрованный трафик, но не содержимое |
| **GitHub** (github.com, raw.githubusercontent.com, GitHub Pages, ghcr.io) | Обновление таблиц маппинга аниме и подписанного конфига приложения; проверка обновлений в сборках вне сторов; загрузка обновлений NAS-ОС | Обычный HTTP-запрос файла (IP, User-Agent) |
| **Хостинг загрузок** (например, Cloudflare R2) | Скачивание ISO и обновлений NAS-ОС | Обычный HTTP-запрос файла |
| **Провайдер LLM** (только ваш ключ API) | **Только если вы включили** помощь LLM в распознавании с собственным ключом | Имена файлов и папок, которые нужно распознать. По умолчанию используется локальная модель или функция выключена |

Синхронизация между **вашими** устройствами идёт напрямую по локальной сети (обнаружение через mDNS), через вашу сетевую папку (служебные файлы `.project_alpha/state/`) или через ваш собственный сервер. Проект при этом ничего не получает.

### 5. Чего мы не делаем

- Не встраиваем SDK аналитики, рекламы, атрибуции и трекинга.
- Не используем рекламные идентификаторы (IDFA, рекламный ID Android) и не отслеживаем вас в других приложениях и на сайтах.
- Не продаём и не передаём данные: у нас их нет.
- Не просим создавать учётную запись проекта.

### 6. Отчёты о сбоях и диагностика

- По умолчанию отчёты о сбоях **не сохраняются**. Если вы включите их в настройках, приложение сохранит отчёт **локально**.
- Из отчёта и логов автоматически удаляются пути к вашим файлам, токены и пароли.
- Приложение **ничего не отправляет само**. Чтобы помочь с ошибкой, вы можете экспортировать отчёт и **сами** приложить его к issue на GitHub. Перед отправкой его можно просмотреть. Опубликованный issue виден всем, и на него распространяется политика GitHub.
- Магазины и ОС могут собирать собственную диагностику по вашим системным настройкам (например, «Делиться с разработчиками приложений» на устройствах Apple или отчёты о сбоях Android). Это регулируется политикой Apple или Google и включается и выключается в настройках системы.

### 7. Пожертвования

Проект живёт на добровольные пожертвования. В сборках вне сторов есть ссылки на площадки (GitHub Sponsors, Open Collective, Liberapay, Ko-fi, Boosty). Платёж обрабатывает площадка по своей политике; проект видит только то, что площадка показывает получателю. Если в сборке для стора будут «чаевые» через встроенные покупки, платёж целиком обрабатывают Apple или Google. Приложение не получает ваших платёжных данных.

### 8. Дети

Приложение не предназначено специально для детей и ни у кого не собирает персональные данные, в том числе у детей до 13 лет (до 16 — в ЕС). Детские профили и родительский контроль работают локально.

### 9. Безопасность

Секреты хранятся в системном защищённом хранилище (§3). Запросы к внешним сервисам идут по HTTPS там, где сервис его поддерживает. Удалённый доступ к серверу по умолчанию выключен и использует шифрование. Об уязвимостях сообщайте по [SECURITY.md](../../SECURITY.md).

### 10. Ваши права

Мы не обрабатываем ваши персональные данные, поэтому запросы на доступ, исправление или удаление к проекту обычно неприменимы: все данные у вас, и вы управляете ими сами (§3). Чтобы воспользоваться правами (GDPR и др.) у стороннего сервиса из §4, обратитесь к этому сервису.

### 11. Изменения

Политика меняется только через публичный pull request в репозитории, и история правок видна на GitHub. Если в приложении появится новый внешний сервис, сначала обновится эта политика.

### 12. Контакты

- Вопросы: [GitHub Issues](https://github.com/DenisHumen/project_alpha/issues).
- Уязвимости и приватные обращения: [Private Vulnerability Reporting](https://github.com/DenisHumen/project_alpha/security/advisories/new).
- Email для обращений по приватности: *будет добавлен владельцем перед публикацией в сторах (Google Play требует контакт).*

---

<a id="english"></a>

## English version

### 1. Summary

- **We do not collect your data.** The project runs no servers, has no accounts, no analytics, no ads and no tracking. The project_alpha developers never receive or see your libraries, watch history, searches, IP address or passwords.
- Everything stays **on your devices**: in the app, on your own media server or NAS, in your own network shares.
- To fetch artwork, descriptions, subtitles and lyrics, the app **contacts public services directly** (TMDB, AniList, etc.). Those services see your IP address and the request itself; see §4.
- Crash reports are stored **only if you turn them on**, and are sent **only by you, manually**.

### 2. Scope

This policy covers the project_alpha apps on all platforms (iOS, iPadOS, tvOS, macOS, Android, Android TV, Windows, Linux), the project_alpha media server and the NAS OS built on it. All of them are free software under MPL-2.0. The source code is public at <https://github.com/DenisHumen/project_alpha>, so every statement here can be verified against the code.

It does not cover the third-party services the app contacts (§4), app stores (Apple, Google, Microsoft) or donation platforms. Each has its own policy.

### 3. Data stored on your devices

| Data | Where | Why |
|---|---|---|
| Library: file paths, titles, matched IDs (TMDB, AniDB, …), artwork, metadata cache | Local SQLite database and cache of the app or your server | Showing your library |
| Watch/listen progress and history, favourites, profiles | Same | "Continue", sync between your own devices |
| Settings | Same | App operation |
| Your sources (SMB, NFS, WebDAV, Plex, Jellyfin, Emby, your server) | Same | Connecting to them |
| **Passwords, tokens and API keys** (shares, Plex/Jellyfin/Emby, Twitch, Trakt, OpenSubtitles, AniDB, your own TMDB/TVDB/LLM keys, …) | **OS secure storage:** Keychain (iOS, iPadOS, tvOS, macOS), Android Keystore, Windows Credential Manager/DPAPI, Secret Service (Linux). If unavailable on Linux, an encrypted file is used and the app warns you | Signing in to your services. Secrets are masked in logs and reports |
| Logs | App folder, rotated | Diagnostics; paths and tokens are stripped |
| Crash reports | App folder, **only if enabled** | Diagnostics (§6) |

You can delete everything by uninstalling the app or clearing its data in system settings, and on the server by deleting its data folder.

### 4. What the app sends directly to third-party services

Requests go **from your device or your own server straight to the service**, never through project servers (there are none). Each service sees your **IP address**, the **User-Agent** (app name and version) and the **request content**, and handles them under its own privacy policy. Most integrations can be turned off in settings; some work only after you turn them on.

| Service | When | What is sent (besides IP and User-Agent) |
|---|---|---|
| **TMDB** (The Movie Database) | Movie/TV matching, details, search | Title and year parsed from the file or folder name, TMDB IDs, your search text, language; project or your own API key |
| **AniList** | Anime matching | Title, IDs |
| **Shikimori** | Anime matching, Russian titles | Title, IDs |
| **AniDB** | Anime, if you add your AniDB account | ed2k file hash and size, IDs; your AniDB login |
| **TVDB**, **TVmaze**, **Fanart.tv** | Extra metadata and artwork (TVDB with your own key only) | Title, IDs; key |
| **OpenSubtitles** (and an optional second subtitle provider you choose) | Subtitle search and download | File hash (moviehash) and size, movie/episode IDs, language; your OpenSubtitles login if set |
| **MusicBrainz**, **Cover Art Archive** | Music metadata and covers | Artist, album, track, MusicBrainz IDs from tags |
| **AcoustID** | **Only if you enable** audio fingerprinting | Audio fingerprint and duration |
| **LRCLIB** | Synced lyrics | Artist, track, album, duration |
| **Twitch** (Helix API, video, chat) and emote services **7TV**, **BTTV**, **FFZ** | Twitch section | Your Twitch token after device-code sign-in (the app never sees your Twitch password), channel and VOD IDs, chat messages you send |
| **Trakt** | **Only if you connect** your Trakt account | Watch history and progress, ratings, watchlist — to your Trakt account |
| **Plex**, **Jellyfin**, **Emby** | If you add such a server | Requests to **your** server; for Plex also sign-in via plex.tv |
| **iroh relays** (n0 public relays or your own relay) | Remote access to **your** server, if enabled | Connections are end-to-end encrypted between device and server; a relay sees only IP addresses and encrypted traffic, never content |
| **GitHub** (github.com, raw.githubusercontent.com, GitHub Pages, ghcr.io) | Updating anime mapping tables and the signed app config; update checks in non-store builds; NAS OS updates | Plain HTTP file request (IP, User-Agent) |
| **Download hosting** (e.g. Cloudflare R2) | Downloading the NAS OS ISO and updates | Plain HTTP file request |
| **LLM provider** (your own API key only) | **Only if you enable** LLM-assisted matching with your own key | File and folder names to be matched. By default a local model is used or the feature is off |

Sync between **your own** devices goes directly over your local network (mDNS discovery), through your network share (`.project_alpha/state/` files) or through your own server. The project receives nothing.

### 5. What we do not do

- No analytics, advertising, attribution or tracking SDKs.
- No advertising identifiers (IDFA, Android Advertising ID); no tracking across other apps or websites.
- No selling or sharing of data; we have none.
- No project account is required.

### 6. Crash reports and diagnostics

- Crash reports are **off by default**. If you enable them, they are stored **locally**.
- File paths, tokens and passwords are automatically removed from reports and logs.
- The app **never sends anything by itself**. To report a problem, you can export a report, review it and attach it **yourself** to a GitHub issue. Published issues are public and fall under GitHub's privacy policy.
- App stores and operating systems may collect their own diagnostics according to your system settings (for example Apple's "Share with App Developers" or Android crash reporting). That is governed by Apple's or Google's policies and controlled in system settings.

### 7. Donations

The project is funded by voluntary donations. Non-store builds link to donation platforms (GitHub Sponsors, Open Collective, Liberapay, Ko-fi, Boosty). The platform processes the payment under its own policy; the project sees only what the platform shows to recipients. If a store build offers optional tips via in-app purchase, the payment is processed entirely by Apple or Google; the app never receives your payment details.

### 8. Children

The app is not directed specifically at children and collects no personal data from anyone, including children under 13 (under 16 in the EU). Kids profiles and parental controls work locally.

### 9. Security

Secrets are kept in OS secure storage (§3). Requests to external services use HTTPS where the service supports it. Remote access to your server is off by default and encrypted. Report vulnerabilities as described in [SECURITY.md](../../SECURITY.md).

### 10. Your rights

We do not process your personal data, so access, correction or deletion requests to the project generally do not apply: all data is with you, and you control it (§3). To exercise your rights (GDPR, etc.) with a third-party service from §4, contact that service.

### 11. Changes

This policy changes only through a public pull request in the repository, and its history is visible on GitHub. Any new external service is added to this policy before it ships in the app.

### 12. Contact

- Questions: [GitHub Issues](https://github.com/DenisHumen/project_alpha/issues).
- Vulnerabilities and private reports: [Private Vulnerability Reporting](https://github.com/DenisHumen/project_alpha/security/advisories/new).
- Privacy contact email: *to be added by the owner before store submission (Google Play requires a contact).*
