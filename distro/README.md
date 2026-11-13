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

## Ограничения прототипа

- Нет подписи образа и `policy.json` (DIST-002), нет брендинга (DIST-003), нет настоящих сервисов (DIST-009, DIST-013), нет health-check-ов greenboot (DIST-011).
- Пакеты не залочены по версиям (lockfile — вместе со сборкой в CI).
- Проверено только в QEMU/KVM; железо (Intel N100, AMD APU, `vainfo`, QSV/VAAPI) — отдельно.
