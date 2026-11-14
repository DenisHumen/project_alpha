// SPDX-License-Identifier: MPL-2.0
//! Raw C ABI of the FFmpeg functions and struct prefixes the prober uses.
//!
//! Only the leading fields of each public struct are mirrored, up to the
//! last one we read. FFmpeg adds public fields only at the end of a struct
//! within a major version, and the prefixes below are identical in
//! libavformat 61 (FFmpeg 7.1) and 63 (FFmpeg 9.0, our `tools/ffmpeg`
//! build); the offsets were taken from `offsetof` against both header sets
//! and are pinned by the tests at the bottom. [`crate::Ffmpeg`] refuses
//! other major versions ([`SUPPORTED_AVFORMAT`]).
//!
//! Codec IDs are *not* stable across majors (e.g. `AV_CODEC_ID_HEVC` is 173
//! in 7.1 and 172 in 9.0), so the prober compares codec names from
//! `avcodec_get_name`, never numeric IDs.

#![allow(non_camel_case_types, non_snake_case, missing_docs)]

use std::ffi::{c_char, c_int, c_uint, c_void};
use std::ops::RangeInclusive;

/// libavformat majors whose struct prefixes match this file.
pub const SUPPORTED_AVFORMAT: RangeInclusive<u32> = 61..=63;
/// libavcodec majors, paired with [`SUPPORTED_AVFORMAT`].
pub const SUPPORTED_AVCODEC: RangeInclusive<u32> = 61..=63;
/// libavutil majors (FFmpeg 7.x has 59, 8.x 60, 9.x 61).
pub const SUPPORTED_AVUTIL: RangeInclusive<u32> = 59..=61;

/// `AV_NOPTS_VALUE`.
pub const AV_NOPTS_VALUE: i64 = i64::MIN;
/// `AV_TIME_BASE`: microseconds.
pub const AV_TIME_BASE: i64 = 1_000_000;

/// `enum AVMediaType`.
pub const AVMEDIA_TYPE_VIDEO: c_int = 0;
pub const AVMEDIA_TYPE_AUDIO: c_int = 1;
pub const AVMEDIA_TYPE_DATA: c_int = 2;
pub const AVMEDIA_TYPE_SUBTITLE: c_int = 3;
pub const AVMEDIA_TYPE_ATTACHMENT: c_int = 4;

/// `AV_DISPOSITION_*`.
pub const AV_DISPOSITION_DEFAULT: c_int = 1 << 0;
pub const AV_DISPOSITION_DUB: c_int = 1 << 1;
pub const AV_DISPOSITION_ORIGINAL: c_int = 1 << 2;
pub const AV_DISPOSITION_COMMENT: c_int = 1 << 3;
pub const AV_DISPOSITION_FORCED: c_int = 1 << 6;
pub const AV_DISPOSITION_HEARING_IMPAIRED: c_int = 1 << 7;
pub const AV_DISPOSITION_VISUAL_IMPAIRED: c_int = 1 << 8;
pub const AV_DISPOSITION_ATTACHED_PIC: c_int = 1 << 10;

/// `enum AVPacketSideDataType` values we look for.
pub const AV_PKT_DATA_MASTERING_DISPLAY_METADATA: c_int = 20;
pub const AV_PKT_DATA_CONTENT_LIGHT_LEVEL: c_int = 22;
pub const AV_PKT_DATA_DOVI_CONF: c_int = 29;

/// `enum AVColorTransferCharacteristic` / `AVColorPrimaries` values.
pub const AVCOL_TRC_SMPTE2084: c_int = 16;
pub const AVCOL_TRC_ARIB_STD_B67: c_int = 18;
pub const AVCOL_PRI_BT2020: c_int = 9;

/// `enum AVFieldOrder`: 0 unknown, 1 progressive, 2..=5 interlaced.
pub const AV_FIELD_UNKNOWN: c_int = 0;
pub const AV_FIELD_PROGRESSIVE: c_int = 1;

/// `AV_PROFILE_UNKNOWN`.
pub const AV_PROFILE_UNKNOWN: c_int = -99;

