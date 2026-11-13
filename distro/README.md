# distro/ — NAS-ОС project_alpha (прототип, фаза 0)

Неизменяемая (image-based) ОС для NAS и медиасервера на базе bootc. Вся ОС описана одним `Containerfile` и поставляется как OCI-образ; обновления атомарные, с откатом (`bootc upgrade` / `bootc rollback`). Решения фазы 0 — в [ADR-0010](../docs/adr/0010-nas-os-phase0.md), исследование — [05](../docs/research/05-nas-linux-distro.md).

> Основано на Fedora Linux. Имя ОС ещё не выбрано (DEC-004); рабочее имя образа — `alpha-os`.

## Структура

| Путь | Что |
|---|---|
| `Containerfile` | образ ОС: `FROM fedora-bootc:45@sha256:…` (пин по digest), пакеты, сервисы, firewall, `bootc container lint` |
| `packages.txt` | пакеты поверх базы (Samba, wsdd, Avahi, firewalld, smartmontools, mdadm, btrfs-progs, greenboot, Cockpit) |
| `rootfs/` | файлы, копируемые в образ: юниты-заглушки наших сервисов, preset, `tmpfiles.d`, `smb.conf` |
| `scripts/build-image.sh` | сборка образа (`podman`, если есть, иначе `docker`) |
| `scripts/make-disk.sh` | загрузочный raw-диск через `bootc install to-disk --via-loopback` в привилегированном контейнере |
| `scripts/ci-build.sh`, `ci-sbom.sh`, `sign-image.sh` | сборка в CI (build → rechunk → lint), SBOM (syft), подпись cosign |
| `keys/` | публичный ключ cosign `alpha-os.pub` (кладёт владелец, см. ниже) |
| `tests/signature-policy.sh` | DIST-002: устройство отклоняет неподписанный образ |
| `scripts/run-vm.sh` | запуск диска в QEMU/KVM (2 vCPU, 3 GiB, SSH на `127.0.0.1:22522`, `VM_UEFI=1` для OVMF) |
| `installer/` | `alpha-installer` (Go): пока только обнаружение дисков (DIST-004), см. [installer/README.md](installer/README.md) |
| `tests/vm-switch-rollback.sh` | сценарий фазы 0: switch → rollback → upgrade → rollback → digest-пин и возврат на тег |
| `.work/` | локальные артефакты (диски, ключи SSH, логи); в git не попадает |

## Что в образе

- **Наши сервисы — заглушки** (`/usr/libexec/alpha/alpha-stub`, Python): `alpha-media` (:8096), `alpha-admin` (:80), `alpha-updated` (:7091, наружу не открыт). Каждая отвечает `GET /health` и `/healthz` JSON-ом с версией образа. Юниты с `DynamicUser=yes` и `ProtectSystem=strict`.
- **Samba**: SMB3, `vfs_fruit` для macOS, шара `Media` → `/var/srv/media` (гостевое чтение). **wsdd** (Windows), **Avahi** + nss-mdns (`<hostname>.local`).
- **firewalld**: зона по умолчанию `home`; открыты samba, mdns, ws-discovery, http, https, 8096/tcp. (ssh и dhcpv6-client остаются из стандартной зоны `home` — на прототипе SSH нужен для тестов; выключение по умолчанию — в DIST-013.)
- **smartd**, **chronyd**, **greenboot** (проверки появятся в DIST-011), **Cockpit** установлен, `cockpit.socket` выключен.
- `bootc-fetch-apply-updates.timer` **замаскирован**: обновлениями будет управлять `alpha-updated` (DIST-009).
- Версия образа — `/usr/share/alpha/version` (`ALPHA_OS_VERSION`, build-arg).

## Сборка

```bash
distro/scripts/build-image.sh localhost/alpha-os:dev            # или:
docker build -f distro/Containerfile -t localhost/alpha-os:dev distro
```

`bootc container lint` выполняется в конце сборки. Остаётся одно предупреждение из базового образа (`/var/lib/authselect/checksum`), на работу не влияет.

## Проверка на VM

Требования: docker (пользователь в группе `docker`) **или** rootful podman, `qemu-system-x86_64`, доступ к `/dev/kvm`, `ssh`, `python3`, ~6 GB на диске. Sudo на хосте не нужен.

```bash
distro/tests/vm-switch-rollback.sh          # ~10 минут; KEEP=1 — оставить VM и реестр
```

Сценарий поднимает локальный реестр `registry:2` на `127.0.0.1:5000` (VM видит хост как `10.0.2.2`), собирает v1 и v2, ставит **стоковую** базу на raw-диск, загружает её в QEMU и по SSH проверяет:

1. `bootc switch 10.0.2.2:5000/alpha-os:dev` → загружен v1, сервисы живы, `/health` отвечает, smbd слушает 445, зона firewalld `home`, таймер замаскирован;
2. `bootc rollback` → снова стоковая база;
3. `bootc rollback` → снова v1;
4. в реестр под тем же тегом публикуется v2, `bootc upgrade --check` и `bootc upgrade` → загружен v2;
5. `bootc rollback` → v1;
6. `bootc switch` на v2 **по digest**, затем обратно на тег `:dev`; `bootc upgrade --check` снова отслеживает тег (нужно для поэтапной раскатки, DIST-012).

### Результат (2026-11-13)

