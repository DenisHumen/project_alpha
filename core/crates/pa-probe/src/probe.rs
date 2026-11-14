// SPDX-License-Identifier: MPL-2.0
//! The probe itself: libavformat over a windowed [`ByteSource`].

use std::collections::BTreeMap;
use std::ffi::{CStr, CString, c_char, c_int};
use std::ptr::{null, null_mut};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use pa_player_io::ffmpeg::AvioContext;
use pa_vfs::{ByteSource, Vfs, VfsPath};

use crate::bitstream::{self, Hints, NalFraming};
use crate::error::{ProbeError, ProbeErrorKind};
use crate::library::Ffmpeg;
use crate::model::{
    Attachment, AudioStream, Chapter, DolbyVision, DurationSource, HdrFormat, MediaInfo,
    ObjectAudio, StreamCommon, SubtitleKind, SubtitleStream, VideoStream,
};
use crate::sys::{self, AVCodecParameters, AVDictionary, AVFormatContext, AVStream, Functions};
use crate::window::{CANCELLED, WindowedSource};

/// Default bytes read from the start of a file.
pub const DEFAULT_HEAD_BYTES: u64 = 8 << 20;
/// Default bytes readable at the end of a file (MP4 `moov` at the end,
/// MPEG-TS last timestamps).
pub const DEFAULT_TAIL_BYTES: u64 = 4 << 20;
/// Smallest accepted head window.
pub const MIN_HEAD_BYTES: u64 = 1 << 20;

/// AVIO buffer size; reads from the source are at most this large.
const AVIO_BUFFER: usize = 64 * 1024;

/// How a probe reads.
#[derive(Debug, Clone)]
pub struct ProbeOptions {
    /// Bytes readable from the start of the file (at least
    /// [`MIN_HEAD_BYTES`]). libavformat's `probesize` is set to this.
    pub head_bytes: u64,
    /// Bytes readable at the end of the file; 0 disables the tail.
    pub tail_bytes: u64,
    /// libavformat `analyzeduration`: media time analysed to find stream
    /// parameters.
    pub analyze_duration: Duration,
    /// Video packets inspected for HDR10+ and Dolby Vision RPUs (HEVC and
    /// AV1 only); 0 disables the scan.
    pub hdr_scan_packets: usize,
    /// Set to `true` from another thread to abort the probe.
    pub cancel: Option<Arc<AtomicBool>>,
}

impl Default for ProbeOptions {
    fn default() -> Self {
        Self {
            head_bytes: DEFAULT_HEAD_BYTES,
            tail_bytes: DEFAULT_TAIL_BYTES,
            analyze_duration: Duration::from_secs(5),
            hdr_scan_packets: 12,
            cancel: None,
        }
    }
}

impl ProbeOptions {
    fn is_cancelled(&self) -> bool {
        self.cancel
            .as_ref()
            .is_some_and(|c| c.load(Ordering::Relaxed))
    }
}

/// Probes `source` with the process-wide FFmpeg ([`Ffmpeg::shared`]).
///
/// # Errors
///
/// See [`ProbeErrorKind`].
pub fn probe(source: Arc<dyn ByteSource>, opts: &ProbeOptions) -> Result<MediaInfo, ProbeError> {
    Ffmpeg::shared()?.probe(source, opts)
}

/// Opens `path` on `vfs` and probes it with the process-wide FFmpeg.
///
/// # Errors
///
/// [`ProbeErrorKind::Source`] if the file cannot be opened, otherwise as
/// [`probe`].
pub fn probe_path(
    vfs: &dyn Vfs,
    path: &VfsPath,
    opts: &ProbeOptions,
) -> Result<MediaInfo, ProbeError> {
    let ffmpeg = Ffmpeg::shared()?;
    let source = vfs.open(path).map_err(|e| ProbeError::from_vfs(&e))?;
    ffmpeg.probe(Arc::from(source), opts)
}

/// Closes the format context on every exit path.
struct Input<'a> {
    f: &'a Functions,
    fmt: *mut AVFormatContext,
}

impl Drop for Input<'_> {
    fn drop(&mut self) {
        // SAFETY: `fmt` was opened by avformat_open_input and is closed once.
        unsafe { (self.f.avformat_close_input)(&raw mut self.fmt) };
    }
}

