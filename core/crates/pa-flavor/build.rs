// SPDX-License-Identifier: MPL-2.0
//! Publishes the resolved flavor to the build scripts of dependent crates
//! (`links` metadata, read back by `pa_flavor::build::emit_cfgs`) and sets
//! the same cfgs on this crate.

use std::env;

/// (cargo feature env suffix, metadata key, cfg name)
const CAPABILITIES: [(&str, &str, &str); 3] = [
    ("TWITCH_NATIVE", "twitch_native", "pa_twitch_native"),
    ("TWITCH_ARCHIVE", "twitch_archive", "pa_twitch_archive"),
    ("DONATION_LINKS", "donation_links", "pa_donation_links"),
];

fn main() {
    println!("cargo::rerun-if-changed=build.rs");
    println!("cargo::rustc-check-cfg=cfg(pa_flavor, values(\"store\", \"full\"))");
    let mut full = false;
    for (feature, key, cfg) in CAPABILITIES {
        println!("cargo::rustc-check-cfg=cfg({cfg})");
        let on = env::var_os(format!("CARGO_FEATURE_{feature}")).is_some();
        full |= on;
        println!("cargo::metadata={key}={}", u8::from(on));
        if on {
            println!("cargo::rustc-cfg={cfg}");
        }
    }
    let flavor = if full { "full" } else { "store" };
    println!("cargo::metadata=flavor={flavor}");
    println!("cargo::rustc-cfg=pa_flavor=\"{flavor}\"");
}
