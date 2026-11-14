// SPDX-License-Identifier: MPL-2.0
//! Writes the pa-vfs contract fixture into a directory (seeds the Samba
//! containers in `tests/servers/`).
//!
//! `cargo run -p pa-vfs-smb --example write_fixture -- <dir>`

fn main() -> std::io::Result<()> {
    let dir = std::env::args_os()
        .nth(1)
        .ok_or_else(|| std::io::Error::other("usage: write_fixture <dir>"))?;
    std::fs::create_dir_all(&dir)?;
    pa_vfs::contract::write_fixture(dir.as_ref())
}
