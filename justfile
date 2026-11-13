# SPDX-License-Identifier: MPL-2.0
# Root justfile: only `mod` lines (docs/ROADMAP.md §0.5).
# Recipes live in each area's own justfile. List everything with:
#   just --list --list-submodules

mod android 'clients/android/justfile'
mod apple 'clients/apple/justfile'
mod bootstrap 'tools/bootstrap/justfile'
mod core 'core/justfile'
mod design 'design/justfile'
mod desktop 'clients/desktop/justfile'
mod distro 'distro/justfile'
mod docs 'docs/justfile'
mod flutter 'clients/flutter/justfile'
mod packaging 'packaging/justfile'
mod roadmap 'tools/roadmap/justfile'
mod server 'server/justfile'
mod tests 'tests/justfile'
