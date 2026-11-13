# 05. Собственный дистрибутив для NAS / медиасервера

> Исследование для project_alpha, область: **дистрибутив** (ISO-образ appliance-системы на базе существующего Linux).
> Дата: 2026-11-13. Факты из интернета приведены со ссылками. Где источник один или слабый, это отмечено.

---

## 0. TL;DR (решения в одном абзаце)

Строим **неизменяемую (image-based) ОС на Fedora bootc** (`quay.io/fedora/fedora-bootc`, Fedora 45), а не на «классическом» Fedora Server. Вся ОС, включая наш медиасервер, веб-админку и агент обновлений, описывается одним `Containerfile`, собирается в GitHub Actions, подписывается cosign и публикуется как OCI-образ в GHCR. Обновления атомарные, с откатом (`bootc upgrade` / `bootc rollback`), их проверяют health-check'и greenboot-rs. Управляет ими наш демон `alpha-updated` через API, которым пользуются веб-админка и клиентское приложение. Установщик свой: лёгкий TUI (Go + bubbletea) на live-ISO, который собирается `image-builder ... generic-iso`. Он размечает диск и выполняет `bootc install to-filesystem` из образа, вшитого в ISO, поэтому интернет при установке не нужен. Хранилище по умолчанию: **btrfs** (single или RAID1/RAID1c3/RAID10) для зеркал и **mdadm RAID5/6 с btrfs поверх** для чётности. Для сборных медиа-архивов из дисков разного размера позже добавится режим **mergerfs + SnapRAID**. ZFS не входит в MVP. Используем загрузочную цепочку Fedora (shim, GRUB, ядро) без изменений, поэтому Secure Boot работает «из коробки» без собственного shim-review. Название не должно содержать «Fedora». Кодеки и драйверы NVIDIA требуют отдельного юридического решения.

---

## 1. Выбор базы

### 1.1 Что важно именно для нас

| Критерий | Почему важен |
|---|---|
| Атомарные обновления с откатом | Это требование владельца. NAS «у бабушки» не должен ломаться после обновления |
| Свежее ядро и Mesa | Аппаратное транскодирование на новых Intel iGPU и Arc (QSV/VAAPI), AMD VCN, новые NVMe и сетевые карты |
| Простая кастомизация | Маленькая команда. Нужен один понятный декларативный артефакт, а не «пересборка дистрибутива» |
| Контроль над обновлениями | Наши каналы stable/beta, поэтапная раскатка, запуск из UI |
| Готовые и подписанные загрузчик и ядро | Secure Boot без собственного shim |
| Экосистема | Пакеты Samba, Podman, NetworkManager, firewalld, smartmontools, Cockpit |

### 1.2 Сравнение кандидатов

