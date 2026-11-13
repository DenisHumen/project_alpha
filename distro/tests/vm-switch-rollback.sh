#!/usr/bin/env bash
# SPDX-License-Identifier: MPL-2.0
# Phase-0 VM check (DIST-001): switch / upgrade / rollback of the NAS OS image.
#
#   distro/tests/vm-switch-rollback.sh
#
# Scenario (each step ends with a reboot and assertions over SSH):
#   0. install the stock pinned base image to a raw disk, boot it;
#   1. `bootc switch` to our image v1 (local registry)  -> v1 booted, our services healthy;
#   2. `bootc rollback`                                   -> stock base booted again;
#   3. `bootc rollback` (back to v1)                      -> v1 booted;
#   4. push v2 under the same tag, `bootc upgrade`        -> v2 booted;
#   5. `bootc rollback`                                   -> v1 booted;
#   6. `bootc switch` to v2 by digest, then back to the tag; `bootc upgrade --check` still
#      tracks the tag (needed for staged rollouts, 05 §4.2 p.3).
#
# Requirements: docker (or rootful podman) with --privileged, qemu-system-x86_64, /dev/kvm,
# ssh, python3, ~6 GB free disk in distro/.work. Takes ~10 minutes.
# KEEP=1 leaves the VM and the registry running for manual poking.
set -euo pipefail
source "$(dirname "$0")/../scripts/lib.sh"

mkdir -p "${WORK_DIR}"
KEY="${WORK_DIR}/vm_key"
[[ -f "${KEY}" ]] || ssh-keygen -q -t ed25519 -N '' -C alpha-os-vm-test -f "${KEY}"

LOCAL_REPO="localhost:${REGISTRY_PORT}/alpha-os"
VM_REPO="10.0.2.2:${REGISTRY_PORT}/alpha-os"
LOCAL_BASE="localhost:${REGISTRY_PORT}/fedora-bootc:45"

SSH=(ssh -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -o LogLevel=ERROR
     -o ConnectTimeout=5 -o BatchMode=yes -i "${KEY}" -p "${VM_SSH_PORT}" root@127.0.0.1)
vm() { "${SSH[@]}" "$@"; }

cleanup() {
    if [[ "${KEEP:-0}" == 1 ]]; then
        log "KEEP=1: VM and registry left running"
        return
    fi
    if [[ -f "${WORK_DIR}/qemu.pid" ]]; then
        kill "$(cat "${WORK_DIR}/qemu.pid")" 2>/dev/null || true
        rm -f "${WORK_DIR}/qemu.pid"
    fi
    "${ENGINE}" rm -f "${REGISTRY_NAME}" >/dev/null 2>&1 || true
}
trap cleanup EXIT

wait_ssh() {
    local i
    for i in $(seq 1 60); do
        if vm true 2>/dev/null; then return 0; fi
        sleep 5
    done
    die "VM did not come up over SSH (see ${WORK_DIR}/serial.log)"
}

reboot_vm() {
    log "Rebooting VM"
    vm systemctl reboot || true
    # Wait until SSH goes away so we do not talk to the old boot.
    local i
    for i in $(seq 1 30); do
        vm true 2>/dev/null || break
        sleep 2
    done
    wait_ssh
    vm systemctl is-system-running --wait >/dev/null || true
}

# Prints "<image ref> <ALPHA_OS_VERSION or 'stock'>" of the booted deployment.
booted() {
    local ref ver
    ref="$(vm bootc status --format=json | python3 -c \
        'import json,sys; print(json.load(sys.stdin)["status"]["booted"]["image"]["image"]["image"])')"
    ver="$(vm 'cat /usr/share/alpha/version 2>/dev/null | sed -n "s/^ALPHA_OS_VERSION=//p"')"
    echo "${ref} ${ver:-stock}"
}

expect_booted() {
    local want_ref="$1" want_ver="$2" got
    got="$(booted)"
    [[ "${got}" == "${want_ref} ${want_ver}" ]] || die "booted: '${got}', expected '${want_ref} ${want_ver}'"
    log "OK booted ${got}"
}

check_services() {
    local svc port
    for svc in alpha-media alpha-admin alpha-updated smb wsdd avahi-daemon firewalld chronyd; do
        vm systemctl is-active --quiet "${svc}" || die "service ${svc} is not active"
    done
    for port in 8487 80 7091; do
        vm "curl -fsS http://127.0.0.1:${port}/health" >/dev/null || die "no /health on :${port}"
    done
    vm "ss -ltn | grep -q ':445 '" || die "smbd is not listening on 445"
    [[ "$(vm firewall-cmd --get-default-zone)" == home ]] || die "firewalld default zone is not home"
    vm "systemctl is-enabled bootc-fetch-apply-updates.timer | grep -qx masked" \
        || die "bootc-fetch-apply-updates.timer is not masked"
    log "OK services healthy, timer masked, firewall zone home"
}

