// SPDX-License-Identifier: MPL-2.0
//! Test helpers: a tiny Matroska writer, a network-like source wrapper,
//! a temp dir and `dlopen` (std only; the libraries are found at run time).

#![allow(dead_code)]

use std::ffi::{CStr, CString, c_char, c_int, c_void};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use pa_vfs::ByteSource;

// ---------------------------------------------------------------- Matroska

/// Audio blocks of the test file: 8 kHz mono s16le, 100 ms each.
pub const BLOCK_MS: u64 = 100;
pub const BLOCK_BYTES: usize = 1600;

fn element(id: &[u8], payload: &[u8]) -> Vec<u8> {
    let mut out = id.to_vec();
    // 8-byte EBML size: marker 0x01 then 7 bytes big-endian.
    out.push(0x01);
    out.extend_from_slice(&(payload.len() as u64).to_be_bytes()[1..]);
    out.extend_from_slice(payload);
    out
}

fn uint(id: &[u8], v: u64) -> Vec<u8> {
    let bytes = v.to_be_bytes();
    let skip = bytes.iter().take(7).take_while(|b| **b == 0).count();
    element(id, &bytes[skip..])
}

fn float(id: &[u8], v: f64) -> Vec<u8> {
    element(id, &v.to_be_bytes())
}

fn string(id: &[u8], v: &str) -> Vec<u8> {
    element(id, v.as_bytes())
}

/// Payload byte `i` of block `n`.
pub fn sample_byte(block: usize, i: usize) -> u8 {
    (block * 7 + i) as u8
}

/// A valid Matroska file with one PCM audio track of `blocks` × 100 ms,
/// one cluster per second, no cues: enough for libavformat and mpv.
pub fn mkv(blocks: usize) -> Vec<u8> {
    let header = [
        uint(&[0x42, 0x86], 1),
        uint(&[0x42, 0xF7], 1),
        uint(&[0x42, 0xF2], 4),
        uint(&[0x42, 0xF3], 8),
        string(&[0x42, 0x82], "matroska"),
        uint(&[0x42, 0x87], 4),
        uint(&[0x42, 0x85], 2),
    ]
    .concat();
    let info = [
        uint(&[0x2A, 0xD7, 0xB1], 1_000_000),
        string(&[0x4D, 0x80], "pa-player-io test"),
        string(&[0x57, 0x41], "pa-player-io test"),
        float(&[0x44, 0x89], (blocks as u64 * BLOCK_MS) as f64),
    ]
    .concat();
    let audio = [
        float(&[0xB5], 8000.0),
        uint(&[0x9F], 1),
        uint(&[0x62, 0x64], 16),
    ]
    .concat();
    let track = [
        uint(&[0xD7], 1),
        uint(&[0x73, 0xC5], 1),
        uint(&[0x83], 2),
        string(&[0x86], "A_PCM/INT/LIT"),
        element(&[0xE1], &audio),
    ]
    .concat();
    let mut segment = [
        element(&[0x15, 0x49, 0xA9, 0x66], &info),
        element(&[0x16, 0x54, 0xAE, 0x6B], &element(&[0xAE], &track)),
    ]
    .concat();
    let per_cluster = (1000 / BLOCK_MS) as usize;
    for first in (0..blocks).step_by(per_cluster) {
        let cluster_ms = first as u64 * BLOCK_MS;
        let mut cluster = uint(&[0xE7], cluster_ms);
        for n in first..blocks.min(first + per_cluster) {
            let rel = ((n - first) as u64 * BLOCK_MS) as i16;
            let mut block = vec![0x81];
            block.extend_from_slice(&rel.to_be_bytes());
            block.push(0x80); // keyframe
            block.extend((0..BLOCK_BYTES).map(|i| sample_byte(n, i)));
            cluster.extend(element(&[0xA3], &block));
        }
        segment.extend(element(&[0x1F, 0x43, 0xB6, 0x75], &cluster));
    }
    [
        element(&[0x1A, 0x45, 0xDF, 0xA3], &header),
        element(&[0x18, 0x53, 0x80, 0x67], &segment),
    ]
    .concat()
}