impl Ffmpeg {
    /// Analyses `source`: container, duration, streams, chapters, tags and
    /// attachments, reading at most `head_bytes + tail_bytes` from it.
    /// Blocking; call it from a worker thread. Separate calls may run in
    /// parallel.
    ///
    /// # Errors
    ///
    /// See [`ProbeErrorKind`].
    pub fn probe(
        &self,
        source: Arc<dyn ByteSource>,
        opts: &ProbeOptions,
    ) -> Result<MediaInfo, ProbeError> {
        let f = &self.functions;
        let size = source.size();
        if size == 0 {
            return Err(ProbeError::new(ProbeErrorKind::InvalidData, "empty file"));
        }
        let head = opts.head_bytes.max(MIN_HEAD_BYTES);
        let window = Arc::new(WindowedSource::new(
            source,
            head,
            opts.tail_bytes,
            opts.cancel.clone(),
        ));
        // SAFETY: the AVIO functions come from the same loaded FFmpeg.
        let io = unsafe { AvioContext::new(f.avio_api(), window.clone(), AVIO_BUFFER) }
            .map_err(|e| ProbeError::new(ProbeErrorKind::LibraryUnavailable, e.to_string()))?;

        // SAFETY: plain allocation.
        let mut fmt = unsafe { (f.avformat_alloc_context)() };
        if fmt.is_null() {
            return Err(ProbeError::new(
                ProbeErrorKind::LibraryUnavailable,
                "avformat_alloc_context failed",
            ));
        }
        let ret = {
            let mut dict: *mut AVDictionary = null_mut();
            let analyze = opts.analyze_duration.as_micros().min(i64::MAX as u128);
            // SAFETY: `fmt` is a fresh context; `pb` is our live AVIOContext
            // (avformat_open_input sets AVFMT_FLAG_CUSTOM_IO, so it is not
            // freed by FFmpeg). On failure FFmpeg frees `fmt` and nulls it.
            unsafe {
                (*fmt).pb = io.as_ptr();
                dict_set(f, &mut dict, c"probesize", &head.min(i64::MAX as u64).to_string());
                dict_set(f, &mut dict, c"analyzeduration", &analyze.to_string());
                let ret = (f.avformat_open_input)(&raw mut fmt, null(), null(), &raw mut dict);
                (f.av_dict_free)(&raw mut dict);
                ret
            }
        };
        if ret < 0 {
            return Err(self.open_error(ret, &io, opts));
        }
        // Declared after `io`, so it is dropped (closed) before it.
        let input = Input { f, fmt };

        // SAFETY: `fmt` is open. A negative result still leaves whatever
        // was found (e.g. a stream without parameters past the window).
        let _ = unsafe { (f.avformat_find_stream_info)(input.fmt, null_mut()) };
        if opts.is_cancelled() {
            return Err(cancelled());
        }

        // SAFETY: `fmt` is open and analysed.
        let mut info = unsafe { self.collect(input.fmt, size) };
        if opts.hdr_scan_packets > 0 {
            // SAFETY: as above.
            unsafe { self.scan_hdr(input.fmt, &mut info, opts.hdr_scan_packets) };
        }
        for v in &mut info.video {
            v.hdr = hdr_formats(v);
        }
        drop(input);
        if opts.is_cancelled() {
            return Err(cancelled());
        }
        info.bytes_read = window.bytes_read.load(Ordering::Relaxed);
        info.window_truncated = window.truncated.load(Ordering::Relaxed);
        Ok(info)
    }

    fn open_error(&self, ret: c_int, io: &AvioContext, opts: &ProbeOptions) -> ProbeError {
        if opts.is_cancelled() {
            return cancelled();
        }
        if let Some(e) = io.source().take_error() {
            if e.to_string().contains(CANCELLED) {
                return cancelled();
            }
            return ProbeError::from_vfs(&e);
        }
        let mut buf = [0 as c_char; 128];
        // SAFETY: `buf` holds 128 bytes; av_strerror NUL-terminates.
        let text = unsafe {
            (self.functions.av_strerror)(ret, buf.as_mut_ptr(), buf.len());
            CStr::from_ptr(buf.as_ptr()).to_string_lossy().into_owned()
        };
        ProbeError::new(
            ProbeErrorKind::InvalidData,
            format!("libavformat cannot open the file: {text}"),
        )
    }