# --- build and publish images ------------------------------------------------------------
log "Starting local registry on 127.0.0.1:${REGISTRY_PORT}"
"${ENGINE}" rm -f "${REGISTRY_NAME}" >/dev/null 2>&1 || true
"${ENGINE}" run -d --name "${REGISTRY_NAME}" -p "127.0.0.1:${REGISTRY_PORT}:5000" \
    docker.io/library/registry:2 >/dev/null

"${ENGINE}" pull -q "${BASE_IMAGE}" >/dev/null
"${ENGINE}" tag "${BASE_IMAGE}" "${LOCAL_BASE}"
"${ENGINE}" push -q "${LOCAL_BASE}" >/dev/null

# Test images are unsigned and come from a local registry: keep the stock signature policy
# (the policy itself is covered by tests/signature-policy.sh).
export ALPHA_SIGNATURE_POLICY=off
V1="0.0.1-test.1"
V2="0.0.1-test.2"
"${DISTRO_DIR}/scripts/build-image.sh" "${LOCAL_REPO}:v1" "${V1}"
"${DISTRO_DIR}/scripts/build-image.sh" "${LOCAL_REPO}:v2" "${V2}"
"${ENGINE}" tag "${LOCAL_REPO}:v1" "${LOCAL_REPO}:dev"
"${ENGINE}" push -q "${LOCAL_REPO}:dev" >/dev/null
"${ENGINE}" push -q "${LOCAL_REPO}:v2" >/dev/null

# --- step 0: stock base on disk ------------------------------------------------------------
STOCK_REF="${BASE_IMAGE%@*}"   # tag only; what a stock install would track
"${DISTRO_DIR}/scripts/make-disk.sh" "docker://${LOCAL_BASE}" "${STOCK_REF}" \
    "${WORK_DIR}/disk.raw" "${KEY}.pub"
"${DISTRO_DIR}/scripts/run-vm.sh" "${WORK_DIR}/disk.raw"
wait_ssh
expect_booted "${STOCK_REF}" stock

# The VM pulls from the plain-HTTP registry on the host.
vm "printf '[[registry]]\nlocation = \"10.0.2.2:${REGISTRY_PORT}\"\ninsecure = true\n' \
    > /etc/containers/registries.conf.d/50-alpha-local.conf"

# --- step 1: switch to our image ----------------------------------------------------------
log "Step 1: bootc switch ${VM_REPO}:dev"
vm bootc switch "${VM_REPO}:dev"
reboot_vm
expect_booted "${VM_REPO}:dev" "${V1}"
check_services

# --- step 2: rollback to stock -------------------------------------------------------------
log "Step 2: bootc rollback (to stock base)"
vm bootc rollback
reboot_vm
expect_booted "${STOCK_REF}" stock

# --- step 3: rollback again (to v1) --------------------------------------------------------
log "Step 3: bootc rollback (back to v1)"
vm bootc rollback
reboot_vm
expect_booted "${VM_REPO}:dev" "${V1}"

# --- step 4: upgrade to v2 ------------------------------------------------------------------
log "Step 4: publish v2 as :dev and bootc upgrade"
"${ENGINE}" tag "${LOCAL_REPO}:v2" "${LOCAL_REPO}:dev"
"${ENGINE}" push -q "${LOCAL_REPO}:dev" >/dev/null
vm bootc upgrade --check
vm bootc upgrade
reboot_vm
expect_booted "${VM_REPO}:dev" "${V2}"
check_services

# --- step 5: rollback to v1 -----------------------------------------------------------------
log "Step 5: bootc rollback (to v1)"
vm bootc rollback
reboot_vm
expect_booted "${VM_REPO}:dev" "${V1}"
check_services

# --- step 6: pin by digest and return to the tag ------------------------------------------
V2_DIGEST="$("${ENGINE}" inspect --format '{{range .RepoDigests}}{{println .}}{{end}}' \
    "${LOCAL_REPO}:v2" | grep -m1 "^${LOCAL_REPO}@" | cut -d@ -f2)"
[[ -n "${V2_DIGEST}" ]] || die "could not resolve the digest of v2"
log "Step 6: bootc switch ${VM_REPO}@${V2_DIGEST}"
vm bootc switch "${VM_REPO}@${V2_DIGEST}"
reboot_vm
expect_booted "${VM_REPO}@${V2_DIGEST}" "${V2}"
log "Step 6: bootc switch back to ${VM_REPO}:dev"
vm bootc switch "${VM_REPO}:dev"
vm bootc upgrade --check
reboot_vm
expect_booted "${VM_REPO}:dev" "${V2}"

log "ALL STEPS PASSED"
