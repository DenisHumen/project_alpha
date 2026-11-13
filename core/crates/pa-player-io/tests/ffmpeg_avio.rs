// SPDX-License-Identifier: MPL-2.0
//! libavformat demuxes a Matroska file opened through `pa-vfs` via the
//! custom `AVIOContext` (NET-008). Needs our LGPL FFmpeg build:
//! `PA_PLAYER_IO_LIB_DIR=tools/mpv/out/linux-x86_64/lib`; skipped otherwise.

#![cfg(unix)]

mod common;

use std::ffi::{CStr, c_char, c_int, c_void};
use std::ptr::{null, null_mut};
use std::sync::Arc;
use std::sync::atomic::Ordering;

use common::{BLOCK_BYTES, Choppy, Lib, TempDir, lib_dir, mkv, sample_byte};
use pa_player_io::ffmpeg::{AvioApi, AvioContext, interrupt};
use pa_vfs::{ByteSource, ErrorKind, LocalFs, Vfs, VfsError, VfsPath};

type AllocContext = unsafe extern "C" fn() -> *mut c_void;
type OpenInput =
    unsafe extern "C" fn(*mut *mut c_void, *const c_char, *const c_void, *mut c_void) -> c_int;
type FindStreamInfo = unsafe extern "C" fn(*mut c_void, *mut c_void) -> c_int;
type CloseInput = unsafe extern "C" fn(*mut *mut c_void);
type ReadFrame = unsafe extern "C" fn(*mut c_void, *mut c_void) -> c_int;
type SeekFrame = unsafe extern "C" fn(*mut c_void, c_int, i64, c_int) -> c_int;
type PacketAlloc = unsafe extern "C" fn() -> *mut c_void;
type PacketFree = unsafe extern "C" fn(*mut *mut c_void);
type PacketUnref = unsafe extern "C" fn(*mut c_void);

struct Av {
    api: AvioApi,
    alloc_context: AllocContext,
    open_input: OpenInput,
    find_stream_info: FindStreamInfo,
    close_input: CloseInput,
    read_frame: ReadFrame,
    seek_frame: SeekFrame,
    packet_alloc: PacketAlloc,
    packet_free: PacketFree,
    packet_unref: PacketUnref,
}

fn load() -> Option<Av> {
    let dir = lib_dir()?;
    let util = Lib::open(&dir, "avutil");
    let codec = Lib::open(&dir, "avcodec");
    let format = Lib::open(&dir, "avformat");
    unsafe {
        Some(Av {
            api: AvioApi {
                av_malloc: util.sym("av_malloc"),
                av_free: util.sym("av_free"),
                avio_alloc_context: format.sym("avio_alloc_context"),
                avio_context_free: format.sym("avio_context_free"),
            },
            alloc_context: format.sym("avformat_alloc_context"),
            open_input: format.sym("avformat_open_input"),
            find_stream_info: format.sym("avformat_find_stream_info"),
            close_input: format.sym("avformat_close_input"),
            read_frame: format.sym("av_read_frame"),
            seek_frame: format.sym("av_seek_frame"),
            packet_alloc: codec.sym("av_packet_alloc"),
            packet_free: codec.sym("av_packet_free"),
            packet_unref: codec.sym("av_packet_unref"),
        })
    }
}

// Public, ABI-stable field offsets of FFmpeg structs on 64-bit targets.
const PTR: usize = std::mem::size_of::<*const ()>();
/// `AVFormatContext`: av_class, iformat, oformat, priv_data, pb, ctx_flags, nb_streams.
const FMT_IFORMAT: usize = PTR;
const FMT_PB: usize = 4 * PTR;
const FMT_NB_STREAMS: usize = 5 * PTR + 4;
/// `AVFormatContext.interrupt_callback` is not at a short fixed offset, so
/// the test checks the interrupt callback directly instead.
/// `AVPacket`: buf, pts, dts, data, size.
const PKT_PTS: usize = 8;
const PKT_DATA: usize = 24;
const PKT_SIZE: usize = 32;

unsafe fn field<T: Copy>(base: *mut c_void, offset: usize) -> T {
    unsafe { base.cast::<u8>().add(offset).cast::<T>().read() }
}