    /// # Safety
    ///
    /// `fmt` is an open, analysed format context of this FFmpeg.
    unsafe fn collect(&self, fmt: *mut AVFormatContext, size: u64) -> MediaInfo {
        let f = &self.functions;
        // SAFETY: guaranteed by the caller; FFmpeg keeps the arrays valid
        // until the context is closed.
        unsafe {
            let ctx = &*fmt;
            let mut info = MediaInfo {
                size_bytes: size,
                ..MediaInfo::default()
            };
            if let Some(iformat) = ctx.iformat.as_ref() {
                info.container = cstr(iformat.name).unwrap_or_default();
                info.container_long_name = cstr(iformat.long_name).unwrap_or_default();
            }
            info.bit_rate = u64::try_from(ctx.bit_rate).ok().filter(|b| *b > 0);
            info.tags = dict_map(f, ctx.metadata);

            for i in 0..ctx.nb_streams as usize {
                let st = *ctx.streams.add(i);
                if st.is_null() || (*st).codecpar.is_null() {
                    continue;
                }
                let st = &*st;
                let par = &*st.codecpar;
                match par.codec_type {
                    sys::AVMEDIA_TYPE_VIDEO => info.video.push(self.video(st, par)),
                    sys::AVMEDIA_TYPE_AUDIO => info.audio.push(self.audio(st, par)),
                    sys::AVMEDIA_TYPE_SUBTITLE => info.subtitles.push(self.subtitle(st, par)),
                    sys::AVMEDIA_TYPE_ATTACHMENT => info.attachments.push(self.attachment(st, par)),
                    _ => {}
                }
            }

            for i in 0..ctx.nb_chapters as usize {
                let ch = *ctx.chapters.add(i);
                if let Some(ch) = ch.as_ref() {
                    info.chapters.push(Chapter {
                        start_ms: to_ms(ch.start, ch.time_base).unwrap_or(0),
                        end_ms: to_ms(ch.end, ch.time_base).unwrap_or(0),
                        title: dict_get(f, ch.metadata, "title"),
                    });
                }
            }
            info.chapters.sort_by_key(|c| c.start_ms);

            if ctx.duration != sys::AV_NOPTS_VALUE && ctx.duration > 0 {
                info.duration_ms = u64::try_from(ctx.duration / 1000).ok();
                info.duration_source = match ctx.duration_estimation_method {
                    sys::AVFMT_DURATION_FROM_PTS => Some(DurationSource::Pts),
                    sys::AVFMT_DURATION_FROM_STREAM => Some(DurationSource::Stream),
                    sys::AVFMT_DURATION_FROM_BITRATE => Some(DurationSource::Bitrate),
                    _ => None,
                };
            } else {
                // No container duration: the longest stream, if any declares one.
                info.duration_ms = (0..ctx.nb_streams as usize)
                    .filter_map(|i| (*ctx.streams.add(i)).as_ref())
                    .filter_map(|st| to_ms(st.duration, st.time_base))
                    .filter(|ms| *ms > 0)
                    .max();
                info.duration_source = info.duration_ms.map(|_| DurationSource::Stream);
            }
            info
        }
    }

    /// # Safety
    ///
    /// `st` and `par` are a live stream and its parameters.
    unsafe fn common(&self, st: &AVStream, par: &AVCodecParameters) -> StreamCommon {
        let f = &self.functions;
        let d = st.disposition;
        // SAFETY: guaranteed by the caller; the name functions return static
        // strings or null.
        unsafe {
            let profile = if par.profile == sys::AV_PROFILE_UNKNOWN {
                None
            } else {
                cstr((f.avcodec_profile_name)(par.codec_id, par.profile))
            };
            StreamCommon {
                index: u32::try_from(st.index).unwrap_or(0),
                codec: cstr((f.avcodec_get_name)(par.codec_id)).unwrap_or_default(),
                profile,
                language: dict_get(f, st.metadata, "language")
                    .filter(|l| !l.eq_ignore_ascii_case("und")),
                title: dict_get(f, st.metadata, "title"),
                default: d & sys::AV_DISPOSITION_DEFAULT != 0,
                forced: d & sys::AV_DISPOSITION_FORCED != 0,
                comment: d & sys::AV_DISPOSITION_COMMENT != 0,
                hearing_impaired: d & sys::AV_DISPOSITION_HEARING_IMPAIRED != 0,
                visual_impaired: d & sys::AV_DISPOSITION_VISUAL_IMPAIRED != 0,
                original: d & sys::AV_DISPOSITION_ORIGINAL != 0,
                dub: d & sys::AV_DISPOSITION_DUB != 0,
            }
        }
    }