/// `enum AVDurationEstimationMethod`.
pub const AVFMT_DURATION_FROM_PTS: c_int = 0;
pub const AVFMT_DURATION_FROM_STREAM: c_int = 1;
pub const AVFMT_DURATION_FROM_BITRATE: c_int = 2;

/// `AV_CODEC_PROP_*` of `AVCodecDescriptor.props`.
pub const AV_CODEC_PROP_BITMAP_SUB: c_int = 1 << 16;
pub const AV_CODEC_PROP_TEXT_SUB: c_int = 1 << 17;

/// `AV_PROFILE_*` values that mark object audio (the same number in
/// different codecs).
pub const AV_PROFILE_EAC3_DDP_ATMOS: c_int = 30;
pub const AV_PROFILE_TRUEHD_ATMOS: c_int = 30;
pub const AV_PROFILE_DTS_HD_MA_X: c_int = 61;
pub const AV_PROFILE_DTS_HD_MA_X_IMAX: c_int = 62;

/// `AVERROR_EOF`.
pub const AVERROR_EOF: c_int = pa_player_io::ffmpeg::AVERROR_EOF;

/// `AVRational`.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AVRational {
    pub num: c_int,
    pub den: c_int,
}

impl AVRational {
    /// The value as `f64`, `None` for 0/x, x/0 and negative values.
    #[must_use]
    pub fn to_f64(self) -> Option<f64> {
        (self.num > 0 && self.den > 0).then(|| f64::from(self.num) / f64::from(self.den))
    }
}

/// `AVDictionaryEntry`.
#[repr(C)]
pub struct AVDictionaryEntry {
    pub key: *const c_char,
    pub value: *const c_char,
}

/// Opaque `AVDictionary`.
#[repr(C)]
pub struct AVDictionary {
    _private: [u8; 0],
}

/// `AVInputFormat` prefix.
#[repr(C)]
pub struct AVInputFormat {
    pub name: *const c_char,
    pub long_name: *const c_char,
}

/// `AVIOInterruptCB`.
#[repr(C)]
pub struct AVIOInterruptCB {
    pub callback: Option<pa_player_io::ffmpeg::InterruptFn>,
    pub opaque: *mut c_void,
}

/// `AVFormatContext` prefix, up to `duration_estimation_method`.
#[repr(C)]
pub struct AVFormatContext {
    pub av_class: *const c_void,
    pub iformat: *const AVInputFormat,
    pub oformat: *const c_void,
    pub priv_data: *mut c_void,
    pub pb: *mut c_void,
    pub ctx_flags: c_int,
    pub nb_streams: c_uint,
    pub streams: *mut *mut AVStream,
    pub nb_stream_groups: c_uint,
    pub stream_groups: *mut *mut c_void,
    pub nb_chapters: c_uint,
    pub chapters: *mut *mut AVChapter,
    pub url: *mut c_char,
    pub start_time: i64,
    pub duration: i64,
    pub bit_rate: i64,
    pub packet_size: c_uint,
    pub max_delay: c_int,
    pub flags: c_int,
    pub probesize: i64,
    pub max_analyze_duration: i64,
    pub key: *const u8,
    pub keylen: c_int,
    pub nb_programs: c_uint,
    pub programs: *mut *mut c_void,
    pub video_codec_id: c_int,
    pub audio_codec_id: c_int,
    pub subtitle_codec_id: c_int,
    pub data_codec_id: c_int,
    pub metadata: *mut AVDictionary,
    pub start_time_realtime: i64,
    pub fps_probe_size: c_int,
    pub error_recognition: c_int,
    pub interrupt_callback: AVIOInterruptCB,
    pub debug: c_int,
    pub max_streams: c_int,
    pub max_index_size: c_uint,
    pub max_picture_buffer: c_uint,
    pub max_interleave_delta: i64,
    pub max_ts_probe: c_int,
    pub max_chunk_duration: c_int,
    pub max_chunk_size: c_int,
    pub max_probe_packets: c_int,
    pub strict_std_compliance: c_int,
    pub event_flags: c_int,
    pub avoid_negative_ts: c_int,
    pub audio_preload: c_int,
    pub use_wallclock_as_timestamps: c_int,
    pub skip_estimate_duration_from_pts: c_int,
    pub avio_flags: c_int,
    pub duration_estimation_method: c_int,
}