| База | Модель обновлений | Плюсы | Минусы | Вердикт |
|---|---|---|---|---|
| **Fedora Server (пакетный)** | dnf, по пакетам | Привычно, гибко, есть Anaconda и Cockpit | Нет атомарности и нормального отката. Состояние системы «дрейфует». Каждые ~6 месяцев major-upgrade через `dnf system-upgrade`, который рискованно выполнять на тысячах машин | ❌ для appliance |
| **Fedora CoreOS** | rpm-ostree / bootc, автообновления Zincati | Атомарность, свежесть | Рассчитан на Ignition и облака, а не на интерактивную установку. Собственные стримы обновлений Fedora, которые нам пришлось бы «перебивать» | ⚠️ Годится как база для деривата (так делал [uCore от Universal Blue](https://github.com/ublue-os/ucore)), но проще взять fedora-bootc |
| **Fedora bootc (image mode)** | `bootc upgrade/switch/rollback`, ОС = OCI-образ | ОС собирается обычным `Containerfile`. Свой реестр, свои теги-каналы, подпись cosign. A/B-подобный откат через ostree-деплойменты. В Fedora идёт [инициатива Image Mode с целью production-ready к Fedora 45](https://pagure.io/Fedora-Council/tickets/issue/549) | Технология молодая, местами шероховата (см. §2.4). Fedora живёт 13 месяцев, раз в полгода нужно переходить на новую major-версию | ✅ **Рекомендация** |
| **CentOS Stream 10 / AlmaLinux bootc** | то же (bootc) | Цикл жизни около 5 лет, стабильность. Тот же инструментарий, что у Fedora bootc | Ядро 6.12 с бэкпортами, более старая Mesa. Поддержка новейших iGPU и Arc для транскодирования отстаёт. Часть пакетов придётся брать из EPEL | 🔁 Запасной вариант или будущий канал «LTS». Переход дёшев: меняется одна строка `FROM` |
| **Debian (как OpenMediaVault)** | apt | Огромная экосистема, стабильность | Атомарность пришлось бы строить самим (RAUC/Mender A/B, собственные разделы). Ядро в stable старое | ❌ слишком много собственной инженерии |
| **Ubuntu Core** | snaps, A/B | Атомарный, есть A/B | Всё упаковывается в snap. Привязка к Snap Store или платному Brand Store Canonical. Samba и драйверы GPU в snap неудобны | ❌ |
| **NixOS** | поколения, откат | Декларативность, откат «из коробки» | Высокий порог входа, особые подходы к упаковке. Нет понятия «подписанный образ ОС из реестра» без своей обвязки | ❌ для небольшой команды |
| **openSUSE MicroOS / Aeon / Leap Micro** | transactional-update + снапшоты btrfs | Зрелый атомарный подход, btrfs нативен (Rockstor живёт на openSUSE) | Модель «пакеты + снапшоты», а не «образ из CI». Меньше сообщество, будущее MicroOS-ветки менялось | ⚠️ рабочий вариант, но bootc лучше подходит под наш CI/CD |

**Почему не «тяжело кастомизированный Fedora Server», как предложил владелец.** Требование «атомарно, с откатом, управляется из UI» в пакетной модели на практике не выполнимо. Снапшоты btrfs + snapper дают откат, но не дают **воспроизводимой идентичной ОС** на всех устройствах. bootc даёт и то и другое: все устройства на канале `stable` побитово запускают один и тот же `/usr`, а «кастомизация» превращается в обычный `Containerfile`. Fedora Server при этом остаётся источником пакетов: мы берём те же RPM, только в образном режиме.

### 1.3 Уроки существующих NAS-ОС

| ОС | База и модель | Чему учимся |
|---|---|---|
| **TrueNAS SCALE / CE (25.x → 26)** | Debian, ZFS. Обновление = новый boot environment (ZFS-клон) с выбором в загрузчике. [В 2026 roadmap: ядро 6.18 LTS, OpenZFS 2.4, LXC](https://linuxiac.com/?p=206861) | Образные обновления + boot environments — правильная модель. Пользователь не трогает shell. Минус: ZFS отпугивает новичков (нельзя добавить один диск к RAIDZ, хотя RAIDZ expansion уже появился) |
| **HexOS** | UI-надстройка над TrueNAS. [1.0 вышла в апреле 2026](https://docs.hexos.com/blog/2026-04-02) | Хороший UX-ориентир: «мастер для людей» поверх зрелого движка. Но [темп развития критикуют](https://nascompares.com/news/is-hexos-in-trouble-mid-2026-update/), а зависимость от чужого движка тормозит продукт |
| **Unraid** | Slackware, загрузка с USB в RAM, собственный parity-массив | Пользователям медиа-библиотек очень нравится смешивать диски разного размера и добавлять по одному. Это аргумент за режим mergerfs + SnapRAID |
| **OpenMediaVault** | Debian + плагины | Гибко, но обновления пакетные и хрупкие. UI устарел. Наглядно показывает, чего не делать |
| **Rockstor** | openSUSE + btrfs | btrfs как основная ФС для NAS — рабочий путь. RAID5/6 на btrfs они тоже не рекомендуют |
| **ZimaOS / CasaOS** | Собственная ОС, магазин приложений-контейнеров. [1.6 в апреле 2026 с упором на RAID recovery](https://shop.zimaspace.com/sv/pages/zimaos-1-6-0-features-fixes-upgrade-issues) | Очень простой онбординг и красивый веб-UI — ориентир по UX |
| **Synology DSM** | Собственная, mdadm + LVM + btrfs (SHR) | Золотой стандарт UX. **mdadm для избыточности + btrfs поверх для снапшотов и контрольных сумм** — проверенная в массовом продукте схема, её и берём для режима «чётность» |

### 1.4 Рекомендация по базе

- **База:** `FROM quay.io/fedora/fedora-bootc:45`. Если окажется, что образ слишком велик, берём вариант minimal или собираем через `bootc-base-imagectl` из минимального набора пакетов.
- **Переходы на major-версии** (45 → 46 → 47) — это наш собственный выпуск. Меняем тег в `FROM`, прогоняем CI и тесты, катим в `beta`, затем в `stable`. Fedora живёт около 13 месяцев. Например, [Fedora 43 достигает EOL 9 декабря 2026](https://endoflife.ai/article-fedora-43-eol), а [Fedora 45 вышла в начале ноября 2026 после двух переносов](https://phoronix.com/news/Fedora-45-Second-NO-GO). Поэтому на каждую major-версию закладываем окно в 1–2 месяца.
- **План Б:** `quay.io/centos-bootc/centos-bootc:stream10` или AlmaLinux bootc. Нужен, если стабильность важнее свежести GPU-стека, или как отдельный канал `lts`.

---

## 2. Установщик

### 2.1 Требования (от владельца)

1. **Экран 1:** hostname, имя пользователя, пароль; сеть: DHCP или статика (IP/маска, шлюз, DNS).
2. **Экран 2:** выбор целевого (системного) диска с человекочитаемыми именами, например `Samsung 990 PRO 1TB — /dev/nvme0n1`.
3. Дальше всё ставится без участия пользователя, затем перезагрузка.

### 2.2 Варианты

| Вариант | Как | Плюсы | Минусы |
|---|---|---|---|
| **A. Кастомизированный Anaconda (GTK) + kickstart + addon** | Kickstart с командой [`bootc`](https://fedoramagazine.org/introducing-the-new-bootc-kickstart-command-in-anaconda/) или `ostreecontainer`, свой addon для экрана | Проверенный движок разметки, LUKS, RAID | GTK UI [объявлен к удалению после Fedora 45](https://communityblog.fedoraproject.org/?p=14910). Addon-API сложный. UI тяжёлый, брендировать трудно |
| **B. Anaconda WebUI** | [По умолчанию в Spins и KDE с Fedora 43, для Server Edition — с Fedora 45](https://communityblog.fedoraproject.org/?p=14910) | Современный UI на Cockpit/PatternFly | Не рассчитан на сторонние экраны и брендинг. Поддержка кастомных шагов и офлайн-установки bootc не документирована |
| **C. `bootc-image-builder` → `anaconda-iso`** | Готовая сборка ISO из bootc-образа | Ноль кода | [bootc-image-builder архивирован и влит в `image-builder`](https://osbuild.org/docs/bootc/deprecation-notice/). Тип `anaconda-iso` [в новом инструменте недоступен и будет удалён в RHEL 11](https://osbuild.org/docs/developer-guide/projects/image-builder/advanced/bootc/isos/). Ставит на первый найденный диск, а нам нужен выбор диска. Годится только как прототип на неделю |
| **D. Свой TUI (Go + bubbletea или Rust + ratatui) на live-ISO** | Live-ISO собирается `image-builder ... generic-iso` из нашего «installer»-контейнера, TUI стартует автоматически на tty1 | Полный контроль над UX и брендом. Маленький объём (около 2–4 тыс. строк). Работает без мыши и браузера. Можно повторить внешний вид установщика TrueNAS | Разметку, RAID и LUKS пишем сами (это немного: `sgdisk`, `mkfs`, `cryptsetup`, `bootc install to-filesystem`) |
| **E. Маленький веб-установщик на live-ISO** | Live-ISO поднимает HTTP на :80, пользователь заходит с телефона или ноутбука | Удобно для headless-NAS без монитора. Тот же стек, что у веб-админки | Нужен DHCP в сети, чтобы узнать адрес. Безопасность (кто угодно в LAN может установить систему) решается PIN-кодом на экране |

### 2.3 Рекомендация: **D + E на одном движке**

Один бинарник `alpha-installer` (Go):
- **Backend** (общий) отвечает за обнаружение дисков, валидацию, разметку и запуск `bootc install`.
- **Frontend 1** — TUI (bubbletea), стартует на tty1.
- **Frontend 2 (фаза 2)** — HTTP-UI на том же порту, что потом займёт веб-админка. На экране TUI выводится `http://<ip> PIN 4831`. Это закрывает сценарий «NAS без монитора».

Go выбран потому, что бэкенд веб-админки и агент обновлений тоже удобно писать на Go (D-Bus, systemd, Podman API — всё есть). Rust + ratatui тоже годится. Выбор языка лучше согласовать с исследователем бэкенда медиасервера.

**Поток установщика:**

```
[Загрузка live-ISO] → tty1: alpha-installer
 ├─ Экран 1: Hostname | Пользователь | Пароль ×2 | Сеть: (•) DHCP ( ) Статика [IP/CIDR, шлюз, DNS1, DNS2]
 │           + выбор интерфейса, если их несколько (показываем MAC, link up/down, скорость)
 ├─ Экран 2: Системный диск
 │     ( ) Samsung SSD 990 PRO 1TB      1.0 TB   NVMe   /dev/nvme0n1   S/N …1234
 │     ( ) WDC WD40EFRX-68N32N0         4.0 TB   SATA   /dev/sda       HDD
 │     [x] Использовать остаток диска под данные (если диск один)
 │     [ ] Шифровать системный диск (LUKS2 + TPM2)          ← опционально
 ├─ Экран 3: Подтверждение «ВСЕ ДАННЫЕ НА /dev/nvme0n1 БУДУТ УДАЛЕНЫ», ввести «yes»/имя диска
 └─ Автоматически: разметка → bootc install → запись конфигурации → reboot
```

RAID для дисков с данными в установщике **не создаётся** (как в TrueNAS). Это делает мастер в веб-админке после первой загрузки. Так установщик остаётся простым, а данные на остальных дисках не трогаются.

### 2.4 Технические детали установки

**Чтение дисков.** Основной источник:

```bash
lsblk -J -b -d -o NAME,PATH,TYPE,MODEL,VENDOR,SERIAL,WWN,SIZE,TRAN,ROTA,RM,HOTPLUG,PTTYPE
```

Дополнительно:
- `udevadm info --query=property /dev/sdX` даёт `ID_MODEL`, `ID_SERIAL_SHORT`, `ID_BUS`, `ID_PATH`. Для NVMe модель берётся из `/sys/class/nvme/nvme0/model`.
- Отфильтровываем `loop`, `zram`, `sr*` и **сам загрузочный USB**: определяем его через `findmnt /run/initramfs/live` → родительское устройство. Помечаем «уже содержит данные» по `PTTYPE` и `blkid`: «найден btrfs-том "media"», «найден mdraid»).
- Размер показываем в **десятичных единицах**, как на коробке (1 TB = 10¹² байт). Тогда «990 PRO 1TB» показывается как 1.0 TB, а не как 931 GiB.
- Человеческое имя = `MODEL` с нормализованными пробелами. Если модель пустая (часть USB-мостов), показываем `VENDOR + размер + TRAN`.
- По запросу показываем SMART-статус (`smartctl -H -j`) и сразу предупреждаем о проблемном диске.

**Разметка системного диска** (GPT, UEFI-only; BIOS/CSM не поддерживаем, для NAS 2026 года это нормально):

| # | Раздел | Размер | ФС |
|---|---|---|---|
| 1 | ESP | 1 GiB | vfat |
| 2 | /boot | 2 GiB | ext4 (или xfs) |
| 3 | root (+ /var) | 64 GiB, если включено «остаток под данные», иначе весь диск | btrfs; опционально LUKS2 |
| 4 | data (опц.) | остаток | btrfs, монтируется в `/srv/data/<pool>` |

- В корневом btrfs заводим **отдельный сабволюм для `/var`** (`@var`). bootc/ostree откатывает `/usr` и трёхсторонне сливает `/etc`, а **`/var` не откатывает**. Поэтому перед каждым обновлением наш агент делает снапшот `@var` (база медиасервера, конфиги). Это даёт полный откат «ОС + состояние приложения» (подробнее в §4).
- `bootc install to-filesystem` устанавливает в уже размеченную и смонтированную ФС. Для MVP можно начать с `bootc install to-disk --filesystem btrfs --wipe` (там есть и `--block-setup tpm2-luks`), а на свою разметку перейти, когда понадобится раздел данных.

**Источник образа — офлайн.** Payload-образ вшивается в container storage ISO (`--bootc-installer-payload-ref` в [image-builder](https://osbuild.org/docs/developer-guide/projects/image-builder/advanced/bootc/isos/)). Установка идёт из `containers-storage:`, а **отслеживаемая ссылка** (`--target-imgref ghcr.io/<org>/alpha-os:stable`) указывает на реестр. Известная ловушка: если поставить из локального архива без target imgref, [система «привяжется» к локальному пути и никогда не обновится](https://github-redirect.dependabot.com/rhinstaller/anaconda/discussions/5197). Это обязательный пункт в QA-чеклисте.

Примерная команда в установщике:

```bash
podman run --rm --privileged --pid=host --security-opt label=type:unconfined_t \
  -v /dev:/dev -v /mnt/target:/target \
  containers-storage:localhost/alpha-os:payload \
  bootc install to-filesystem --target-imgref ghcr.io/<org>/alpha-os:stable \
     --enforce-container-sigpolicy --karg console=tty0 /target
```

Флаги сверить с текущей версией bootc на этапе прототипа.

**Пост-установочная конфигурация.** Не используем kickstart, а пишем файлы прямо в целевой `/etc` и `/var`:
- `/etc/hostname`;
- пользователь: через `systemd-sysusers` или `useradd --root`. Пароль храним в хеше yescrypt. Пользователь входит в группу `wheel` и получает админ-роль в нашей админке;
- `/etc/NetworkManager/system-connections/*.nmconnection` для статики, DHCP идёт по умолчанию;
- `/var/lib/alpha/install.json` с параметрами первого запуска (UUID устройства, канал обновлений, язык).

**Шифрование (опционально).** LUKS2 с авторазблокировкой через TPM2 (`systemd-cryptenroll --tpm2-device=auto --tpm2-pcrs=7`). Recovery-ключ показываем один раз и предлагаем скачать или сфотографировать QR. Для дисков с данными шифрование настраивается отдельно, в мастере пулов. По умолчанию шифрование **выключено**: для домашнего NAS потеря ключа страшнее кражи.

**UEFI и Secure Boot.**
- Мы **не пересобираем** shim, GRUB и ядро, а используем подписанные Fedora. Тогда Secure Boot работает на любых машинах, доверяющих Microsoft UEFI CA. Собственный shim потребовал бы [прохождения shim-review и подписи Microsoft](https://techcommunity.microsoft.com/blog/hardwaredevcenter/updated-uefi-signing-requirements/1062916). Это месяцы и аудит. Дополнительно сейчас идёт переход [со старого Microsoft CA 2011 на UEFI CA 2023 (двойная подпись)](https://techcommunity.microsoft.com/t5/hardware-dev-center/signing-with-the-new-2023-microsoft-uefi-certificates-what/ba-p/4455787), и Fedora сама обеспечивает совместимость.
- Внедеревные модули (NVIDIA, ZFS) подписываем **своим ключом**. Пользователь один раз регистрирует его через MOK (`mokutil --import`, подтверждение в MokManager при перезагрузке). Так делает [Universal Blue для ZFS/NVIDIA kmods](https://universal-blue.discourse.group/t/ucore-zfs-kmods-are-now-signed-for-secureboot/383). В установщике это галочка «У меня NVIDIA → подготовить регистрацию ключа». Без NVIDIA и ZFS MOK не нужен. Это один из аргументов держать MVP на внутриядерных драйверах.

---

## 3. Хранилище и RAID

### 3.1 Сравнение технологий (состояние на ноябрь 2026)

| Технология | Статус | Плюсы | Минусы | Для нас |
|---|---|---|---|---|
| **btrfs single / RAID1 / RAID1c3 / RAID10** | В ядре, стабильно, основная ФС Fedora | Контрольные суммы, самовосстановление при зеркале, снапшоты, scrub, диски разного размера в RAID1, добавление и удаление дисков на лету | RAID1 = 50% ёмкости | ✅ **По умолчанию для 1–2 дисков и «зеркала»** |
| **btrfs RAID5/6** | По-прежнему **нестабильно, не рекомендуется для данных, которые нельзя восстановить**. В 2026 году идут лишь подготовительные патчи: [минимум 4 устройства для RAID6](https://lkml.iu.edu/hypermail/linux/kernel/2605.1/06764.html), RFC RAID5 поверх raid-stripe-tree (пока только для zoned-устройств) | — | write hole, проблемы scrub/repair | ❌ не предлагать |
| **mdadm RAID1/5/6/10** | Зрелый, в ядре | Надёжная чётность, расширение (`--grow`), понятная замена диска, write-intent bitmap, PPL против write hole для RAID5 | Нет контрольных сумм: молча испорченный блок md не обнаружит | ✅ **Для режима «чётность» (3+ диска): mdadm + btrfs (single data, dup metadata) поверх, как SHR у Synology** |
| **ZFS (OpenZFS 2.4)** | Вне ядра (CDDL). Поддержка новых ядер с задержкой: [2.4.0 — до 6.18, 2.4.4 — до 7.2](https://phoronix.com/news/OpenZFS-2.4.4-Released) | Лучшая целостность, RAIDZ, снапшоты, send/recv | Лицензия несовместима с GPL, поэтому модуль собираем сами (kmod в образе). Fedora обновляет ядро быстрее, чем выходит поддержка в OpenZFS, и это может блокировать наши обновления. MOK для Secure Boot. Высокий порог для пользователя | ⏸️ Не в MVP. Возможно, «ZFS edition» в фазе 3 по образцу uCore (`-zfs` вариант образа) |
| **bcachefs** | [Удалён из mainline в 6.18, теперь DKMS-модуль](https://www.linuxjournal.com/content/bcachefs-ousted-mainline-kernel-move-dkms-and-what-it-means) | Интересные возможности (tiering, erasure coding) | Вне ядра, экспериментальный, будущее неясно | ❌ |
| **mergerfs + SnapRAID** | Пользовательское пространство, зрелые проекты | Диски разного размера, по одному. Каждый диск — отдельная ФС (при потере 2+ дисков теряются только их файлы). Диски, которые не читаются, засыпают (тишина и экономия). Идеально для **медиа-библиотек: «записал один раз, читаю много»** | Чётность не realtime (sync по расписанию), новые файлы до sync не защищены. Не подходит под «живые» данные (БД, фото с телефона) | ✅ **Фаза 2: режим «Медиа-архив (как Unraid)»** |

### 3.2 Мастер создания пула (UX)

Пользователь выбирает **цель**, а не уровень RAID:

```
Создать хранилище
  Выбрано дисков: 4 (2× 8 TB, 2× 4 TB)

  ( ) Максимум места        — без защиты                         24 TB  ⚠ потеря любого диска = потеря данных
  ( ) Зеркало               — копия каждого файла на 2 дисках     12 TB  выдержит отказ 1 диска     [btrfs raid1]
  (•) Баланс (рекомендуем)  — чётность                            12 TB* выдержит отказ 1 диска     [mdadm raid5 + btrfs]
  ( ) Надёжно               — двойная чётность                     8 TB* выдержит отказ 2 дисков    [mdadm raid6 + btrfs]
  ( ) Медиа-архив           — диски разного размера, по одному    16 TB  выдержит отказ 1 диска     [mergerfs + SnapRAID] (фаза 2)
  * для mdadm диски выравниваются по наименьшему — покажем «потеряно 8 TB, лучше выбрать Зеркало»
```

Правила мастера:
- 1 диск → только «Максимум места» (btrfs single, метаданные dup) + предупреждение.
- 2 диска → «Зеркало» по умолчанию.
- 3 диска → «Баланс» (RAID5) или «Зеркало» (btrfs raid1 на трёх дисках тоже работает).
- 4+ диска → «Баланс» по умолчанию, «Надёжно», если дисков ≥ 6 или они ≥ 12 TB (долгий ребилд).
- Разные размеры → считаем ёмкость для каждого варианта и подсвечиваем самый выгодный (btrfs raid1 хорошо работает с разными размерами).
- Расчёт ёмкости в btrfs-случае берём из логики `btrfs filesystem usage` (или калькулятора), не изобретаем свою.
- Перед созданием проверяем SMART, предупреждаем о SMR-дисках (если модель есть в известном списке) и смешении HDD/SSD.
- На выходе получаем пул `/srv/data/<имя>`, сабволюмы `media`, `photos`, `backups`, SMB-шару `Media` и автоматически подключённую к медиасерверу библиотеку.

Реализация: shell-out к `mdadm`, `mkfs.btrfs`, `btrfs` через наш storage-сервис. Альтернатива — UDisks2 D-Bus (умеет mdraid и частично btrfs). Конфиги (`/etc/mdadm.conf`, точки монтирования через systemd `.mount`-юниты или `/etc/fstab`) лежат в `/etc`, а он сохраняется при обновлениях bootc.

### 3.3 Обслуживание

| Задача | Как | Расписание |
|---|---|---|
| SMART | `smartd` (+ `smartctl -j` для UI), короткий тест еженедельно, длинный ежемесячно | systemd timer |
| btrfs scrub | `btrfs scrub start` | ежемесячно, ночью, с ограничением I/O |
| mdadm check | `echo check > /sys/block/mdX/md/sync_action` (Fedora уже ставит `raid-check`) | ежемесячно |
| SnapRAID | `snapraid sync` ежедневно, `snapraid scrub -p 5` еженедельно | timer |
| Балансировка btrfs | только по событию (добавили диск) | вручную из UI |
| Уведомления | Наш `alpha-notify`: веб-UI, push в клиентское приложение, email (SMTP), опционально Telegram/ntfy. Источники: smartd, mdadm `--monitor` (PROGRAM=), журнал btrfs (`btrfs device stats`), заполненность > 85/95% | события |

**Замена диска** (мастер «Диск неисправен»):
1. Показываем, какой диск сломан: модель, серийник, **физический слот** через `ID_PATH` и `ledctl` (индикация, если бэкплейн поддерживает).
2. Пользователь выключает NAS (или делает hot-swap) и меняет диск.
3. Мастер показывает новый пустой диск: «Заменить им WDC …1234?»
4. Дальше `btrfs replace start` или `mdadm --add`, прогресс ребилда в UI, уведомление по окончании.

---

## 4. Обновления (end-to-end)

### 4.1 Архитектура

```
GitHub (repo alpha-os)
  └─ Actions: build Containerfile → rechunk → test (QEMU) → push GHCR → cosign sign
        ghcr.io/<org>/alpha-os:45.20261113.1   (неизменяемый тег версии)
        ghcr.io/<org>/alpha-os:beta            (плавающий)
        ghcr.io/<org>/alpha-os:stable          (плавающий)
  └─ Release service: updates.<domain>/v1/channels/stable.json  (подписанный манифест)
        { "version": "45.20261113.1", "digest": "sha256:…", "rollout": 25,
          "min_from": "45.20261001.0", "notes_url": "…", "requires_reboot": true }

Устройство
  alpha-updated (демон, root, systemd)
    ├─ периодически: GET манифест → проверка подписи → решение (канал, % раскатки, окно обслуживания)
    ├─ bootc upgrade --check / --download-only  (загрузка в фоне, стейджинг)
    ├─ снапшот @var  (btrfs subvolume snapshot -r)
    ├─ bootc upgrade --from-downloaded --apply   (или ждёт кнопку «Перезагрузить сейчас»)
    ├─ после загрузки: greenboot-rs health checks → OK: «boot success» / FAIL: автооткат
    └─ API (unix socket / локальный HTTP за auth-прокси админки):
         GET  /api/system/update        → текущая версия, доступная, статус загрузки, release notes
         POST /api/system/update/check
         POST /api/system/update/apply  {when: "now"|"maintenance"}
         POST /api/system/rollback
         PUT  /api/system/update/settings {channel, auto: off|download|install, window: "03:00-05:00"}
  Веб-админка и клиентское приложение (через API медиасервера / облачный relay) → тот же API
```

### 4.2 Ключевые решения

1. **Выключаем стандартный `bootc-fetch-apply-updates.timer`**: он сам ставит обновление и перезагружает машину. Вместо него работает наш `alpha-updated`. Используем штатные операции bootc: `--check`, `--download-only`, `--from-downloaded`, `--apply`, `--tag` ([man bootc-upgrade](https://bootc.dev/bootc/man/bootc-upgrade.8.html)), а также `bootc rollback` и `bootc status --format=json`.

2. **Каналы = теги в реестре.** Переключение канала: `bootc switch ghcr.io/<org>/alpha-os:beta` (или `bootc upgrade --tag beta`). Каналы: `stable`, `beta`. Возможны `nightly` для внутренних устройств и в будущем `lts` на CentOS Stream.

3. **Поэтапная раскатка.** Плавающий тег `stable` двигаем **только после** того, как раскатка дошла до 100%. До этого устройство из когорты (hash(device_id) mod 100 < rollout) переходит на **конкретную версию**: неизменяемый тег или digest из манифеста. Затем возвращается на отслеживание `stable`. Перед продакшеном нужно проверить в прототипе, что `bootc switch` по digest и обратно сохраняет нормальное отслеживание. Манифест также служит «стоп-краном»: `"halt": true` останавливает раскатку на всех устройствах, не трогая реестр.

4. **Подписи.** cosign **с ключом** (не keyless): ключ хранится в GitHub secret, а лучше в KMS. На устройстве `/etc/containers/policy.json` (поставляется в образе через `/usr/etc`, затем в `/etc`) задаёт `default: reject`, а для `ghcr.io/<org>/alpha-os` — `sigstoreSigned` с нашим публичным ключом. Плюс `registries.d/*.yaml` с `use-sigstore-attachments: true`. Keyless-проверка [в containers/image пока не умеет сверять identity](https://gitlab.com/dunn.dev/immutable/basef/-/issues/15), поэтому только ключ. Типичная ошибка «signature required but none exists» у тех, кто включил политику раньше, чем начал подписывать ([пример](https://universal-blue.discourse.group/t/failed-to-invoke-method-openimage-a-signature-was-required-but-no-signature-exists/7335)), закрывается тестом в CI. Манифест раскатки подписываем отдельно (minisign или тот же cosign `sign-blob`).

5. **Health checks и автооткат: greenboot-rs.** [Rust-переписанный greenboot для bootc и rpm-ostree](https://spinics.net/lists/fedora-devel/msg329177.html):
   - `required.d` (провал = откат после N попыток): сеть поднялась, `alpha-admin` отвечает на `/healthz`, медиасервер отвечает на `/health`, `smbd` активен, root-ФС смонтирована rw, `/var` в порядке;
   - `wanted.d` (только лог и уведомление): пулы данных смонтированы, GPU виден (`/dev/dri/renderD128`, `nvidia-smi`). **Отказ диска с данными не должен откатывать ОС**;
   - `red.d`: собрать диагностику (journal, `bootc status`) в `/var/lib/alpha/failed-updates/` и после отката показать пользователю «Обновление X не прошло проверку и было отменено» с кнопкой «Отправить отчёт».

6. **Состояние приложения (`/var`).** bootc не откатывает `/var`. Отсюда правила:
   - миграции БД медиасервера **обратно совместимы на одну версию** (expand → contract);
   - перед apply агент делает read-only снапшот `@var`. Если greenboot откатил ОС, агент при старте старого деплоймента видит маркер и предлагает (или делает автоматически) откат `@var` к снапшоту;
   - храним 3 последних снапшота.

7. **Размер обновлений.** Образ «рекчанкуем» (`rpm-ostree compose build-chunked-oci` или `bootc-base-imagectl rechunk`): слои стабильны, и при обновлении качаются только изменённые чанки. Обычно это десятки–сотни МБ, а не весь образ. Появился `--from-delta` (oci-delta артефакт) — пригодится для офлайн-обновления с USB.

8. **Перезагрузка.** Обновление ОС = перезагрузка (около 1–2 минут на NAS). Если в обновлении меняется только userspace, можно попробовать `--soft-reboot=auto`. По умолчанию: «скачать автоматически, установить в окно обслуживания 03:00–05:00, если никто не смотрит видео» (агент спрашивает медиасервер об активных сессиях). Пользователь может выбрать «только уведомлять».

9. **Major-апгрейды Fedora (45 → 46)** для устройства выглядят как обычное обновление: новый образ. Мы гоняем их в beta минимум 2–4 недели.

### 4.3 Ядро и драйверы для транскодирования

| GPU | Что нужно | Где берём | Влияние на обновления |
|---|---|---|---|
| **Intel iGPU / Arc (QSV, VAAPI)** | i915/xe в ядре; `intel-media-driver` (iHD); oneVPL runtime (`libvpl`, `intel-vpl-gpu-rt`); firmware (`linux-firmware`) | Ядро и firmware из Fedora. [`intel-media-driver` с полным набором кодеков — RPM Fusion nonfree](https://fedoraproject.org/wiki/Hardware_Video_Acceleration) | Нет внедеревных модулей. Обновляется вместе с образом. **Лучший путь, рекомендуем его в маркетинге** |
| **AMD (VAAPI)** | amdgpu в ядре; Mesa. Для H.264/HEVC нужен `mesa-va-drivers-freeworld` (RPM Fusion), в Fedora эти кодеки вырезаны | RPM Fusion free | Без модулей, но Mesa freeworld должна совпадать с версией Mesa Fedora — проверяем в CI |
| **NVIDIA (NVENC/NVDEC)** | Открытые модули ядра NVIDIA + проприетарный userspace (`libnvidia-encode`, `libcuda`). Модуль собирается **под каждое ядро** | Собираем kmod в CI (akmods в build-стадии Containerfile, как в uBlue), подписываем своим ключом. Fedora [не может поставлять внедеревные модули](https://project-resistor.codeberg.page/features/ai-developer-desktop-cuda/nvidia-drivers/), а [в официальных bootc-образах Secure Boot с NVIDIA ломается](https://discussion.fedoraproject.org/t/enabling-gpus-on-fedora-with-image-mode-fedora-bootc/170247) | **Отдельный вариант образа** `alpha-os-nvidia` (тот же Containerfile + стадия). Если kmod не собрался под новое ядро, CI не публикует образ, и устройства с NVIDIA просто остаются на прошлой версии. Устройство выбирает вариант при установке (детект по `lspci`) и может переключиться через `bootc switch` |

Медиасервер получает доступ к GPU так: нативный сервис в группах `render`/`video`, либо контейнер с `--device /dev/dri` или NVIDIA CDI (`nvidia-ctk cdi generate` → `--device nvidia.com/gpu=all`).

---

## 5. Веб-админка и сервисы

### 5.1 Своя админка vs Cockpit

| | Свой UI | Cockpit + плагины |
|---|---|---|
| UX «для людей», бренд, единый стиль с клиентским приложением | ✅ | ❌ (PatternFly, «админский» вид) |
| Единый API для веба и клиентских приложений | ✅ | ❌ (Cockpit — это канал к D-Bus и shell через websocket, а не REST API) |
| Скорость старта | ❌ больше работы | ✅ хранилище, сеть, сервисы, терминал, журнал готовы |
| Поддержка bootc | Своя | `cockpit-ostree` ориентирован на rpm-ostree, с bootc поддержка частичная |

**Рекомендация:** **свой UI + свой бэкенд `alpha-admin` (Go)**. Он работает поверх системных D-Bus API:
- systemd (сервисы, таймеры, journal);
- NetworkManager (сеть, Wi-Fi, статика, VLAN/бонды позже);
- firewalld (зоны, сервисы);
- UDisks2 + наш storage-сервис (пулы, SMART);
- logind (перезагрузка, выключение);
- `alpha-updated` (обновления);
- Samba (генерация конфига).

Один REST/JSON (или gRPC-Web) API обслуживает и веб, и наши клиентские приложения (вкладка «Сервер» в приложении). **Cockpit** ставим в образ, но **выключенным** (`cockpit.socket` disabled). В UI есть пункт «Расширенный режим (Cockpit)», который включает его на :9090 только для LAN. Это даёт терминал и журнал «для гиков» без нашей работы.

### 5.2 Сервисы в образе

| Сервис | Реализация | Примечания |
|---|---|---|
| **Наш медиасервер** | **Нативно в образе** (`/usr/lib/alpha-media`, systemd unit, состояние в `/var/lib/alpha-media`) | Релизы ОС и медиасервера связаны и тестируются вместе, а при откате ОС откатывается и сервер. Проще доступ к GPU. Хотфиксы медиасервера = новый образ, а с rechunk это маленькая дельта. Альтернатива: Podman Quadlet + [logically bound images](https://bootc.dev) bootc. Её стоит выбрать, если медиасервер будет выпускаться чаще ОС или поставляться и на другие ОС как контейнер |
| **Samba (SMB3)** | Пакет Fedora. Конфиг генерируем в `/etc/samba/smb.conf.d/` (или `net conf` registry) | Шары: `Media` (чтение для гостей опционально), `Users/<name>`. Пользователи Samba синхронизируются с пользователями админки (`pdbedit`). `vfs_fruit` для macOS и Time Machine, `wsdd` для обнаружения в Windows, Avahi (mDNS) для `alpha.local` |
| **Пользователи** | Системные пользователи (группа `alpha-users`) + роли в БД админки | Админ = `wheel`. SSH по умолчанию выключен, включается в UI (только ключи) |
| **Firewall** | firewalld, зона `home` | Открыты: 80/443 (админка), 445 (SMB), порт медиасервера, mDNS/WSD. Всё остальное по явному включению |
| **TLS** | Самоподписанный сертификат + опционально Let's Encrypt через наш DDNS-домен (`<id>.alpha-cloud.example`) или Tailscale cert | |
| **Удалённый доступ** | Фаза 1: интеграция Tailscale (пакет в образе, вход из UI). Фаза 2: собственный relay/туннель (WireGuard) — это область другого исследователя | Не открывать порты на роутере по умолчанию (UPnP выключен) |
| **Опциональные приложения** | Podman Quadlet (`/etc/containers/systemd/*.container`), «магазин» из курированного списка (qBittorrent, Immich…) | Фаза 3. Данные в `/var/lib/alpha-apps` |
| **Уведомления, метрики** | `alpha-notify`; метрики — node_exporter-подобный эндпоинт или встроенные графики | |
| **Время** | chrony | |

---

## 6. Пайплайн сборки

### 6.1 Структура репозитория `alpha-os`

```
alpha-os/
  Containerfile            # FROM quay.io/fedora/fedora-bootc:45@sha256:…  (пин по digest)
  Containerfile.nvidia     # FROM ghcr.io/<org>/alpha-os:<ver> + akmods nvidia stage
  installer/Containerfile  # live-окружение установщика (FROM тот же base + alpha-installer + dracut dmsquash-live)
  installer/iso.yaml       # /usr/lib/image-builder/bootc/iso.yaml: метка, kargs, grub-меню
  rootfs/                  # файлы, копируемые в образ (/usr/lib/systemd, /usr/share/alpha, policy.json …)
  packages.txt + lockfile  # список пакетов; версии фиксируются lock-файлом
  tests/                   # QEMU-тесты (pytest + paramiko/pexpect), greenboot-checks
  .github/workflows/
```

Скелет `Containerfile`:

```dockerfile
FROM quay.io/fedora/fedora-bootc:45@sha256:<pinned>
COPY rootfs/ /
RUN dnf -y install samba samba-usershares wsdd avahi smartmontools mdadm btrfs-progs \
      firewalld NetworkManager-wifi tailscale cockpit-system greenboot greenboot-default-health-checks \
      https://mirrors.rpmfusion.org/free/fedora/rpmfusion-free-release-45.noarch.rpm ... \
 && dnf -y install intel-media-driver libva-utils mesa-va-drivers-freeworld \
 && dnf -y swap fedora-logos generic-logos \
 && rpm -ivh /tmp/alpha-media-*.rpm /tmp/alpha-admin-*.rpm /tmp/alpha-updated-*.rpm \
 && systemctl enable alpha-admin alpha-media alpha-updated smb wsdd avahi-daemon firewalld smartd greenboot-healthcheck \
 && systemctl disable bootc-fetch-apply-updates.timer cockpit.socket \
 && dnf clean all && bootc container lint
```

### 6.2 Workflow в GitHub Actions

1. **build** (ubuntu-24.04 или ubuntu-24.04-arm для aarch64 в будущем, rootful podman/buildah):
   - освобождаем место на раннере (на стандартном раннере около 14 GB свободно, сборка ISO требует 20–30 GB). Используем action вроде `jlumbroso/free-disk-space` или larger runner;
   - `podman build` → `rechunk` → `bootc container lint`;
   - `podman push ghcr.io/<org>/alpha-os:<ver>` (+ `:nvidia-<ver>`);
   - `cosign sign --key env://COSIGN_KEY ghcr.io/...@<digest>`, SBOM (`syft`) и attest.
2. **iso**: `sudo image-builder build --bootc-ref localhost/alpha-installer --bootc-installer-payload-ref ghcr.io/<org>/alpha-os@<digest> generic-iso` ([docs](https://osbuild.org/docs/developer-guide/projects/image-builder/advanced/bootc/isos/)). Учитываем известную проблему с `systemd-remount-fs.service` при установке через Anaconda, которая нас не касается, если ставим своим установщиком. Выход: `alpha-os-<ver>-x86_64.iso` + `.sha256` + подпись.
3. **test** (KVM доступен на стандартных Linux-раннерах GitHub):
   - QEMU + OVMF (Secure Boot vars с ключами Microsoft) + swtpm;
   - сценарий 1: установка с ISO в неинтерактивном режиме (`alpha-installer --answers answers.json`, тот же бэкенд, что и TUI) на виртуальный NVMe + 3 виртуальных HDD → загрузка → проверки по SSH (`bootc status`, health-эндпоинты, SMB через `smbclient`, создание пула RAID5 через API);
   - сценарий 2: обновление N-1 → N, перезагрузка, greenboot OK;
   - сценарий 3: «плохое» обновление (намеренно сломанный health check) → автооткат на N-1;
   - сценарий 4: проверка подписи (неподписанный образ должен отклоняться);
   - на этапе бета: ночные прогоны на реальном железе (self-hosted раннер с 2–3 машинами: Intel N100, AMD, NVIDIA).
4. **promote** (ручной `workflow_dispatch` с approval): двигаем `beta`/`stable`, обновляем манифест раскатки.
5. **publish ISO**: см. ниже.

**Воспроизводимость.** Базовый образ пиним по digest, RPM-пакеты фиксируем lock-файлом. Используем `SOURCE_DATE_EPOCH`, наши бинарники собираются детерминированно (Go: `-trimpath`, `-buildid=`). Цель — «пересобираемость», а не побитовая идентичность: для ostree/ISO она недостижима без больших усилий и нам не нужна.

### 6.3 Размер ISO и хостинг

- Ориентир: базовый fedora-bootc около 1.5–2 GB в несжатом виде. Наши сервисы, Samba, firmware и драйверы Intel/AMD добавляют своё. Live-окружение установщика делит те же слои. **ISO получится примерно 1.6–2.5 GB.** Вариант с NVIDIA больше на ~0.5–1 GB.
- **Ограничение GitHub Releases: каждый файл < 2 GiB** (ровно 2 GiB [может быть отклонён](https://github.com/orgs/community/discussions/61733)), при этом [общий размер релиза и трафик не ограничены](https://docs.github.com/en/repositories/releasing-projects-on-github/about-releases).
- Стратегия:
  1. Держим основной ISO **< 1.9 GiB**: используем `fedora-bootc` minimal, исключаем ненужную прошивку (`linux-firmware` разбит на подпакеты, ставим нужные: intel, amd, realtek, mediatek wifi), убираем документацию и локали, сжимаем squashfs xz/zstd.
  2. Вариант с NVIDIA **не кладём в ISO**. При установке выбираем «NVIDIA» и тянем `alpha-os-nvidia` из сети, либо обычный ISO + `bootc switch` после первой загрузки.
  3. **Основной хостинг ISO — Cloudflare R2** (бесплатный исходящий трафик) за `download.<domain>`, плюс GitHub Release с `.sha256`, `.sig` и ссылкой. Резервные зеркала: SourceForge (бесплатно для open source) или Backblaze B2 + CDN. Если ISO неизбежно > 2 GiB, на GitHub выкладываем `split`-части или только ссылку.
  4. Опционально `.torrent` и сертификат подписи на сайте.

---

## 7. Юридические вопросы

| Вопрос | Суть | Что делаем |
|---|---|---|
| **Товарный знак Fedora** | [Fedora Trademark Guidelines](https://docs.fedoraproject.org/ga/legal/trademarks/): модифицированные сборки нельзя называть «Fedora» и нельзя использовать Fedora в названии продукта или компании. Для смешанных сборок есть знак «Fedora Remix», но и он не может быть частью нашего бренда | Своё имя (например, «Alpha OS»). `dnf swap fedora-logos generic-logos`, свой `/usr/lib/os-release` (`NAME="Alpha OS"`, `ID=alpha-os`, `ID_LIKE=fedora`, `VERSION_ID=45.2026…`). Fedora-бренд убираем из grub, plymouth и motd. В документации пишем «основано на Fedora Linux» — фактическое указание, это допустимо. Перед релизом юрист проверяет текст |
| **Лицензии ОС** | GPL и другие: обязанность предоставить исходники для бинарников, которые мы распространяем | Пакеты Fedora — ссылаемся на SRPM Fedora (точные NEVRA в SBOM). Наши изменения публикуем в открытом репо `alpha-os`. Для пропатченных пакетов — свой SRPM-репозиторий |
| **Firmware** | `linux-firmware` распространяется на условиях redistributable-лицензий, Fedora его уже поставляет | Ок, просто включаем нужные подпакеты |
| **Кодеки и патенты (H.264, HEVC, AAC…)** | Fedora поставляет `ffmpeg-free` без патентованных кодеков, а в Mesa вырезаны VAAPI H.264/HEVC. Полные версии лежат в [RPM Fusion](https://rpmfusion.net/Howto/Multimedia) (free = свободная лицензия, но патентный риск; nonfree = несвободная лицензия) | **Главный юридический риск продукта.** Медиасервер без HEVC/H.264 бесполезен. Варианты: (а) собственный статически собранный `ffmpeg` внутри медиасервера (как `jellyfin-ffmpeg` у Jellyfin) — риск в юрисдикциях с софт-патентами (США), в большинстве стран ЕС ниже; (б) лицензирование через пулы (Access Advance / Via LA), как делают коммерческие продукты (Plex, Synology), — платно и с отчётностью; (в) сначала дистрибуция из юрисдикции с низким риском. **Решение принимает владелец с юристом.** Технически код готов к любому варианту: ffmpeg живёт внутри пакета медиасервера, а не в ОС |
| **RPM Fusion в образе** | Перераспространение пакетов RPM Fusion в своём образе юридически означает, что мы сами их распространяем | Перечень пакетов RPM Fusion в образе минимален и задокументирован (intel-media-driver, mesa-va-drivers-freeworld). Юрист проверяет лицензии |
| **NVIDIA** | Открытые модули ядра — MIT/GPL. Userspace (`libcuda`, `libnvidia-encode`) — проприетарная лицензия NVIDIA с условиями перераспространения. Сообщества вроде uBlue поставляют их в образах | Отдельный вариант образа и отдельная галочка в установщике с текстом лицензии NVIDIA (принятие EULA пользователем). Юридическая проверка лицензии перед публикацией |
| **ZFS (если будет)** | CDDL несовместима с GPL: распространять бинарный модуль вместе с ядром — серая зона (Canonical так делает, большинство — нет) | Если дойдём до этого, то только отдельный вариант образа и после консультации |
| **Secure Boot** | Не подписываем свой shim, поэтому договоры с Microsoft не нужны. Ключ MOK для kmods — наш, его безопасность на нас | Ключ подписи kmods хранится в KMS/HSM, ротация описана в runbook |

---

## 8. Поэтапный план

### Фаза 0 — Прототип (2–3 недели)
- [ ] Репо `alpha-os`: `Containerfile` на `fedora-bootc:45`, Samba, Avahi, wsdd, firewalld, smartd, заглушки наших сервисов.
- [ ] Сборка в GitHub Actions → push в GHCR → `bootc switch` на тестовой VM, проверка `bootc upgrade` и `bootc rollback`.
- [ ] Быстрый ISO для демонстрации (допустимо через legacy `anaconda-iso` или `bootc install to-disk` вручную).
- [ ] Проверить на железе: Intel N100/N150 (QSV), AMD APU, `vainfo`, транскодирование тестовым ffmpeg.
- [ ] **Решения на выходе:** язык установщика и бэкенда (Go?), нативный медиасервер или Quadlet, юридическая позиция по кодекам (запустить параллельно).

### Фаза 1 — MVP ISO (2–3 месяца)
- [ ] `alpha-installer` (TUI): экран сети и пользователя, экран выбора диска (lsblk + udev, человеческие имена), офлайн-установка из вшитого образа, `--target-imgref` на GHCR `stable`.
- [ ] Live-ISO через `image-builder generic-iso`, размер < 1.9 GiB, публикация в R2 + GitHub Release.
- [ ] Брендинг: os-release, generic-logos, grub и plymouth.
- [ ] `alpha-admin` (минимум): вход, дашборд (CPU/RAM/температуры/диски), **мастер пула** (single / btrfs RAID1 / mdadm RAID5/6 + btrfs), SMB-шары, пользователи, сеть.
- [ ] `alpha-updated`: check, download, apply, rollback через API. Каналы stable/beta (теги). cosign + policy.json. greenboot-rs health checks. Снапшот `@var` перед обновлением.
- [ ] Медиасервер предустановлен, Intel и AMD VAAPI работают из коробки.
- [ ] Уведомления: SMART, деградация RAID, заполненность диска (в веб-UI + email).
- [ ] CI: QEMU-тесты установки, обновления, отката и проверки подписи.

### Фаза 2 — Публичная бета (+3 месяца)
- [ ] Веб-установщик (HTTP + PIN) для headless.
- [ ] Поэтапная раскатка (подписанный манифест, когорты, стоп-кран), окно обслуживания с учётом активных стримов.
- [ ] Вариант образа **NVIDIA** (kmod в CI, подпись, MOK-флоу в установщике и UI).
- [ ] Режим «Медиа-архив» (mergerfs + SnapRAID), мастер замены диска с индикацией слота, расширение пулов.
- [ ] LUKS2 + TPM2 (опционально) для системного диска и пулов.
- [ ] Удалённый доступ (Tailscale, затем собственный relay), Let's Encrypt.
- [ ] Управление обновлениями из клиентского приложения.
- [ ] Self-hosted раннер с реальным железом для ночных тестов.
- [ ] Первый переход на major-версию (Fedora 45 → 46) по всей цепочке beta → stable.

### Фаза 3 — Полная версия (+6 месяцев)
- [ ] Каталог приложений (Podman Quadlet), бэкапы (на USB-диск, в облако S3/B2, на другой Alpha-NAS), снапшоты пулов по расписанию с восстановлением из UI («Предыдущие версии» в Windows через `vfs_shadow_copy2`).
- [ ] Вариант ZFS (если будет спрос), канал `lts` на CentOS Stream 10 bootc (если будет спрос от «консервативных» пользователей).
- [ ] aarch64 (Raspberry Pi 5 / RK3588), если это нужно продукту.
- [ ] Офлайн-обновление с USB (`--from-delta` / oci-archive).
- [ ] Телеметрия по opt-in (успешность обновлений, откаты) для управления раскаткой.

---

## 9. Риски и открытые вопросы

| Риск | Вероятность | Митигирование |
|---|---|---|
| bootc и инструменты Fedora image mode продолжают меняться (уже было: bootc-image-builder → image-builder, `anaconda-iso` объявлен устаревшим) | Высокая | Минимум зависимостей от «обвязки»: свой установщик, ISO через базовый `generic-iso`. Следить за [Fedora Image Mode initiative](https://pagure.io/Fedora-Council/tickets/issue/549) |
| Короткий жизненный цикл Fedora (13 мес.), ежегодно 2 major-перехода | Средняя | Автоматизированные тесты. Запасной путь на CentOS Stream 10 bootc |
| Патентные претензии по кодекам | Средняя (зависит от рынка) | Юрист до публичного релиза; ffmpeg изолирован в пакете медиасервера |
| ISO > 2 GiB | Средняя | Минимальный образ, R2 как основной хостинг |
| NVIDIA kmod не собирается под новое ядро | Высокая (регулярно) | Отдельный вариант образа; CI-гейт оставляет NVIDIA-устройства на прошлой версии |
| `/var` не откатывается вместе с ОС | Определённая | Снапшоты `@var`, совместимые миграции |
| Пользователь путает системный диск и диски с данными | Средняя | Предупреждения в установщике (найдены данные/RAID), ввод имени диска для подтверждения |

---

## Источники

- Fedora Image Mode initiative (цель production-ready к F45): https://pagure.io/Fedora-Council/tickets/issue/549
- bootc-image-builder deprecation → image-builder: https://osbuild.org/docs/bootc/deprecation-notice/
- image-builder: ISO для bootc (generic-iso, payload ref): https://osbuild.org/docs/developer-guide/projects/image-builder/advanced/bootc/isos/
- man bootc-upgrade: https://bootc.dev/bootc/man/bootc-upgrade.8.html
- Anaconda `bootc` kickstart-команда: https://fedoramagazine.org/introducing-the-new-bootc-kickstart-command-in-anaconda/
- Офлайн-установка ostreecontainer и привязка к локальному пути: https://github-redirect.dependabot.com/rhinstaller/anaconda/discussions/5197
- Anaconda WebUI roadmap: https://communityblog.fedoraproject.org/?p=14910
- Fedora 45 — перенос релиза: https://phoronix.com/news/Fedora-45-Second-NO-GO
- Fedora 43 EOL: https://endoflife.ai/article-fedora-43-eol
- greenboot-rs (Fedora change): https://spinics.net/lists/fedora-devel/msg329177.html
- greenboot: автоматические откаты: https://developers.redhat.com/index%2Ephp/articles/2024/08/12/greenboot-automate-rollbacks-atomically-updated-systems
- Подпись образов и keyless-ограничения: https://gitlab.com/dunn.dev/immutable/basef/-/issues/15 , https://universal-blue.discourse.group/t/failed-to-invoke-method-openimage-a-signature-was-required-but-no-signature-exists/7335
- uCore (Universal Blue, ZFS/NVIDIA варианты): https://github.com/ublue-os/ucore , https://universal-blue.discourse.group/t/ucore-zfs-kmods-are-now-signed-for-secureboot/383
- btrfs RAID6 патчи 2026: https://lkml.iu.edu/hypermail/linux/kernel/2605.1/06764.html
- bcachefs вне mainline: https://www.linuxjournal.com/content/bcachefs-ousted-mainline-kernel-move-dkms-and-what-it-means
- OpenZFS 2.4.4 (Linux до 7.2): https://phoronix.com/news/OpenZFS-2.4.4-Released
- TrueNAS 2026 roadmap: https://linuxiac.com/?p=206861
- HexOS 1.0: https://docs.hexos.com/blog/2026-04-02 ; критика: https://nascompares.com/news/is-hexos-in-trouble-mid-2026-update/
- ZimaOS 1.6: https://shop.zimaspace.com/sv/pages/zimaos-1-6-0-features-fixes-upgrade-issues
- Fedora Trademark Guidelines: https://docs.fedoraproject.org/ga/legal/trademarks/
- Fedora Hardware Video Acceleration: https://fedoraproject.org/wiki/Hardware_Video_Acceleration
- RPM Fusion Multimedia: https://rpmfusion.net/Howto/Multimedia
- NVIDIA на Fedora bootc: https://discussion.fedoraproject.org/t/enabling-gpus-on-fedora-with-image-mode-fedora-bootc/170247 ; https://project-resistor.codeberg.page/features/ai-developer-desktop-cuda/nvidia-drivers/
- Microsoft UEFI signing requirements / CA 2023: https://techcommunity.microsoft.com/blog/hardwaredevcenter/updated-uefi-signing-requirements/1062916 , https://techcommunity.microsoft.com/t5/hardware-dev-center/signing-with-the-new-2023-microsoft-uefi-certificates-what/ba-p/4455787
- GitHub Releases limits: https://docs.github.com/en/repositories/releasing-projects-on-github/about-releases , https://github.com/orgs/community/discussions/61733
