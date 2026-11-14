# SPDX-License-Identifier: MPL-2.0
#
# Pinned upstream source for the LGPL libsmb2 build (NET-002).
# The tarball is verified against its SHA-256 before use. Bumping the version
# means updating the version, URL and hash together in one commit.
#
# 6.3.x is the maintained 6-branch (security fixes on top of 6.2, same API,
# ABI and soname); master has unreleased API changes.

LIBSMB2_VERSION=6.3.2
LIBSMB2_URL="https://github.com/sahlberg/libsmb2/archive/refs/tags/libsmb2-${LIBSMB2_VERSION}.tar.gz"
LIBSMB2_SHA256=a9e59c2e0bf5fe1232142e2ae3a00697046a0201ce85ae5f80af714cc85dc66a
# lib/ and include/ (the library) are LGPL-2.1-or-later; examples/ is
# BSD-2-Clause and is not built.
LIBSMB2_LICENSE="LGPL-2.1-or-later"

PA_SMB2_PACKAGES="libsmb2"