/// `AVStream` prefix, up to `avg_frame_rate`.
#[repr(C)]
pub struct AVStream {
    pub av_class: *const c_void,
    pub index: c_int,
    pub id: c_int,
    pub codecpar: *mut AVCodecParameters,
    pub priv_data: *mut c_void,
    pub time_base: AVRational,
    pub start_time: i64,
    pub duration: i64,
    pub nb_frames: i64,
    pub disposition: c_int,
    pub discard: c_int,
    pub sample_aspect_ratio: AVRational,
    pub metadata: *mut AVDictionary,
    pub avg_frame_rate: AVRational,
}

/// `AVChannelLayout.u`.
#[repr(C)]
#[derive(Clone, Copy)]
pub union AVChannelLayoutU {
    pub mask: u64,
    pub map: *mut c_void,
}

/// `AVChannelLayout`.
#[repr(C)]
pub struct AVChannelLayout {
    pub order: c_int,
    pub nb_channels: c_int,
    pub u: AVChannelLayoutU,
    pub opaque: *mut c_void,
}

/// `AVPacketSideData`.
#[repr(C)]
pub struct AVPacketSideData {
    pub data: *mut u8,
    pub size: usize,
    pub type_: c_int,
}

/// `AVCodecParameters` prefix, up to `sample_rate`.
#[repr(C)]
pub struct AVCodecParameters {
    pub codec_type: c_int,
    pub codec_id: c_int,
    pub codec_tag: u32,
    pub extradata: *mut u8,
    pub extradata_size: c_int,
    pub coded_side_data: *mut AVPacketSideData,
    pub nb_coded_side_data: c_int,
    pub format: c_int,
    pub bit_rate: i64,
    pub bits_per_coded_sample: c_int,
    pub bits_per_raw_sample: c_int,
    pub profile: c_int,
    pub level: c_int,
    pub width: c_int,
    pub height: c_int,
    pub sample_aspect_ratio: AVRational,
    pub framerate: AVRational,
    pub field_order: c_int,
    pub color_range: c_int,
    pub color_primaries: c_int,
    pub color_trc: c_int,
    pub color_space: c_int,
    pub chroma_location: c_int,
    pub video_delay: c_int,
    pub ch_layout: AVChannelLayout,
    pub sample_rate: c_int,
}

/// `AVChapter`.
#[repr(C)]
pub struct AVChapter {
    pub id: i64,
    pub time_base: AVRational,
    pub start: i64,
    pub end: i64,
    pub metadata: *mut AVDictionary,
}

/// `AVPacket` prefix, up to `flags`.
#[repr(C)]
pub struct AVPacket {
    pub buf: *mut c_void,
    pub pts: i64,
    pub dts: i64,
    pub data: *mut u8,
    pub size: c_int,
    pub stream_index: c_int,
    pub flags: c_int,
}

/// `AVCodecDescriptor` prefix.
#[repr(C)]
pub struct AVCodecDescriptor {
    pub id: c_int,
    pub type_: c_int,
    pub name: *const c_char,
    pub long_name: *const c_char,
    pub props: c_int,
}

/// `AVComponentDescriptor`.
#[repr(C)]
pub struct AVComponentDescriptor {
    pub plane: c_int,
    pub step: c_int,
    pub offset: c_int,
    pub shift: c_int,
    pub depth: c_int,
}

/// `AVPixFmtDescriptor` prefix.
#[repr(C)]
pub struct AVPixFmtDescriptor {
    pub name: *const c_char,
    pub nb_components: u8,
    pub log2_chroma_w: u8,
    pub log2_chroma_h: u8,
    pub flags: u64,
    pub comp: [AVComponentDescriptor; 4],
}

/// `AVDOVIDecoderConfigurationRecord` (all `uint8_t`).
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct AVDOVIDecoderConfigurationRecord {
    pub dv_version_major: u8,
    pub dv_version_minor: u8,
    pub dv_profile: u8,
    pub dv_level: u8,
    pub rpu_present_flag: u8,
    pub el_present_flag: u8,
    pub bl_present_flag: u8,
    pub dv_bl_signal_compatibility_id: u8,
}

