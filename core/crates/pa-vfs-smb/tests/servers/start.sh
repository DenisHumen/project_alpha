#!/bin/sh
# SPDX-License-Identifier: MPL-2.0
#
# Entry point of the Samba test containers (alpine): installs Samba, creates
# the throwaway user and starts smbd in the foreground.
#   PA_SMB_SIGNING=mandatory   require signing on every connection
#   PA_SMB_NETEM="<args>"      shape egress with `tc qdisc ... netem <args>`
set -eu

apk add --no-cache samba iproute2 iproute2-tc >/dev/null

mkdir -p /etc/samba
cp /etc/pa/smb.conf /etc/samba/smb.conf
if [ -n "${PA_SMB_SIGNING:-}" ]; then
    sed -i "s/^\[global\]/[global]\n   server signing = ${PA_SMB_SIGNING}/" /etc/samba/smb.conf
fi

adduser -D -H -s /sbin/nologin "$PA_SMB_USER"
printf '%s\n%s\n' "$PA_SMB_PASS" "$PA_SMB_PASS" | smbpasswd -a -s "$PA_SMB_USER" >/dev/null

if [ -n "${PA_SMB_NETEM:-}" ]; then
    # shellcheck disable=SC2086 # netem arguments are a word list
    tc qdisc add dev eth0 root netem $PA_SMB_NETEM
    tc qdisc show dev eth0
fi

exec smbd --foreground --no-process-group --debug-stdout