    unsafe fn video(&self, st: &AVStream, par: &AVCodecParameters) -> VideoStream {
        let f = &self.functions;
        // SAFETY: `st`/`par` are live (caller); side data arrays are owned
        // by `par`; name getters return static strings or null.
        unsafe {
            let mut v = VideoStream {
                common: self.common(st, par),
                codec_tag: fourcc(par.codec_tag),
                width: u32::try_from(par.width).unwrap_or(0),
                height: u32::try_from(par.height).unwrap_or(0),
                cover_art: st.disposition & sys::AV_DISPOSITION_ATTACHED_PIC != 0,
                ..VideoStream::default()
            };
            let sar = if st.sample_aspect_ratio.to_f64().is_some() {
                st.sample_aspect_ratio
            } else {
                par.sample_aspect_ratio
            };
            if sar.to_f64().is_some() && sar.num != sar.den {
                v.sample_aspect_ratio = Some((sar.num as u32, sar.den as u32));
            }
            let rate = if st.avg_frame_rate.to_f64().is_some() {
                st.avg_frame_rate
            } else {
                par.framerate
            };
            if rate.to_f64().is_some() {
                v.frame_rate = Some((rate.num as u32, rate.den as u32));
            }
            if par.format >= 0 {
                v.pix_fmt = cstr((f.av_get_pix_fmt_name)(par.format));
            }
            v.bit_depth = if par.bits_per_raw_sample > 0 {
                u8::try_from(par.bits_per_raw_sample).ok()
            } else if par.format >= 0 {
                (f.av_pix_fmt_desc_get)(par.format)
                    .as_ref()
                    .filter(|d| d.nb_components > 0)
                    .and_then(|d| u8::try_from(d.comp[0].depth).ok())
            } else {
                None
            };
            v.interlaced = match par.field_order {
                sys::AV_FIELD_UNKNOWN => None,
                sys::AV_FIELD_PROGRESSIVE => Some(false),
                _ => Some(true),
            };
            v.color_primaries = known(cstr((f.av_color_primaries_name)(par.color_primaries)));
            v.color_transfer = known(cstr((f.av_color_transfer_name)(par.color_trc)));
            v.color_space = known(cstr((f.av_color_space_name)(par.color_space)));
            v.color_range = known(cstr((f.av_color_range_name)(par.color_range)));

            let n = usize::try_from(par.nb_coded_side_data).unwrap_or(0);
            for i in 0..n {
                let sd = &*par.coded_side_data.add(i);
                match sd.type_ {
                    sys::AV_PKT_DATA_DOVI_CONF
                        if sd.size >= size_of::<sys::AVDOVIDecoderConfigurationRecord>() =>
                    {
                        let rec = sd
                            .data
                            .cast::<sys::AVDOVIDecoderConfigurationRecord>()
                            .read_unaligned();
                        v.dolby_vision = Some(DolbyVision {
                            profile: Some(rec.dv_profile),
                            level: Some(rec.dv_level),
                            compatibility_id: Some(rec.dv_bl_signal_compatibility_id),
                            rpu: rec.rpu_present_flag != 0,
                            enhancement_layer: rec.el_present_flag != 0,
                            base_layer: rec.bl_present_flag != 0,
                        });
                    }
                    sys::AV_PKT_DATA_MASTERING_DISPLAY_METADATA => v.mastering_display = true,
                    sys::AV_PKT_DATA_CONTENT_LIGHT_LEVEL => v.content_light_level = true,
                    _ => {}
                }
            }
            v
        }
    }

