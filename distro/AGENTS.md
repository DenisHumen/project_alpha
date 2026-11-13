<!-- SPDX-License-Identifier: MPL-2.0 -->

# AGENTS.md — `distro/` (NAS-ОС)

Общие правила — в корневом [AGENTS.md](../AGENTS.md). Подробности — [distro/README.md](README.md), решения — ADR-0010. По ADR-0011 NAS-ОС идёт после релиза 0.1 клиентов.

- **Что здесь:** image-based ОС на bootc: `Containerfile`, `packages.txt`, `rootfs/`, скрипты в `scripts/`, VM-тест в `tests/`. Владелец — workstream DIST.
- **Нужно:** docker (пользователь в группе `docker`) или rootful podman; для VM-теста — `qemu-system-x86_64`, `/dev/kvm`, `ssh`, `python3`, ~6 GB диска. sudo на хосте не нужен.

## Сборка и тесты (из корня репозитория)

```bash
distro/scripts/build-image.sh localhost/alpha-os:dev   # сборка образа + bootc container lint
distro/tests/vm-switch-rollback.sh                     # ~10 мин: switch → rollback → upgrade → digest-пин
shellcheck distro/scripts/*.sh distro/tests/*.sh       # если установлен
```

Локальные артефакты (диски, ключи SSH, логи) — в `distro/.work/`, он в `.gitignore`.

## Правила области

- В каждом скрипте и конфиге SPDX-заголовок (`# SPDX-License-Identifier: MPL-2.0`).
- Базовый образ — по digest; пакеты RPM Fusion и NVIDIA — только по задачам LEGAL-010 и DIST.
- Без бренда и логотипов Fedora: формулировка «основано на Fedora Linux» (DIST-003).
- Ключи подписи образа (cosign) — только в CI-окружении, см. [docs/dev/secrets.md](../docs/dev/secrets.md).
