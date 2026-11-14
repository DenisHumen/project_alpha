// SPDX-License-Identifier: MPL-2.0
//! Build flavor cfgs (`cfg(pa_twitch_native)` and friends) from `pa-flavor`
//! (CORE-013).

fn main() {
    pa_flavor::build::emit_cfgs();
}