    unsafe fn audio(&self, st: &AVStream, par: &AVCodecParameters) -> AudioStream {
        let f = &self.functions;
        // SAFETY: `st`/`par` are live (caller); the layout description is
        // written into our buffer and NUL-terminated.
        unsafe {
            let common = self.common(st, par);
            let mut buf = [0 as c_char; 64];
            let layout = if par.ch_layout.nb_channels > 0
                && (f.av_channel_layout_describe)(&raw const par.ch_layout, buf.as_mut_ptr(), buf.len()) > 0
            {
                cstr(buf.as_ptr())
            } else {
                None
            };
            let bits = if par.bits_per_raw_sample > 0 {
                par.bits_per_raw_sample
            } else if common.codec.starts_with("pcm_") {
                par.bits_per_coded_sample
            } else {
                0
            };
            AudioStream {
                object_audio: object_audio(&common.codec, par.profile),
                channels: u32::try_from(par.ch_layout.nb_channels).unwrap_or(0),
                channel_layout: layout,
                sample_rate: u32::try_from(par.sample_rate).ok().filter(|r| *r > 0),
                bit_depth: u8::try_from(bits).ok().filter(|b| *b > 0),
                bit_rate: u64::try_from(par.bit_rate).ok().filter(|b| *b > 0),
                common,
            }
        }
    }

    unsafe fn subtitle(&self, st: &AVStream, par: &AVCodecParameters) -> SubtitleStream {
        // SAFETY: `st`/`par` are live (caller); descriptors are static.
        unsafe {
            let desc = (self.functions.avcodec_descriptor_get)(par.codec_id).as_ref();
            let kind = match desc.map(|d| d.props) {
                Some(p) if p & sys::AV_CODEC_PROP_TEXT_SUB != 0 => SubtitleKind::Text,
                Some(p) if p & sys::AV_CODEC_PROP_BITMAP_SUB != 0 => SubtitleKind::Bitmap,
                _ => SubtitleKind::Unknown,
            };
            SubtitleStream {
                common: self.common(st, par),
                kind,
            }
        }
    }

    unsafe fn attachment(&self, st: &AVStream, par: &AVCodecParameters) -> Attachment {
        let f = &self.functions;
        // SAFETY: `st`/`par` are live (caller).
        unsafe {
            Attachment {
                index: u32::try_from(st.index).unwrap_or(0),
                filename: dict_get(f, st.metadata, "filename"),
                mime_type: dict_get(f, st.metadata, "mimetype"),
                codec: cstr((f.avcodec_get_name)(par.codec_id)).unwrap_or_default(),
                size_bytes: u64::try_from(par.extradata_size).ok().filter(|s| *s > 0),
            }
        }
    }

    /// Reads the first video packets of the main HEVC/AV1 stream and looks
    /// for HDR10+ and Dolby Vision RPUs.
    ///
    /// # Safety
    ///
    /// `fmt` is an open, analysed format context of this FFmpeg.
    unsafe fn scan_hdr(&self, fmt: *mut AVFormatContext, info: &mut MediaInfo, max_packets: usize) {
        let f = &self.functions;
        let Some(pos) = info.video.iter().position(|v| {
            !v.cover_art && (v.common.codec == "hevc" || v.common.codec == "av1")
        }) else {
            return;
        };
        let index = info.video[pos].common.index;
        // SAFETY: guaranteed by the caller; packet data is valid until unref.
        let hints = unsafe {
            let ctx = &*fmt;
            let par = &*(*(*ctx.streams.add(index as usize))).codecpar;
            let av1 = info.video[pos].common.codec == "av1";
            let framing = if par.extradata.is_null() || par.extradata_size <= 0 {
                NalFraming::AnnexB
            } else {
                NalFraming::from_hevc_extradata(std::slice::from_raw_parts(
                    par.extradata,
                    par.extradata_size as usize,
                ))
            };
            let mut pkt = (f.av_packet_alloc)();
            if pkt.is_null() {
                return;
            }
            let mut hints = Hints::default();
            let (mut seen, mut total) = (0, 0);
            // Other streams interleave with video; bound the total too.
            while seen < max_packets && total < max_packets * 16 {
                if (f.av_read_frame)(fmt, pkt) < 0 {
                    break;
                }
                total += 1;
                let p = &*pkt;
                if p.stream_index as u32 == index && !p.data.is_null() && p.size > 0 {
                    seen += 1;
                    let data = std::slice::from_raw_parts(p.data, p.size as usize);
                    let found = if av1 {
                        bitstream::scan_av1(data)
                    } else {
                        bitstream::scan_hevc(data, framing)
                    };
                    hints.hdr10plus |= found.hdr10plus;
                    hints.dovi_rpu |= found.dovi_rpu;
                }
                (f.av_packet_unref)(pkt);
                if hints.hdr10plus && hints.dovi_rpu {
                    break;
                }
            }
            (f.av_packet_free)(&raw mut pkt);
            hints
        };
        let v = &mut info.video[pos];
        if hints.hdr10plus {
            v.hdr.push(HdrFormat::Hdr10Plus);
        }
        if hints.dovi_rpu && v.dolby_vision.is_none() {
            v.dolby_vision = Some(DolbyVision {
                rpu: true,
                ..DolbyVision::default()
            });
        }
    }
}