Сценарий пройден целиком (`ALL STEPS PASSED`) на хосте Ubuntu с docker 29 и QEMU 10.2 (KVM, SeaBIOS, 2 vCPU, 3 GiB): базовый образ bootc 1.17.1, SELinux в VM — `Enforcing`. Все 6 шагов, включая переход на digest и возврат на тег. `smartd` в VM неактивен — у virtio-диска нет SMART, на железе это нормальная работа; в проверку сервисов он не входит.

### Почему именно так

- **Диск** пишется `bootc install to-disk --generic-image --via-loopback` из контейнера базового образа с `--privileged --pid=host -v /dev:/dev`. Нужны loop-устройства, `mkfs` и монтирование, поэтому **rootless podman не подходит**; docker из группы `docker` — подходит.
- **Источник** для установки — `docker://localhost:5000/…`, а не `docker-archive:`: `docker save` (containerd image store) отдаёт слои, которые ostree не принимает (`invalid gzip header`).
- **Реестр без TLS** разрешается файлом в `/etc/containers/registries.conf.d/` — и в контейнере-установщике, и в VM. Это только для тестов.
- **image-builder / bootc-image-builder** пока не используем: им нужен образ в containers-storage хоста (то есть podman), а на машине разработчика может быть только docker. Для ISO (DIST-007) — `image-builder … generic-iso` в CI на runner с podman.

## CI и подпись образов (DIST-002)

Workflow `.github/workflows/distro.yml`:

- **PR** (в том числе из форков) — только сборка, токен read-only, без секретов: `podman build` → `bootc-base-imagectl rechunk` (стабильные слои, обновление качает только изменённое) → `bootc container lint` → SBOM (syft, SPDX JSON, артефакт workflow) → `tests/signature-policy.sh`.
- **push в `main`** — то же, плюс push `ghcr.io/denishumen/alpha-os:<ver>` (`<ver>` = `45.YYYYMMDD.<run>`), подпись cosign по digest, SBOM как attestation, затем `:nightly` переносится на тот же digest. **Тег `os-vX.Y.Z`** — публикует `:X.Y.Z`.
- **promote** (ручной запуск, окружение `release`): проверяет подпись версии публичным ключом и переносит `:beta` или `:stable` на её digest.

Подпись — cosign v2 **с ключом** (не keyless, 05 §4.2 п.4): подпись лежит рядом с образом как `<repo>:sha256-<digest>.sig`, этот формат проверяет containers/image, а значит и bootc.

### Политика на устройстве

`/usr/libexec/alpha/alpha-signature-policy` ставит при сборке:

- `/etc/containers/policy.json`: `default: reject`; для `ghcr.io/denishumen/alpha-os` — `sigstoreSigned` с ключом `/usr/share/alpha/keys/alpha-os.pub` и `matchRepository` (подходит и для тегов-каналов, и для digest); локальное `containers-storage` разрешено;
- `/etc/containers/registries.d/alpha-os.yaml`: `use-sigstore-attachments: true`.

Режим задаёт build-arg `ALPHA_SIGNATURE_POLICY`: `auto` (по умолчанию: включить, если в `distro/keys/` есть `alpha-os.pub`), `enforce` (без ключа сборка падает), `off` (стоковая политика; так собирает образы VM-тест с локальным реестром). Режим записан в `/usr/share/alpha/version`.

Пока ключ не закоммичен, CI публикует образ **без** политики и без подписи (с предупреждением) — так нельзя «закирпичить» устройства политикой раньше, чем появятся подписи. Если ключ закоммичен, а секрета нет, публикация падает.

### Тест «неподписанный образ отклоняется»

`distro/tests/signature-policy.sh IMAGE` (в CI на каждом PR; локально нужен `cosign` v2): генерирует одноразовую пару ключей, кладёт в локальный реестр крошечные образы и **внутри собранного образа ОС** (его skopeo и containers/image — тот же стек, которым тянет обновления bootc) ставит политику с тестовым ключом. Ожидается: подписанный — принят; неподписанный, подписанный чужим ключом и образ из другого репозитория — отклонены. Если образ собран с `enforce`, дополнительно проверяется сама поставляемая политика.

### Шаги владельца (один раз)

Агенты ключи не создают и секреты не заводят.

1. Локально, на доверенной машине: `cosign generate-key-pair` (cosign v2), надёжный пароль. Появятся `cosign.key` и `cosign.pub`. `cosign.key` в репозиторий **не** класть; хранить офлайн-копию.
2. Settings → Secrets and variables → Actions: секрет `COSIGN_PRIVATE_KEY` = содержимое `cosign.key`, `COSIGN_PASSWORD` = пароль.
3. Settings → Environments: создать `release` с обязательным ревьюером (владелец) — для promote.
4. PR: `cosign.pub` → `distro/keys/alpha-os.pub`. С этого коммита образы собираются с `enforce`, а CI сверяет подпись этим ключом.
5. После первой публикации: пакет `alpha-os` в GHCR сделать **public** (иначе устройства не скачают образ) и связать с репозиторием.
6. Позже — перенос ключа в KMS (`cosign --key gcpkms://…`/`awskms://…`) и ротация: новый образ сначала поставляет оба ключа.

## Ограничения прототипа

- Нет брендинга (DIST-003), нет настоящих сервисов (DIST-009, DIST-013), нет health-check-ов greenboot (DIST-011).
- Пакеты не залочены по версиям (lockfile — отдельной задачей).
- Офлайн-обновления с USB (`oci-archive`) политикой пока отклоняются: проверку подписи для них решим вместе с офлайн-обновлением.
- Проверено только в QEMU/KVM; железо (Intel N100, AMD APU, `vainfo`, QSV/VAAPI) — отдельно.