type Dict = *mut AVDictionary;

/// Every FFmpeg function the prober calls, resolved by name at load time.
#[derive(Clone, Copy)]
pub struct Functions {
    // libavutil
    pub avutil_version: unsafe extern "C" fn() -> c_uint,
    pub av_malloc: unsafe extern "C" fn(usize) -> *mut c_void,
    pub av_free: unsafe extern "C" fn(*mut c_void),
    pub av_strerror: unsafe extern "C" fn(c_int, *mut c_char, usize) -> c_int,
    pub av_dict_set: unsafe extern "C" fn(*mut Dict, *const c_char, *const c_char, c_int) -> c_int,
    pub av_dict_free: unsafe extern "C" fn(*mut Dict),
    pub av_dict_iterate:
        unsafe extern "C" fn(*const AVDictionary, *const AVDictionaryEntry) -> *const AVDictionaryEntry,
    pub av_get_pix_fmt_name: unsafe extern "C" fn(c_int) -> *const c_char,
    pub av_pix_fmt_desc_get: unsafe extern "C" fn(c_int) -> *const AVPixFmtDescriptor,
    pub av_color_primaries_name: unsafe extern "C" fn(c_int) -> *const c_char,
    pub av_color_transfer_name: unsafe extern "C" fn(c_int) -> *const c_char,
    pub av_color_space_name: unsafe extern "C" fn(c_int) -> *const c_char,
    pub av_color_range_name: unsafe extern "C" fn(c_int) -> *const c_char,
    pub av_channel_layout_describe:
        unsafe extern "C" fn(*const AVChannelLayout, *mut c_char, usize) -> c_int,
    // libavcodec
    pub avcodec_version: unsafe extern "C" fn() -> c_uint,
    pub avcodec_get_name: unsafe extern "C" fn(c_int) -> *const c_char,
    pub avcodec_profile_name: unsafe extern "C" fn(c_int, c_int) -> *const c_char,
    pub avcodec_descriptor_get: unsafe extern "C" fn(c_int) -> *const AVCodecDescriptor,
    pub av_packet_alloc: unsafe extern "C" fn() -> *mut AVPacket,
    pub av_packet_free: unsafe extern "C" fn(*mut *mut AVPacket),
    pub av_packet_unref: unsafe extern "C" fn(*mut AVPacket),
    // libavformat
    pub avformat_version: unsafe extern "C" fn() -> c_uint,
    pub avformat_alloc_context: unsafe extern "C" fn() -> *mut AVFormatContext,
    pub avformat_open_input: unsafe extern "C" fn(
        *mut *mut AVFormatContext,
        *const c_char,
        *const c_void,
        *mut Dict,
    ) -> c_int,
    pub avformat_find_stream_info: unsafe extern "C" fn(*mut AVFormatContext, *mut Dict) -> c_int,
    pub avformat_close_input: unsafe extern "C" fn(*mut *mut AVFormatContext),
    pub av_read_frame: unsafe extern "C" fn(*mut AVFormatContext, *mut AVPacket) -> c_int,
    pub avio_alloc_context: unsafe extern "C" fn(
        *mut u8,
        c_int,
        c_int,
        *mut c_void,
        Option<pa_player_io::ffmpeg::ReadPacketFn>,
        Option<pa_player_io::ffmpeg::WritePacketFn>,
        Option<pa_player_io::ffmpeg::SeekFn>,
    ) -> *mut c_void,
    pub avio_context_free: unsafe extern "C" fn(*mut *mut c_void),
}

/// The three libraries [`Functions`] come from.
pub struct Libraries<'a> {
    pub avutil: &'a libloading::Library,
    pub avcodec: &'a libloading::Library,
    pub avformat: &'a libloading::Library,
}