/// HDR formats of `v`, strongest first. [`Ffmpeg::scan_hdr`] leaves
/// [`HdrFormat::Hdr10Plus`] in `v.hdr` when it found it.
fn hdr_formats(v: &VideoStream) -> Vec<HdrFormat> {
    if v.cover_art {
        return Vec::new();
    }
    let mut out = Vec::new();
    if v.dolby_vision.is_some() {
        out.push(HdrFormat::DolbyVision);
    }
    if v.hdr.contains(&HdrFormat::Hdr10Plus) {
        out.push(HdrFormat::Hdr10Plus);
    }
    match v.color_transfer.as_deref() {
        Some("smpte2084") => out.push(HdrFormat::Hdr10),
        Some("arib-std-b67") => out.push(HdrFormat::Hlg),
        _ => {}
    }
    out
}

/// Object audio from FFmpeg's codec profile.
fn object_audio(codec: &str, profile: c_int) -> Option<ObjectAudio> {
    match (codec, profile) {
        ("eac3", sys::AV_PROFILE_EAC3_DDP_ATMOS) | ("truehd", sys::AV_PROFILE_TRUEHD_ATMOS) => {
            Some(ObjectAudio::Atmos)
        }
        ("dts", sys::AV_PROFILE_DTS_HD_MA_X | sys::AV_PROFILE_DTS_HD_MA_X_IMAX) => {
            Some(ObjectAudio::DtsX)
        }
        _ => None,
    }
}

fn cancelled() -> ProbeError {
    ProbeError::new(ProbeErrorKind::Cancelled, CANCELLED)
}

/// `ts` in `tb` units as milliseconds; `None` for `AV_NOPTS_VALUE`,
/// negative values and invalid time bases.
fn to_ms(ts: i64, tb: sys::AVRational) -> Option<u64> {
    if ts == sys::AV_NOPTS_VALUE || ts < 0 || tb.num <= 0 || tb.den <= 0 {
        return None;
    }
    let ms = i128::from(ts) * i128::from(tb.num) * 1000 / i128::from(tb.den);
    u64::try_from(ms).ok()
}

/// Codec tag as text if all four bytes are printable ASCII.
fn fourcc(tag: u32) -> Option<String> {
    let bytes = tag.to_le_bytes();
    (tag != 0 && bytes.iter().all(|b| b.is_ascii_alphanumeric() || *b == b' ' || *b == b'.'))
        .then(|| String::from_utf8_lossy(&bytes).trim_end().to_owned())
}

/// Drops FFmpeg's placeholder names for unset colour properties.
fn known(name: Option<String>) -> Option<String> {
    name.filter(|n| !matches!(n.as_str(), "unknown" | "unspecified" | "reserved"))
}

/// A C string as `String` (lossy UTF-8); `None` for null or empty.
///
/// # Safety
///
/// `p` is null or a valid NUL-terminated string.
unsafe fn cstr(p: *const c_char) -> Option<String> {
    if p.is_null() {
        return None;
    }
    // SAFETY: guaranteed by the caller.
    let s = unsafe { CStr::from_ptr(p) }.to_string_lossy();
    let s = s.trim();
    (!s.is_empty()).then(|| s.to_owned())
}

/// # Safety
///
/// `dict` is null or a live `AVDictionary` of this FFmpeg.
unsafe fn dict_map(f: &Functions, dict: *const AVDictionary) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    let mut entry = null();
    // SAFETY: av_dict_iterate accepts null and returns entries owned by `dict`.
    unsafe {
        loop {
            entry = (f.av_dict_iterate)(dict, entry);
            let Some(e) = entry.as_ref() else { break };
            if let (Some(k), Some(v)) = (cstr(e.key), cstr(e.value)) {
                out.entry(k.to_lowercase()).or_insert(v);
            }
        }
    }
    out
}