/// Opens `source` with libavformat through our AVIOContext.
unsafe fn open(av: &Av, io: &AvioContext) -> Result<*mut c_void, c_int> {
    unsafe {
        let mut fmt = (av.alloc_context)();
        assert!(!fmt.is_null());
        fmt.cast::<u8>().add(FMT_PB).cast::<*mut c_void>().write(io.as_ptr());
        // avformat_open_input sets AVFMT_FLAG_CUSTOM_IO itself when pb is set,
        // and frees the context on failure.
        match (av.open_input)(&mut fmt, null(), null(), null_mut()) {
            0 => Ok(fmt),
            err => Err(err),
        }
    }
}

#[test]
fn libavformat_demuxes_mkv_from_a_vfs_source() {
    let Some(av) = load() else { return };
    let dir = TempDir::new();
    let blocks = 30;
    std::fs::write(dir.path().join("movie.mkv"), mkv(blocks)).unwrap();
    let vfs = LocalFs::new(dir.path()).unwrap();
    let file: Arc<dyn ByteSource> = Arc::from(vfs.open(&VfsPath::parse("/movie.mkv").unwrap()).unwrap());
    // Short reads, as from SMB/WebDAV.
    let source = Arc::new(Choppy::new(file, 3000));

    let io = unsafe { AvioContext::new(av.api, source.clone(), 16 * 1024) }.unwrap();
    let mut fmt = unsafe { open(&av, &io) }.expect("avformat_open_input");
    unsafe {
        let iformat: *mut c_void = field(fmt, FMT_IFORMAT);
        let name = CStr::from_ptr(field::<*const c_char>(iformat, 0));
        assert_eq!(name.to_str().unwrap(), "matroska,webm");
        assert!((av.find_stream_info)(fmt, null_mut()) >= 0);
        assert_eq!(field::<u32>(fmt, FMT_NB_STREAMS), 1);

        let mut pkt = (av.packet_alloc)();
        let mut count = 0;
        while (av.read_frame)(fmt, pkt) >= 0 {
            assert_eq!(field::<c_int>(pkt, PKT_SIZE) as usize, BLOCK_BYTES);
            let data: *const u8 = field(pkt, PKT_DATA);
            assert_eq!(*data, sample_byte(count, 0), "packet {count}");
            assert_eq!(*data.add(BLOCK_BYTES - 1), sample_byte(count, BLOCK_BYTES - 1));
            assert_eq!(field::<i64>(pkt, PKT_PTS), count as i64 * 100);
            (av.packet_unref)(pkt);
            count += 1;
        }
        assert_eq!(count, blocks);
        assert_eq!(source.max_end.load(Ordering::Relaxed), source.size());

        // Seek back to 1.2 s (AV_TIME_BASE units) and read on from there.
        assert!((av.seek_frame)(fmt, -1, 1_200_000, 1 /* AVSEEK_FLAG_BACKWARD */) >= 0);
        assert!((av.read_frame)(fmt, pkt) >= 0);
        let pts = field::<i64>(pkt, PKT_PTS);
        assert!((1000..=1200).contains(&pts), "pts after seek: {pts}");
        let data: *const u8 = field(pkt, PKT_DATA);
        assert_eq!(*data, sample_byte(pts as usize / 100, 0));
        (av.packet_unref)(pkt);
        (av.packet_free)(&mut pkt);

        (av.close_input)(&mut fmt);
    }
    assert!(source.calls.load(Ordering::Relaxed) > 0);
    assert!(io.source().take_error().is_none());
    drop(io);
}

#[derive(Debug)]
struct Broken;

impl ByteSource for Broken {
    fn size(&self) -> u64 {
        1 << 20
    }
    fn read_at(&self, _: u64, _: &mut [u8]) -> pa_vfs::Result<usize> {
        Err(VfsError::new(ErrorKind::AuthenticationFailed, "share rejected the login"))
    }
}

#[test]
fn source_errors_reach_the_caller_and_cancel_interrupts() {
    let Some(av) = load() else { return };
    let io = unsafe { AvioContext::new(av.api, Arc::new(Broken), 4096) }.unwrap();
    let err = unsafe { open(&av, &io) }.unwrap_err();
    assert!(err < 0);
    assert_eq!(io.source().take_error().unwrap().kind(), ErrorKind::AuthenticationFailed);

    assert_eq!(unsafe { interrupt(io.opaque()) }, 0);
    io.source().cancel();
    assert_eq!(unsafe { interrupt(io.opaque()) }, 1);
}
