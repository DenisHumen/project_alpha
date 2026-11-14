// SPDX-License-Identifier: MPL-2.0
//! Prints what `pa-probe` finds in local files, as JSON.
//!
//! ```text
//! PA_FFMPEG_LIB_DIR=tools/ffmpeg/out/linux-x86_64/lib \
//!     cargo run -p pa-probe --example probe -- movie.mkv [more files…]
//! ```

use std::path::Path;
use std::sync::Arc;

use pa_probe::{Ffmpeg, ProbeOptions};
use pa_vfs::{LocalFs, Vfs, VfsPath};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let ffmpeg = Ffmpeg::load_default()?;
    for arg in std::env::args().skip(1) {
        let path = Path::new(&arg).canonicalize()?;
        let dir = path.parent().ok_or("no parent directory")?;
        let name = path.file_name().ok_or("no file name")?.to_string_lossy();
        let vfs = LocalFs::new(dir)?;
        let file = vfs.open(&VfsPath::parse(&format!("/{name}"))?)?;
        match ffmpeg.probe(Arc::from(file), &ProbeOptions::default()) {
            Ok(info) => println!("{}", serde_json::to_string_pretty(&info)?),
            Err(e) => eprintln!("{arg}: {:?}: {e}", e.kind()),
        }
    }
    Ok(())
}