/// Case-insensitive lookup, like `av_dict_get` without flags.
///
/// # Safety
///
/// As [`dict_map`].
unsafe fn dict_get(f: &Functions, dict: *const AVDictionary, key: &str) -> Option<String> {
    let mut entry = null();
    // SAFETY: as in `dict_map`.
    unsafe {
        loop {
            entry = (f.av_dict_iterate)(dict, entry);
            let e = entry.as_ref()?;
            if cstr(e.key).is_some_and(|k| k.eq_ignore_ascii_case(key)) {
                return cstr(e.value);
            }
        }
    }
}

/// # Safety
///
/// `dict` points to a null or live `AVDictionary *`.
unsafe fn dict_set(f: &Functions, dict: &mut *mut AVDictionary, key: &CStr, value: &str) {
    let Ok(value) = CString::new(value) else { return };
    // SAFETY: av_dict_set copies key and value (flags 0).
    unsafe { (f.av_dict_set)(dict, key.as_ptr(), value.as_ptr(), 0) };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hdr_classification() {
        let video = |trc: Option<&str>, dv: bool, plus: bool| VideoStream {
            color_transfer: trc.map(str::to_owned),
            dolby_vision: dv.then(DolbyVision::default),
            hdr: if plus { vec![HdrFormat::Hdr10Plus] } else { vec![] },
            ..VideoStream::default()
        };
        assert!(hdr_formats(&video(Some("bt709"), false, false)).is_empty());
        assert_eq!(hdr_formats(&video(Some("smpte2084"), false, false)), [HdrFormat::Hdr10]);
        assert_eq!(hdr_formats(&video(Some("arib-std-b67"), false, false)), [HdrFormat::Hlg]);
        assert_eq!(
            hdr_formats(&video(Some("smpte2084"), false, true)),
            [HdrFormat::Hdr10Plus, HdrFormat::Hdr10]
        );
        assert_eq!(
            hdr_formats(&video(Some("smpte2084"), true, false)),
            [HdrFormat::DolbyVision, HdrFormat::Hdr10]
        );
        assert_eq!(hdr_formats(&video(None, true, false)), [HdrFormat::DolbyVision]);
        let mut cover = video(Some("smpte2084"), false, false);
        cover.cover_art = true;
        assert!(hdr_formats(&cover).is_empty());
    }

    #[test]
    fn object_audio_from_profiles() {
        assert_eq!(object_audio("eac3", 30), Some(ObjectAudio::Atmos));
        assert_eq!(object_audio("truehd", 30), Some(ObjectAudio::Atmos));
        assert_eq!(object_audio("dts", 61), Some(ObjectAudio::DtsX));
        assert_eq!(object_audio("dts", 60), None);
        assert_eq!(object_audio("ac3", 30), None);
        assert_eq!(object_audio("eac3", sys::AV_PROFILE_UNKNOWN), None);
    }

    #[test]
    fn timestamps_to_ms() {
        let tb = |num, den| sys::AVRational { num, den };
        assert_eq!(to_ms(90_000, tb(1, 90_000)), Some(1000));
        assert_eq!(to_ms(5_000_000, tb(1, 1_000_000)), Some(5000));
        assert_eq!(to_ms(sys::AV_NOPTS_VALUE, tb(1, 1000)), None);
        assert_eq!(to_ms(-1, tb(1, 1000)), None);
        assert_eq!(to_ms(10, tb(1, 0)), None);
        assert_eq!(to_ms(i64::MAX, tb(1_000_000, 1)), None);
    }

    #[test]
    fn fourcc_and_placeholder_names() {
        assert_eq!(fourcc(u32::from_le_bytes(*b"hvc1")).as_deref(), Some("hvc1"));
        assert_eq!(fourcc(0), None);
        assert_eq!(fourcc(0x0000_001B), None);
        assert_eq!(known(Some("unknown".into())), None);
        assert_eq!(known(Some("bt709".into())).as_deref(), Some("bt709"));
    }
}
