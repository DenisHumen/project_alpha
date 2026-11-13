#!/usr/bin/env bash
# SPDX-License-Identifier: MPL-2.0
# Boot a raw disk in QEMU/KVM (headless, serial console to a log file).
#   distro/scripts/run-vm.sh [DISK]
# SSH: ssh -i distro/.work/vm_key -p $VM_SSH_PORT root@127.0.0.1
# Set VM_UEFI=1 to boot with OVMF instead of SeaBIOS (the generic image supports both).
set -euo pipefail
source "$(dirname "$0")/lib.sh"

DISK="${1:-${WORK_DIR}/disk.raw}"
[[ -f "${DISK}" ]] || die "disk not found: ${DISK}"
[[ -w /dev/kvm ]] || die "/dev/kvm is not writable (add the user to the kvm group)"

PIDFILE="${WORK_DIR}/qemu.pid"
SERIAL="${WORK_DIR}/serial.log"

firmware=()
if [[ "${VM_UEFI:-0}" == 1 ]]; then
    code="${OVMF_CODE:-/usr/share/OVMF/OVMF_CODE_4M.fd}"
    vars_src="${OVMF_VARS:-/usr/share/OVMF/OVMF_VARS_4M.fd}"
    cp -n "${vars_src}" "${WORK_DIR}/OVMF_VARS.fd"
    firmware=(-drive "if=pflash,format=raw,readonly=on,file=${code}"
              -drive "if=pflash,format=raw,file=${WORK_DIR}/OVMF_VARS.fd")
fi

log "Starting VM (${VM_CPUS} vCPU, ${VM_MEM_MB} MiB), ssh on 127.0.0.1:${VM_SSH_PORT}"
nice -n 10 qemu-system-x86_64 \
    -enable-kvm -machine q35 -cpu host \
    -smp "${VM_CPUS}" -m "${VM_MEM_MB}" \
    "${firmware[@]}" \
    -drive "file=${DISK},format=raw,if=virtio,cache=unsafe" \
    -netdev "user,id=n0,hostfwd=tcp:127.0.0.1:${VM_SSH_PORT}-:22" \
    -device virtio-net-pci,netdev=n0 \
    -display none -serial "file:${SERIAL}" \
    -daemonize -pidfile "${PIDFILE}"
log "VM pid $(cat "${PIDFILE}"), serial log: ${SERIAL}"
