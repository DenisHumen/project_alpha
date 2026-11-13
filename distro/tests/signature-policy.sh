#!/usr/bin/env bash
# SPDX-License-Identifier: MPL-2.0
# DIST-002: the device refuses unsigned images.
#   distro/tests/signature-policy.sh IMAGE
#
# IMAGE is a built OS image in the local engine storage. The check runs *inside* that image
# with its own skopeo/containers-image stack (the one bootc uses to pull updates) and the
# policy templates shipped in it:
#   1. a throwaway cosign key pair is generated (it never leaves the temp dir);
#   2. a local registry gets tiny test images: signed with that key, unsigned, signed with
#      another key, and a signed image in a different repository;
#   3. in the OS image, `alpha-signature-policy` installs the policy for the test repository
#      with the throwaway public key mounted at the key path;
#   4. `skopeo copy` must accept only the correctly signed image.
# If the image was built with ALPHA_SIGNATURE_POLICY=enforce, the shipped policy is also
# checked: default reject and our repository bound to the key.
#
# Needs: ENGINE (docker or rootful podman), cosign v2 in PATH, curl.
set -euo pipefail
source "$(dirname "$0")/../scripts/lib.sh"

IMAGE="${1:?OS image required, e.g. localhost/alpha-os:dev}"
PORT="${SIGTEST_PORT:-5055}"
REG="localhost:${PORT}"
REPO="${REG}/alpha-os"
OTHER="${REG}/not-alpha-os"
NAME="alpha-os-sigtest-registry"

command -v cosign >/dev/null || die "cosign not found"
command -v curl >/dev/null || die "curl not found"
mkdir -p "${WORK_DIR}"
TMP="$(mktemp -d "${WORK_DIR}/sigtest.XXXXXX")"
cleanup() {
    "${ENGINE}" rm -f "${NAME}" >/dev/null 2>&1 || true
    rm -rf "${TMP}"
}
trap cleanup EXIT

push_flags=()
[[ "$(basename "${ENGINE}")" == podman ]] && push_flags=(--tls-verify=false)

log "Starting test registry on ${REG}"
"${ENGINE}" rm -f "${NAME}" >/dev/null 2>&1 || true
"${ENGINE}" run -d --name "${NAME}" -p "127.0.0.1:${PORT}:5000" docker.io/library/registry:2 >/dev/null
for _ in $(seq 30); do curl -fs "http://${REG}/v2/" >/dev/null && break; sleep 1; done

log "Generating throwaway cosign keys"
export COSIGN_PASSWORD=""
(cd "${TMP}" && cosign generate-key-pair --output-key-prefix good >/dev/null 2>&1 \
    && cosign generate-key-pair --output-key-prefix bad >/dev/null 2>&1)

# Tiny distinct images (different digests: a signature covers one digest).
push_test_image() {
    local ref="$1" ctx="${TMP}/ctx-${RANDOM}"
    mkdir -p "${ctx}"
    printf '%s\n' "${ref}" > "${ctx}/marker"
    printf 'FROM scratch\nCOPY marker /marker\n' > "${ctx}/Containerfile"
    "${ENGINE}" build -q -f "${ctx}/Containerfile" -t "${ref}" "${ctx}" >/dev/null
    "${ENGINE}" push -q "${push_flags[@]}" "${ref}" >/dev/null
    "${ENGINE}" rmi -f "${ref}" >/dev/null 2>&1 || true
}
digest_of() {   # repo tag -> sha256:...
    curl -fsI \
        -H 'Accept: application/vnd.oci.image.index.v1+json' \
        -H 'Accept: application/vnd.oci.image.manifest.v1+json' \
        -H 'Accept: application/vnd.docker.distribution.manifest.list.v2+json' \
        -H 'Accept: application/vnd.docker.distribution.manifest.v2+json' \
        "http://${REG}/v2/${1#"${REG}/"}/manifests/$2" \
        | tr -d '\r' | sed -n 's/^[Dd]ocker-[Cc]ontent-[Dd]igest: //p'
}
sign() {   # ref key
    local ref="$1:$2" digest
    digest="$(digest_of "$1" "$2")"
    [[ -n "${digest}" ]] || die "no digest for ${ref}"
    COSIGN_PRIVATE_KEY="$(cat "${TMP}/$3.key")" COSIGN_TLOG=false \
        "${DISTRO_DIR}/scripts/sign-image.sh" "$1@${digest}" --allow-http-registry >/dev/null 2>&1 \
        || die "cosign could not sign ${ref}"
}

log "Publishing test images"
push_test_image "${REPO}:signed";   sign "${REPO}" signed good
push_test_image "${REPO}:unsigned"
push_test_image "${REPO}:wrongkey"; sign "${REPO}" wrongkey bad
push_test_image "${OTHER}:signed";  sign "${OTHER}" signed good

log "Checking the policy inside ${IMAGE}"
# shellcheck disable=SC2016  # expanded inside the OS image
check='
set -uo pipefail
. /usr/share/alpha/version
echo "image: ALPHA_OS_VERSION=${ALPHA_OS_VERSION} policy=${ALPHA_SIGNATURE_POLICY:-unknown}"
fail=0
if [ "${ALPHA_SIGNATURE_POLICY:-}" = enforce ]; then
    python3 - "${ALPHA_IMAGE_REPO}" <<"PY" || fail=1
import json, sys
p = json.load(open("/etc/containers/policy.json"))
repo = sys.argv[1]
assert p["default"] == [{"type": "reject"}], p["default"]
req = p["transports"]["docker"][repo][0]
assert req["type"] == "sigstoreSigned" and req["keyPath"] == "/usr/share/alpha/keys/alpha-os.pub", req
print("OK shipped policy: default reject, " + repo + " requires our signature")
PY
fi
/usr/libexec/alpha/alpha-signature-policy "$1" >/dev/null || exit 1
expect() {
    out="$(skopeo copy --quiet --src-tls-verify=false "docker://$2" "dir:$(mktemp -d)" 2>&1)"; rc=$?
    if [ "$1" = accept ] && [ "${rc}" = 0 ]; then
        echo "OK accepted  $2"
    elif [ "$1" = reject ] && [ "${rc}" != 0 ] && printf "%s" "${out}" | grep -Eqi "signature|rejected by policy"; then
        echo "OK rejected  $2: $(printf "%s" "${out}" | tail -n1 | cut -c1-160)"
    else
        echo "FAIL expected $1 for $2 (rc=${rc}): ${out}"; fail=1
    fi
}
expect accept "$1:signed"
expect reject "$1:unsigned"
expect reject "$1:wrongkey"
expect reject "$2:signed"
exit "${fail}"
'
if "${ENGINE}" run --rm --net=host \
        -v "${TMP}/good.pub:/usr/share/alpha/keys/alpha-os.pub:ro" \
        "${IMAGE}" bash -c "${check}" check "${REPO}" "${OTHER}"; then
    log "Signature policy test PASSED: unsigned and foreign images are rejected"
else
    die "signature policy test FAILED"
fi