impl Functions {
    /// Resolves every function.
    ///
    /// # Safety
    ///
    /// The libraries must be FFmpeg's libavutil, libavcodec and libavformat
    /// of one build (the signatures are not checked).
    pub(crate) unsafe fn load(libs: &Libraries<'_>) -> Result<Self, String> {
        macro_rules! sym {
            // SAFETY: the caller guarantees the library is the FFmpeg one,
            // whose function has the C type of the field it is assigned to.
            ($lib:ident, $name:literal) => {
                unsafe { get(libs.$lib, concat!($name, "\0")) }?
            };
        }
        Ok(Self {
            avutil_version: sym!(avutil, "avutil_version"),
            av_malloc: sym!(avutil, "av_malloc"),
            av_free: sym!(avutil, "av_free"),
            av_strerror: sym!(avutil, "av_strerror"),
            av_dict_set: sym!(avutil, "av_dict_set"),
            av_dict_free: sym!(avutil, "av_dict_free"),
            av_dict_iterate: sym!(avutil, "av_dict_iterate"),
            av_get_pix_fmt_name: sym!(avutil, "av_get_pix_fmt_name"),
            av_pix_fmt_desc_get: sym!(avutil, "av_pix_fmt_desc_get"),
            av_color_primaries_name: sym!(avutil, "av_color_primaries_name"),
            av_color_transfer_name: sym!(avutil, "av_color_transfer_name"),
            av_color_space_name: sym!(avutil, "av_color_space_name"),
            av_color_range_name: sym!(avutil, "av_color_range_name"),
            av_channel_layout_describe: sym!(avutil, "av_channel_layout_describe"),
            avcodec_version: sym!(avcodec, "avcodec_version"),
            avcodec_get_name: sym!(avcodec, "avcodec_get_name"),
            avcodec_profile_name: sym!(avcodec, "avcodec_profile_name"),
            avcodec_descriptor_get: sym!(avcodec, "avcodec_descriptor_get"),
            av_packet_alloc: sym!(avcodec, "av_packet_alloc"),
            av_packet_free: sym!(avcodec, "av_packet_free"),
            av_packet_unref: sym!(avcodec, "av_packet_unref"),
            avformat_version: sym!(avformat, "avformat_version"),
            avformat_alloc_context: sym!(avformat, "avformat_alloc_context"),
            avformat_open_input: sym!(avformat, "avformat_open_input"),
            avformat_find_stream_info: sym!(avformat, "avformat_find_stream_info"),
            avformat_close_input: sym!(avformat, "avformat_close_input"),
            av_read_frame: sym!(avformat, "av_read_frame"),
            avio_alloc_context: sym!(avformat, "avio_alloc_context"),
            avio_context_free: sym!(avformat, "avio_context_free"),
        })
    }

    /// The functions `pa-player-io` needs for the custom `AVIOContext`.
    #[must_use]
    pub fn avio_api(&self) -> pa_player_io::ffmpeg::AvioApi {
        pa_player_io::ffmpeg::AvioApi {
            av_malloc: self.av_malloc,
            av_free: self.av_free,
            avio_alloc_context: self.avio_alloc_context,
            avio_context_free: self.avio_context_free,
        }
    }
}

