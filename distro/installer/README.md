<!-- SPDX-License-Identifier: MPL-2.0 -->
# distro/installer — `alpha-installer` (Go)

Установщик NAS-ОС ([исследование 05 §2.3–2.4](../../docs/research/05-nas-linux-distro.md), [ADR-0010](../../docs/adr/0010-nas-os-phase0.md)). Пока в нём только **обнаружение дисков** (DIST-004). Разметка и `bootc install` появятся в DIST-005, TUI — в DIST-006.

## Обнаружение дисков (`internal/disks`)

Пакет работает **только на чтение**. Он запускает `lsblk -J -b -o …`, `udevadm info --query=property` и, если попросить, `smartctl -H -j`, а также читает `/sys/block/*/device/model` и `/proc/cmdline`. На блочные устройства он ничего не пишет.

Что пакет делает:

- **Человеческое имя.** Для NVMe модель берётся из sysfs, для остальных — `MODEL` из lsblk, затем `ID_MODEL_ENC` из udev. Пробелы нормализуются, бренд приводится к написанию «как на коробке»: `Samsung SSD 990 PRO 1TB` → `Samsung 990 PRO 1TB`, `WDC WD40EFRX-68N32N0` → `WD WD40EFRX-68N32N0`, `ST4000VN008` → `Seagate ST4000VN008`, `WD_BLACK SN850X` → `WD Black SN850X`. У USB, SAS и SCSI к модели добавляется `VENDOR`, если он не общий (`ATA`, `Generic`). Если модели нет совсем, имя строится как «вендор + тип + размер», например `JMicron USB drive 500 GB` или `Virtual disk 69 GB`.
- **Размер в десятичных единицах**, как на коробке: 1 TB = 10¹² байт (`1000204886016` → `1.0 TB`).
- **Шина и тип носителя**: NVMe, SATA SSD/HDD, SAS HDD, USB, eMMC, SD/MMC, virtio. Для QEMU и VirtualBox пишется «virtual».
- **Исключённые устройства** возвращаются с причиной, чтобы интерфейс мог объяснить, почему диска нет в списке:
  - `virtual-device` — loop, zram, ram, nbd;
  - `optical` — `sr*`;
  - `boot-media` — собственная загрузочная флешка установщика или диск, с которого работает текущая система. Признаки: в поддереве смонтирован `/run/initramfs/live`, `/`, `/sysroot` или `/boot`, либо совпадает метка, UUID или путь из `root=live:…` в `/proc/cmdline`; это работает и при `rd.live.ram=1`;
  - `no-media` — пустой картридер;
  - `read-only`;
  - `too-small` — меньше 16 GB; это временный порог, окончательное значение определит DIST-005;
  - `hardware-area` — `mmcblkXbootN` и `rpmb`.
- **Признаки существующих данных**: таблица разделов (GPT или DOS) и число разделов; файловые системы с метками; участники mdraid вместе с массивом и ФС на нём; LVM, LUKS, ZFS (с именем пула), swap; ESP («вероятно, установлена ОС»); Microsoft reserved («Windows»); смонтированные разделы; провал SMART.

Результат возвращается как `[]disks.Disk`. JSON-форма этой структуры — контракт для TUI и будущего веб-установщика. Тексты находок в `Finding.String()` даны на английском и служат запасным вариантом; локализует их TUI по полю `Kind`.

## Команда

```bash
cd distro/installer
go run ./cmd/alpha-installer disks             # кандидаты для установки
go run ./cmd/alpha-installer disks --all       # вместе с исключёнными и причинами
go run ./cmd/alpha-installer disks --json --smart
go run ./cmd/alpha-installer disks --record DIR   # снимок железа для баг-репорта (S/N и WWN скрыты)
go run ./cmd/alpha-installer disks --from DIR     # воспроизвести снимок
```

## Тесты

```bash
go vet ./... && go test ./...          # go test -run TestGolden -update — обновить эталоны
```

В `internal/disks/testdata/` лежат 10 конфигураций железа. Каждая состоит из `lsblk.json`, свойств udev, файлов sysfs, `/proc/cmdline`, ответов `smartctl` и эталона `expected.txt`:

| Конфигурация | Что проверяет |
|---|---|
| `01-live-usb-nas` | live-USB, NVMe, SATA SSD, 2×HDD, loop и zram |
| `02-mdadm-reinstall` | mdadm RAID1 с btrfs, участник RAID на целом диске, прошлая установка на NVMe, флешка не смонтирована (`rd.live.ram`), SMART FAILED |
| `03-vm-virtio` | virtio без модели, загрузка с виртуального CD |
| `04-qemu-ci` | QEMU NVMe и 3 SATA HDD, как в DIST-008 |
| `05-windows-desktop` | OEM-Samsung с Windows (ESP, MSR), NTFS на Crucial |
| `06-n100-emmc` | eMMC с boot-областями, SD-карта, WD_BLACK |
| `07-usb-mix` | внешний HDD, пустой картридер, маленькая флешка, мост без модели, загрузка по UUID |
| `08-legacy-lsblk-sas` | старый формат lsblk (строки, `mountpoint`, без `path`), SAS, ZFS, LVM, LUKS, Micron с `_` |
| `09-dual-nvme-previous-install` | модель из sysfs при пустом lsblk, прошлая установка с разделом `media`, `root=live:/dev/sda1` |
| `10-workstation-host` | реальный снимок рабочей станции (S/N скрыты): диск с `/` исключается |
