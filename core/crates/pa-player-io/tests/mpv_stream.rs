// SPDX-License-Identifier: MPL-2.0
//! libmpv plays `pa://<id>` from a `pa-vfs` source through `stream_cb`
//! (NET-008). Needs our LGPL libmpv build:
//! `PA_PLAYER_IO_LIB_DIR=tools/mpv/out/linux-x86_64/lib`; skipped otherwise.

#![cfg(unix)]

mod common;

use std::ffi::{CStr, CString, c_char, c_double, c_int, c_void};
use std::ptr::null;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use common::{Choppy, Lib, TempDir, lib_dir, mkv};
use pa_player_io::mpv::{DEFAULT_SCHEME, MpvProtocol, SourceRegistry, StreamCbAddRoFn, stream_uri};
use pa_vfs::{ByteSource, ErrorKind, LocalFs, Vfs, VfsPath};

const EVENT_SHUTDOWN: c_int = 1;
const EVENT_END_FILE: c_int = 7;
const EVENT_FILE_LOADED: c_int = 8;
const END_FILE_REASON_EOF: c_int = 0;
const END_FILE_REASON_ERROR: c_int = 4;

/// `mpv_event`: event_id, error, reply_userdata, data.
#[repr(C)]
struct MpvEvent {
    event_id: c_int,
    error: c_int,
    reply_userdata: u64,
    data: *mut c_void,
}

/// `mpv_event_end_file`: reason, error, …
#[repr(C)]
struct EndFile {
    reason: c_int,
    error: c_int,
}

struct Mpv {
    create: unsafe extern "C" fn() -> *mut c_void,
    set_option_string: unsafe extern "C" fn(*mut c_void, *const c_char, *const c_char) -> c_int,
    initialize: unsafe extern "C" fn(*mut c_void) -> c_int,
    command: unsafe extern "C" fn(*mut c_void, *mut *const c_char) -> c_int,
    wait_event: unsafe extern "C" fn(*mut c_void, c_double) -> *mut MpvEvent,
    get_property_string: unsafe extern "C" fn(*mut c_void, *const c_char) -> *mut c_char,
    free: unsafe extern "C" fn(*mut c_void),
    terminate_destroy: unsafe extern "C" fn(*mut c_void),
    stream_cb_add_ro: StreamCbAddRoFn,
}

fn load() -> Option<Mpv> {
    let lib = Lib::open(&lib_dir()?, "mpv");
    unsafe {
        Some(Mpv {
            create: lib.sym("mpv_create"),
            set_option_string: lib.sym("mpv_set_option_string"),
            initialize: lib.sym("mpv_initialize"),
            command: lib.sym("mpv_command"),
            wait_event: lib.sym("mpv_wait_event"),
            get_property_string: lib.sym("mpv_get_property_string"),
            free: lib.sym("mpv_free"),
            terminate_destroy: lib.sym("mpv_terminate_destroy"),
            stream_cb_add_ro: lib.sym("mpv_stream_cb_add_ro"),
        })
    }
}

fn c(s: &str) -> CString {
    CString::new(s).unwrap()
}

/// A headless mpv core: no video or audio device, audio as fast as possible.
unsafe fn headless(m: &Mpv) -> *mut c_void {
    unsafe {
        let h = (m.create)();
        assert!(!h.is_null());
        for (k, v) in [
            ("vo", "null"),
            ("ao", "null"),
            ("ao-null-untimed", "yes"),
            ("terminal", "no"),
        ] {
            assert_eq!(
                (m.set_option_string)(h, c(k).as_ptr(), c(v).as_ptr()),
                0,
                "option {k}"
            );
        }
        assert_eq!((m.initialize)(h), 0);
        h
    }
}

struct Played {
    loaded: bool,
    file_format: Option<String>,
    duration: Option<f64>,
    end_reason: Option<c_int>,
}

unsafe fn property(m: &Mpv, h: *mut c_void, name: &str) -> Option<String> {
    let p = unsafe { (m.get_property_string)(h, c(name).as_ptr()) };
    if p.is_null() {
        return None;
    }
    let value = unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned();
    unsafe { (m.free)(p.cast()) };
    Some(value)
}

/// `loadfile uri` and waits for the end of the file.
unsafe fn play(m: &Mpv, h: *mut c_void, uri: &str) -> Played {
    let (cmd, uri) = (c("loadfile"), c(uri));
    let mut args = [cmd.as_ptr(), uri.as_ptr(), null()];
    assert_eq!(unsafe { (m.command)(h, args.as_mut_ptr()) }, 0);
    let mut played = Played {
        loaded: false,
        file_format: None,
        duration: None,
        end_reason: None,
    };
    let deadline = Instant::now() + Duration::from_secs(60);
    while Instant::now() < deadline {
        let ev = unsafe { &*(m.wait_event)(h, 1.0) };
        match ev.event_id {
            EVENT_FILE_LOADED => {
                played.loaded = true;
                played.file_format = unsafe { property(m, h, "file-format") };
                played.duration =
                    unsafe { property(m, h, "duration") }.and_then(|d| d.parse().ok());
            }
            EVENT_END_FILE => {
                played.end_reason = Some(unsafe { &*ev.data.cast::<EndFile>() }.reason);
                return played;
            }
            EVENT_SHUTDOWN => break,
            _ => {}
        }
    }
    played
}

#[test]
fn mpv_plays_pa_uri_from_a_vfs_source() {
    let Some(m) = load() else { return };
    let dir = TempDir::new();
    std::fs::write(dir.path().join("movie.mkv"), mkv(20)).unwrap();
    let vfs = LocalFs::new(dir.path()).unwrap();
    let file: Arc<dyn ByteSource> =
        Arc::from(vfs.open(&VfsPath::parse("/movie.mkv").unwrap()).unwrap());
    let source = Arc::new(Choppy::new(file, 3000));

    let registry = Arc::new(SourceRegistry::new());
    let id = registry.insert(source.clone());
    let protocol = MpvProtocol::new(DEFAULT_SCHEME, registry.clone());

    unsafe {
        let h = headless(&m);
        protocol.register(h, m.stream_cb_add_ro).unwrap();
        // A second registration of the same scheme is refused by mpv.
        assert!(protocol.register(h, m.stream_cb_add_ro).is_err());

        let played = play(&m, h, &stream_uri(DEFAULT_SCHEME, &id));
        assert!(played.loaded, "file never loaded");
        assert!(
            played
                .file_format
                .as_deref()
                .is_some_and(|f| f.contains("mkv") || f.contains("matroska")),
            "file-format: {:?}",
            played.file_format
        );
        let duration = played.duration.expect("duration");
        assert!((duration - 2.0).abs() < 0.05, "duration {duration}");
        assert_eq!(played.end_reason, Some(END_FILE_REASON_EOF));
        assert_eq!(source.max_end.load(Ordering::Relaxed), source.size());

        let missing = play(&m, h, "pa://missing");
        assert!(!missing.loaded);
        assert_eq!(missing.end_reason, Some(END_FILE_REASON_ERROR));
        assert_eq!(protocol.take_error().unwrap().kind(), ErrorKind::NotFound);

        (m.terminate_destroy)(h);
    }
    // mpv closed its stream: only the test and the registry hold the source.
    assert_eq!(Arc::strong_count(&source), 2);
    drop(protocol);
}