/// Resolves one symbol as a value of type `T` (a function pointer).
///
/// # Safety
///
/// The symbol must have type `T`.
unsafe fn get<T: Copy>(lib: &libloading::Library, name: &str) -> Result<T, String> {
    // SAFETY: forwarded to the caller.
    match unsafe { lib.get::<T>(name.as_bytes()) } {
        Ok(symbol) => Ok(*symbol),
        Err(e) => Err(format!("{}: {e}", name.trim_end_matches('\0'))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::mem::{offset_of, size_of};

    /// Offsets from `offsetof` against the FFmpeg 7.1 (libavformat 61) and
    /// 9.0 (libavformat 63) headers, x86_64. A mismatch means the mirror
    /// above no longer matches the C structs.
    #[test]
    #[cfg(target_pointer_width = "64")]
    fn offsets_match_ffmpeg_headers() {
        assert_eq!(offset_of!(AVFormatContext, iformat), 8);
        assert_eq!(offset_of!(AVFormatContext, pb), 32);
        assert_eq!(offset_of!(AVFormatContext, nb_streams), 44);
        assert_eq!(offset_of!(AVFormatContext, streams), 48);
        assert_eq!(offset_of!(AVFormatContext, nb_chapters), 72);
        assert_eq!(offset_of!(AVFormatContext, chapters), 80);
        assert_eq!(offset_of!(AVFormatContext, start_time), 96);
        assert_eq!(offset_of!(AVFormatContext, duration), 104);
        assert_eq!(offset_of!(AVFormatContext, bit_rate), 112);
        assert_eq!(offset_of!(AVFormatContext, flags), 128);
        assert_eq!(offset_of!(AVFormatContext, probesize), 136);
        assert_eq!(offset_of!(AVFormatContext, metadata), 192);
        assert_eq!(offset_of!(AVFormatContext, interrupt_callback), 216);
        assert_eq!(offset_of!(AVFormatContext, max_streams), 236);
        assert_eq!(offset_of!(AVFormatContext, duration_estimation_method), 300);

        assert_eq!(offset_of!(AVStream, index), 8);
        assert_eq!(offset_of!(AVStream, codecpar), 16);
        assert_eq!(offset_of!(AVStream, time_base), 32);
        assert_eq!(offset_of!(AVStream, duration), 48);
        assert_eq!(offset_of!(AVStream, disposition), 64);
        assert_eq!(offset_of!(AVStream, metadata), 80);
        assert_eq!(offset_of!(AVStream, avg_frame_rate), 88);

        assert_eq!(offset_of!(AVCodecParameters, codec_tag), 8);
        assert_eq!(offset_of!(AVCodecParameters, extradata), 16);
        assert_eq!(offset_of!(AVCodecParameters, coded_side_data), 32);
        assert_eq!(offset_of!(AVCodecParameters, nb_coded_side_data), 40);
        assert_eq!(offset_of!(AVCodecParameters, format), 44);
        assert_eq!(offset_of!(AVCodecParameters, bit_rate), 48);
        assert_eq!(offset_of!(AVCodecParameters, bits_per_raw_sample), 60);
        assert_eq!(offset_of!(AVCodecParameters, profile), 64);
        assert_eq!(offset_of!(AVCodecParameters, width), 72);
        assert_eq!(offset_of!(AVCodecParameters, sample_aspect_ratio), 80);
        assert_eq!(offset_of!(AVCodecParameters, framerate), 88);
        assert_eq!(offset_of!(AVCodecParameters, field_order), 96);
        assert_eq!(offset_of!(AVCodecParameters, color_primaries), 104);
        assert_eq!(offset_of!(AVCodecParameters, color_trc), 108);
        assert_eq!(offset_of!(AVCodecParameters, color_space), 112);
        assert_eq!(offset_of!(AVCodecParameters, ch_layout), 128);
        assert_eq!(offset_of!(AVCodecParameters, sample_rate), 152);
        assert_eq!(size_of::<AVChannelLayout>(), 24);

        assert_eq!(offset_of!(AVChapter, time_base), 8);
        assert_eq!(offset_of!(AVChapter, start), 16);
        assert_eq!(offset_of!(AVChapter, end), 24);
        assert_eq!(offset_of!(AVChapter, metadata), 32);

        assert_eq!(offset_of!(AVPacket, data), 24);
        assert_eq!(offset_of!(AVPacket, size), 32);
        assert_eq!(offset_of!(AVPacket, stream_index), 36);

        assert_eq!(size_of::<AVPacketSideData>(), 24);
        assert_eq!(offset_of!(AVPacketSideData, type_), 16);

        assert_eq!(offset_of!(AVPixFmtDescriptor, flags), 16);
        assert_eq!(offset_of!(AVPixFmtDescriptor, comp), 24);
        assert_eq!(offset_of!(AVComponentDescriptor, depth), 16);
        assert_eq!(size_of::<AVComponentDescriptor>(), 20);
        assert_eq!(offset_of!(AVCodecDescriptor, props), 24);
    }

    #[test]
    fn rational() {
        assert_eq!(AVRational { num: 24, den: 1 }.to_f64(), Some(24.0));
        assert_eq!(AVRational { num: 0, den: 1 }.to_f64(), None);
        assert_eq!(AVRational { num: 1, den: 0 }.to_f64(), None);
    }
}
