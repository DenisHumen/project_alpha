# SPDX-License-Identifier: MPL-2.0
# Shared helpers for distro/ scripts. Source it, do not execute it.

DISTRO_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
WORK_DIR="${WORK_DIR:-${DISTRO_DIR}/.work}"

# Container engine: podman if present, otherwise docker. Override with ENGINE=...
ENGINE="${ENGINE:-$(command -v podman >/dev/null 2>&1 && echo podman || echo docker)}"

# Pinned base image (keep in sync with the ARG in distro/Containerfile).
BASE_IMAGE="$(sed -n 's/^ARG BASE_IMAGE=//p' "${DISTRO_DIR}/Containerfile")"

# Local throwaway registry used by the VM tests (the VM reaches the host as 10.0.2.2).
REGISTRY_PORT="${REGISTRY_PORT:-5000}"
REGISTRY_NAME="${REGISTRY_NAME:-alpha-os-registry}"

# VM limits: the build machine is shared, keep them small.
VM_CPUS="${VM_CPUS:-2}"
VM_MEM_MB="${VM_MEM_MB:-3072}"
VM_SSH_PORT="${VM_SSH_PORT:-22522}"
VM_DISK_SIZE="${VM_DISK_SIZE:-12G}"

log() { printf '\033[1;34m==>\033[0m %s\n' "$*" >&2; }
die() { printf '\033[1;31mERROR:\033[0m %s\n' "$*" >&2; exit 1; }
