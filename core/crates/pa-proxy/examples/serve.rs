// SPDX-License-Identifier: MPL-2.0
//! Serves a local folder through the proxy and prints the URL of a file,
//! for checking the proxy with real players (`ffprobe`, `mpv`, `curl`):
//!
//! ```sh
//! cargo run -p pa-proxy --example serve -- /srv/media Movies/Dune.mkv
//! ffprobe "$(the printed URL)"
//! ```
//!
//! Runs until stdin is closed (or Enter is pressed).

use std::sync::Arc;

use pa_proxy::{Opener, Proxy, ProxyOptions};
use pa_vfs::{ByteSource, LocalFs, Vfs, VfsPath};

struct Folder(LocalFs);

impl Opener for Folder {
    fn open(&self, resource: &str) -> pa_vfs::Result<Arc<dyn ByteSource>> {
        Ok(Arc::from(self.0.open(&VfsPath::parse(resource)?)?))
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let (Some(root), Some(file)) = (args.next(), args.next()) else {
        eprintln!("usage: serve <folder> <file inside it>");
        std::process::exit(2);
    };
    let proxy = Proxy::start(
        Arc::new(Folder(LocalFs::new(root)?)),
        ProxyOptions::default(),
    )?;
    println!("{}", proxy.url(&file));
    let mut line = String::new();
    std::io::stdin().read_line(&mut line)?;
    Ok(())
}