// ------------------------------------------------------- network-like source

/// Wraps a source like a network backend: at most `chunk` bytes per call;
/// counts calls and remembers the furthest byte served.
#[derive(Debug)]
pub struct Choppy<S> {
    pub inner: S,
    pub chunk: usize,
    pub calls: AtomicU64,
    pub max_end: AtomicU64,
}

impl<S> Choppy<S> {
    pub fn new(inner: S, chunk: usize) -> Self {
        Self {
            inner,
            chunk,
            calls: AtomicU64::new(0),
            max_end: AtomicU64::new(0),
        }
    }
}

impl<S: ByteSource> ByteSource for Choppy<S> {
    fn size(&self) -> u64 {
        self.inner.size()
    }
    fn read_at(&self, offset: u64, buf: &mut [u8]) -> pa_vfs::Result<usize> {
        self.calls.fetch_add(1, Ordering::Relaxed);
        let n = buf.len().min(self.chunk);
        let got = self.inner.read_at(offset, &mut buf[..n])?;
        self.max_end
            .fetch_max(offset + got as u64, Ordering::Relaxed);
        Ok(got)
    }
}

// ------------------------------------------------------------------ temp dir

#[derive(Debug)]
pub struct TempDir(PathBuf);

impl TempDir {
    pub fn new() -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!("pa-player-io-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("create temp dir");
        Self(path)
    }

    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

// -------------------------------------------------------------------- dlopen

/// Directory with our LGPL FFmpeg/libmpv build (`tools/mpv/out/<target>/lib`).
/// Tests that need the real libraries skip when it is not set.
pub fn lib_dir() -> Option<PathBuf> {
    match std::env::var_os("PA_PLAYER_IO_LIB_DIR") {
        Some(dir) => Some(PathBuf::from(dir)),
        None => {
            eprintln!("PA_PLAYER_IO_LIB_DIR is not set: skipping the real-library test");
            None
        }
    }
}

#[cfg(unix)]
unsafe extern "C" {
    fn dlopen(filename: *const c_char, flag: c_int) -> *mut c_void;
    fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
    fn dlerror() -> *mut c_char;
}

#[cfg(target_os = "macos")]
const RTLD_NOW_GLOBAL: c_int = 0x2 | 0x8;
#[cfg(all(unix, not(target_os = "macos")))]
const RTLD_NOW_GLOBAL: c_int = 0x2 | 0x100;

/// A shared library opened for the whole test process (never closed).
#[cfg(unix)]
pub struct Lib(*mut c_void);

#[cfg(unix)]
impl Lib {
    /// Opens `lib<name>.so` / `lib<name>.dylib` from `dir`.
    pub fn open(dir: &Path, name: &str) -> Self {
        let ext = if cfg!(target_os = "macos") {
            "dylib"
        } else {
            "so"
        };
        let path = dir.join(format!("lib{name}.{ext}"));
        let c = CString::new(path.to_str().unwrap()).unwrap();
        let handle = unsafe { dlopen(c.as_ptr(), RTLD_NOW_GLOBAL) };
        assert!(
            !handle.is_null(),
            "dlopen {}: {}",
            path.display(),
            last_error()
        );
        Self(handle)
    }

    /// Resolves `name` as a function pointer of type `F`.
    ///
    /// # Safety
    ///
    /// `F` must be the symbol's real C signature.
    pub unsafe fn sym<F: Copy>(&self, name: &str) -> F {
        assert_eq!(std::mem::size_of::<F>(), std::mem::size_of::<*mut c_void>());
        let c = CString::new(name).unwrap();
        let p = unsafe { dlsym(self.0, c.as_ptr()) };
        assert!(!p.is_null(), "dlsym {name}: {}", last_error());
        unsafe { std::mem::transmute_copy::<*mut c_void, F>(&p) }
    }
}

#[cfg(unix)]
fn last_error() -> String {
    let e = unsafe { dlerror() };
    if e.is_null() {
        String::from("unknown error")
    } else {
        unsafe { CStr::from_ptr(e) }.to_string_lossy().into_owned()
    }
}
