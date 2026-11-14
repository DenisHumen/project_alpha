// SPDX-License-Identifier: MPL-2.0
//! HDR metadata that containers do not always declare, found by scanning
//! the first video packets (pure Rust, no decoding): HDR10+ (SMPTE ST
//! 2094-40 in ITU-T T.35 SEI / AV1 metadata OBUs), Dolby Vision RPUs
//! without a configuration record, static HDR metadata (mastering display,
//! content light level) in SEI / metadata OBUs, and the AV1 sequence
//! header colour description.

/// ITU-T T.35 prefix of HDR10+: country 0xB5 (USA), provider 0x003C
/// (Samsung), oriented code 0x0001, application identifier 4.
const HDR10PLUS_T35: [u8; 6] = [0xB5, 0x00, 0x3C, 0x00, 0x01, 0x04];
/// ITU-T T.35 prefix of a Dolby Vision RPU in AV1: country 0xB5,
/// provider 0x003B (Dolby), oriented code 0x00000800.
const DOVI_AV1_T35: [u8; 7] = [0xB5, 0x00, 0x3B, 0x00, 0x00, 0x08, 0x00];

/// HEVC NAL unit types.
const HEVC_PREFIX_SEI: u8 = 39;
const HEVC_SUFFIX_SEI: u8 = 40;
/// `UNSPEC62`, used by Dolby Vision for RPUs.
const HEVC_DOVI_RPU: u8 = 62;
/// SEI payload types: `user_data_registered_itu_t_t35`,
/// `mastering_display_colour_volume`, `content_light_level_info`.
const SEI_T35: u32 = 4;
const SEI_MDCV: u32 = 137;
const SEI_CLL: u32 = 144;
/// AV1 OBU types and metadata types.
const AV1_OBU_SEQUENCE_HEADER: u8 = 1;
const AV1_OBU_METADATA: u8 = 5;
const AV1_METADATA_HDR_CLL: u64 = 1;
const AV1_METADATA_HDR_MDCV: u64 = 2;
const AV1_METADATA_T35: u64 = 4;

/// ISO/IEC 23091-2 colour description (the same codes as FFmpeg's enums).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ColorDescription {
    pub primaries: u8,
    pub transfer: u8,
    pub matrix: u8,
}

/// What the packets revealed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Hints {
    /// HDR10+ dynamic metadata present.
    pub hdr10plus: bool,
    /// Dolby Vision RPU present.
    pub dovi_rpu: bool,
    /// Mastering display colour volume present.
    pub mastering_display: bool,
    /// Content light level present.
    pub content_light_level: bool,
    /// AV1 sequence header colour description, if signalled.
    pub color: Option<ColorDescription>,
}

impl Hints {
    pub(crate) fn merge(&mut self, other: Self) {
        self.hdr10plus |= other.hdr10plus;
        self.dovi_rpu |= other.dovi_rpu;
        self.mastering_display |= other.mastering_display;
        self.content_light_level |= other.content_light_level;
        self.color = self.color.or(other.color);
    }
}

/// The OBUs inside AV1 codec extradata: an `av1C` record (marker bit set)
/// carries them after its 4-byte header; otherwise the data is OBUs.
pub(crate) fn av1_config_obus(extradata: &[u8]) -> &[u8] {
    match extradata {
        [first, _, _, _, obus @ ..] if first & 0x80 != 0 => obus,
        _ => extradata,
    }
}

/// How HEVC packets frame their NAL units.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NalFraming {
    /// MP4/MKV (`hvcC`): big-endian length prefix of 1–4 bytes.
    Length(usize),
    /// MPEG-TS and raw streams: `00 00 01` start codes.
    AnnexB,
}

impl NalFraming {
    /// From the codec extradata: an `hvcC` record (version byte 1) gives
    /// the length size, anything else means Annex B.
    pub(crate) fn from_hevc_extradata(extradata: &[u8]) -> Self {
        match extradata {
            [1, rest @ ..] if rest.len() >= 22 => Self::Length(usize::from(rest[20] & 3) + 1),
            _ => Self::AnnexB,
        }
    }
}

/// Scans one HEVC access unit.
pub(crate) fn scan_hevc(packet: &[u8], framing: NalFraming) -> Hints {
    let mut hints = Hints::default();
    for_each_nal(packet, framing, |nal| {
        let Some(&first) = nal.first() else { return };
        match (first >> 1) & 0x3F {
            HEVC_DOVI_RPU => hints.dovi_rpu = true,
            HEVC_PREFIX_SEI | HEVC_SUFFIX_SEI if nal.len() > 2 => {
                hints.merge(scan_sei(&unescape(&nal[2..])));
            }
            _ => {}
        }
    });
    hints
}

/// Scans one AV1 temporal unit (low-overhead OBU format) or the OBUs of
/// an `av1C` record.
pub(crate) fn scan_av1(packet: &[u8]) -> Hints {
    let mut hints = Hints::default();
    let mut rest = packet;
    while let Some((&header, tail)) = rest.split_first() {
        let obu_type = (header >> 3) & 0x0F;
        let has_extension = header & 0x04 != 0;
        let has_size = header & 0x02 != 0;
        let mut body = tail;
        if has_extension {
            let Some((_, b)) = body.split_first() else {
                break;
            };
            body = b;
        }
        let payload = if has_size {
            let Some((size, b)) = leb128(body) else { break };
            let Ok(size) = usize::try_from(size) else {
                break;
            };
            if size > b.len() {
                break;
            }
            rest = &b[size..];
            &b[..size]
        } else {
            rest = &[];
            body
        };
        match obu_type {
            AV1_OBU_SEQUENCE_HEADER if hints.color.is_none() => {
                hints.color = av1_sequence_color(payload);
            }
            AV1_OBU_METADATA => match leb128(payload) {
                Some((AV1_METADATA_T35, t35)) => {
                    hints.hdr10plus |= t35.starts_with(&HDR10PLUS_T35);
                    hints.dovi_rpu |= t35.starts_with(&DOVI_AV1_T35);
                }
                Some((AV1_METADATA_HDR_MDCV, _)) => hints.mastering_display = true,
                Some((AV1_METADATA_HDR_CLL, _)) => hints.content_light_level = true,
                _ => {}
            },
            _ => {}
        }
    }
    hints
}

/// MSB-first bit reader for the AV1 sequence header.
struct Bits<'a> {
    data: &'a [u8],
    pos: usize,
}

impl Bits<'_> {
    fn f(&mut self, n: u32) -> Option<u32> {
        let mut v = 0u32;
        for _ in 0..n {
            let byte = *self.data.get(self.pos / 8)?;
            let bit = (byte >> (7 - self.pos % 8)) & 1;
            v = (v << 1) | u32::from(bit);
            self.pos += 1;
        }
        Some(v)
    }

    fn flag(&mut self) -> Option<bool> {
        self.f(1).map(|b| b == 1)
    }

    fn uvlc(&mut self) -> Option<u32> {
        let mut zeros = 0;
        while !self.flag()? {
            zeros += 1;
            if zeros >= 32 {
                return Some(u32::MAX);
            }
        }
        Some(self.f(zeros)? + ((1u32 << zeros) - 1))
    }
}

/// `color_config()` of an AV1 `sequence_header_obu` (AV1 spec §5.5);
/// `None` if not signalled or the header is malformed.
fn av1_sequence_color(payload: &[u8]) -> Option<ColorDescription> {
    let mut b = Bits {
        data: payload,
        pos: 0,
    };
    let seq_profile = b.f(3)?;
    let _still_picture = b.flag()?;
    let reduced = b.flag()?;
    if reduced {
        b.f(5)?; // seq_level_idx[0]
    } else {
        let timing_info = b.flag()?;
        let mut decoder_model_info = false;
        let mut buffer_delay_len = 0;
        if timing_info {
            b.f(32)?; // num_units_in_display_tick
            b.f(32)?; // time_scale
            if b.flag()? {
                b.uvlc()?; // num_ticks_per_picture_minus_1
            }
            decoder_model_info = b.flag()?;
            if decoder_model_info {
                buffer_delay_len = b.f(5)? + 1;
                b.f(32)?; // num_units_in_decoding_tick
                b.f(5)?; // buffer_removal_time_length_minus_1
                b.f(5)?; // frame_presentation_time_length_minus_1
            }
        }
        let initial_display_delay = b.flag()?;
        let operating_points = b.f(5)? + 1;
        for _ in 0..operating_points {
            b.f(12)?; // operating_point_idc
            if b.f(5)? > 7 {
                b.f(1)?; // seq_tier
            }
            if decoder_model_info && b.flag()? {
                b.f(buffer_delay_len)?; // decoder_buffer_delay
                b.f(buffer_delay_len)?; // encoder_buffer_delay
                b.f(1)?; // low_delay_mode_flag
            }
            if initial_display_delay && b.flag()? {
                b.f(4)?; // initial_display_delay_minus_1
            }
        }
    }
    let width_bits = b.f(4)? + 1;
    let height_bits = b.f(4)? + 1;
    b.f(width_bits)?; // max_frame_width_minus_1
    b.f(height_bits)?; // max_frame_height_minus_1
    if !reduced && b.flag()? {
        b.f(4)?; // delta_frame_id_length_minus_2
        b.f(3)?; // additional_frame_id_length_minus_1
    }
    b.f(3)?; // use_128x128_superblock, enable_filter_intra, enable_intra_edge_filter
    if !reduced {
        b.f(4)?; // interintra, masked compound, warped motion, dual filter
        let order_hint = b.flag()?;
        if order_hint {
            b.f(2)?; // enable_jnt_comp, enable_ref_frame_mvs
        }
        // seq_choose_screen_content_tools ? SELECT (2) : seq_force_screen_content_tools
        let force_screen_content_tools = if b.flag()? { 2 } else { b.f(1)? };
        // seq_choose_integer_mv, else seq_force_integer_mv
        if force_screen_content_tools > 0 && !b.flag()? {
            b.f(1)?;
        }
        if order_hint {
            b.f(3)?; // order_hint_bits_minus_1
        }
    }
    b.f(3)?; // enable_superres, enable_cdef, enable_restoration
    // color_config()
    let high_bitdepth = b.flag()?;
    if seq_profile == 2 && high_bitdepth {
        b.f(1)?; // twelve_bit
    }
    if seq_profile != 1 {
        b.f(1)?; // mono_chrome
    }
    if !b.flag()? {
        return None;
    }
    Some(ColorDescription {
        primaries: b.f(8)? as u8,
        transfer: b.f(8)? as u8,
        matrix: b.f(8)? as u8,
    })
}

fn for_each_nal(data: &[u8], framing: NalFraming, mut f: impl FnMut(&[u8])) {
    match framing {
        NalFraming::Length(n) => {
            let mut rest = data;
            while rest.len() >= n {
                let len = rest[..n]
                    .iter()
                    .fold(0usize, |acc, b| (acc << 8) | usize::from(*b));
                rest = &rest[n..];
                if len > rest.len() {
                    break;
                }
                f(&rest[..len]);
                rest = &rest[len..];
            }
        }
        NalFraming::AnnexB => {
            let starts: Vec<usize> = data
                .windows(3)
                .enumerate()
                .filter(|(_, w)| *w == [0, 0, 1])
                .map(|(i, _)| i + 3)
                .collect();
            for (k, &start) in starts.iter().enumerate() {
                let mut end = starts.get(k + 1).map_or(data.len(), |next| next - 3);
                // A 4-byte start code leaves a zero before the next one;
                // trailing zero bytes are not part of the NAL unit.
                while end > start && data[end - 1] == 0 {
                    end -= 1;
                }
                f(&data[start..end]);
            }
        }
    }
}

/// NAL payload with emulation prevention bytes (`00 00 03`) removed.
fn unescape(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len());
    let mut zeros = 0;
    for &b in data {
        if zeros >= 2 && b == 3 {
            zeros = 0;
            continue;
        }
        zeros = if b == 0 { zeros + 1 } else { 0 };
        out.push(b);
    }
    out
}

/// Walks the SEI messages of an unescaped SEI RBSP.
fn scan_sei(rbsp: &[u8]) -> Hints {
    let mut hints = Hints::default();
    let mut rest = rbsp;
    // Stop at the RBSP trailing bits (0x80).
    while rest.len() > 1 {
        let Some((payload_type, r)) = sei_value(rest) else {
            break;
        };
        let Some((size, r)) = sei_value(r) else { break };
        let Ok(size) = usize::try_from(size) else {
            break;
        };
        if size > r.len() {
            break;
        }
        match payload_type {
            SEI_T35 => hints.hdr10plus |= r[..size].starts_with(&HDR10PLUS_T35),
            SEI_MDCV => hints.mastering_display = true,
            SEI_CLL => hints.content_light_level = true,
            _ => {}
        }
        rest = &r[size..];
    }
    hints
}

/// An SEI payload type or size: a run of 0xFF bytes plus a final byte.
fn sei_value(data: &[u8]) -> Option<(u32, &[u8])> {
    let mut value = 0u32;
    for (i, &b) in data.iter().enumerate() {
        value = value.checked_add(u32::from(b))?;
        if b != 0xFF {
            return Some((value, &data[i + 1..]));
        }
    }
    None
}

/// AV1 `leb128()`.
fn leb128(data: &[u8]) -> Option<(u64, &[u8])> {
    let mut value = 0u64;
    for (i, &b) in data.iter().enumerate().take(8) {
        value |= u64::from(b & 0x7F) << (7 * i);
        if b & 0x80 == 0 {
            return Some((value, &data[i + 1..]));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A prefix SEI NAL with one T.35 message (`payload`).
    fn sei_nal(payload: &[u8]) -> Vec<u8> {
        let mut nal = vec![HEVC_PREFIX_SEI << 1, 1, SEI_T35 as u8, payload.len() as u8];
        nal.extend_from_slice(payload);
        nal.push(0x80);
        nal
    }

    fn length_prefixed(nals: &[Vec<u8>]) -> Vec<u8> {
        nals.iter()
            .flat_map(|n| {
                let mut v = (n.len() as u32).to_be_bytes().to_vec();
                v.extend_from_slice(n);
                v
            })
            .collect()
    }

    fn hdr10plus_payload() -> Vec<u8> {
        let mut p = HDR10PLUS_T35.to_vec();
        // application_version, then a body with a 00 00 03 escape inside.
        p.extend_from_slice(&[1, 0x00, 0x00, 0x03, 0x01, 0x42]);
        p
    }

    #[test]
    fn framing_from_extradata() {
        let mut hvcc = vec![1u8; 23];
        hvcc[21] = 0xFC | 3;
        assert_eq!(
            NalFraming::from_hevc_extradata(&hvcc),
            NalFraming::Length(4)
        );
        hvcc[21] = 0xFC | 1;
        assert_eq!(
            NalFraming::from_hevc_extradata(&hvcc),
            NalFraming::Length(2)
        );
        assert_eq!(
            NalFraming::from_hevc_extradata(&[0, 0, 0, 1, 0x40]),
            NalFraming::AnnexB
        );
        assert_eq!(NalFraming::from_hevc_extradata(&[]), NalFraming::AnnexB);
    }

    #[test]
    fn hevc_hdr10plus_in_length_prefixed_sei() {
        let slice = vec![1 << 1, 1, 0xAF, 0x00];
        let pkt = length_prefixed(&[sei_nal(&hdr10plus_payload()), slice]);
        let hints = scan_hevc(&pkt, NalFraming::Length(4));
        assert!(hints.hdr10plus);
        assert!(!hints.dovi_rpu);
    }

    #[test]
    fn hevc_other_t35_is_not_hdr10plus() {
        // ATSC A/53 captions: country 0xB5, provider 0x0031.
        let pkt = length_prefixed(&[sei_nal(&[0xB5, 0x00, 0x31, 0x47, 0x41, 0x39, 0x34])]);
        assert_eq!(scan_hevc(&pkt, NalFraming::Length(4)), Hints::default());
    }

    #[test]
    fn hevc_dovi_rpu_in_annex_b() {
        let mut pkt = vec![0, 0, 0, 1, HEVC_DOVI_RPU << 1, 1, 0x19, 0x08];
        pkt.extend_from_slice(&[0, 0, 1]);
        pkt.extend_from_slice(&sei_nal(&hdr10plus_payload()));
        let hints = scan_hevc(&pkt, NalFraming::AnnexB);
        assert!(hints.dovi_rpu);
        assert!(hints.hdr10plus);
    }

    #[test]
    fn hevc_garbage_does_not_panic() {
        for framing in [
            NalFraming::Length(4),
            NalFraming::Length(1),
            NalFraming::AnnexB,
        ] {
            for len in 0..64 {
                let data: Vec<u8> = (0..len).map(|i| (i * 37 + 0xF0) as u8).collect();
                let _ = scan_hevc(&data, framing);
            }
        }
        // Huge SEI size field.
        let pkt = length_prefixed(&[vec![HEVC_PREFIX_SEI << 1, 1, 4, 0xFF, 0xFF, 0x10]]);
        assert_eq!(scan_hevc(&pkt, NalFraming::Length(4)), Hints::default());
    }

    #[test]
    fn unescape_removes_emulation_prevention() {
        assert_eq!(
            unescape(&[0, 0, 3, 1, 0, 0, 3, 0, 3]),
            vec![0, 0, 1, 0, 0, 0, 3]
        );
    }

    fn av1_metadata_obu(t35: &[u8]) -> Vec<u8> {
        let mut payload = vec![AV1_METADATA_T35 as u8];
        payload.extend_from_slice(t35);
        let mut obu = vec![(AV1_OBU_METADATA << 3) | 0x02, payload.len() as u8];
        obu.extend_from_slice(&payload);
        obu
    }

    #[test]
    fn av1_metadata_obus() {
        // Temporal delimiter, then HDR10+ metadata, then a frame OBU.
        let mut pkt = vec![(2 << 3) | 0x02, 0];
        pkt.extend_from_slice(&av1_metadata_obu(&hdr10plus_payload()));
        pkt.extend_from_slice(&[(6 << 3) | 0x02, 2, 0xAA, 0xBB]);
        let hints = scan_av1(&pkt);
        assert!(hints.hdr10plus);
        assert!(!hints.dovi_rpu);

        let mut dovi = DOVI_AV1_T35.to_vec();
        dovi.extend_from_slice(&[0x19, 0x08]);
        assert!(scan_av1(&av1_metadata_obu(&dovi)).dovi_rpu);
        assert_eq!(scan_av1(&[(5 << 3) | 0x02, 0x7F]), Hints::default());
    }

    /// `av1C` extradata of the SVT-AV1 HDR10 test sample
    /// (`tests/media/synthetic/hdr/hdr10-av1-1080p.mkv`): 4-byte header,
    /// then the sequence header OBU (1920x1080, 10 bit, BT.2020 / PQ /
    /// BT.2020 NCL).
    const AV1C_HDR10: [u8; 20] = [
        0x81, 0x08, 0x4C, 0x00, 0x0A, 0x0E, 0x00, 0x00, 0x00, 0x42, 0xAB, 0xBF, 0xC3, 0x70, 0x0B,
        0xE1, 0x42, 0x44, 0x02, 0x41,
    ];

    #[test]
    fn av1_sequence_header_from_a_real_stream() {
        let hints = scan_av1(av1_config_obus(&AV1C_HDR10));
        assert_eq!(
            hints.color,
            Some(ColorDescription {
                primaries: 9,
                transfer: 16,
                matrix: 9
            })
        );
        // Truncated header: no colour, no panic.
        for n in 0..AV1C_HDR10.len() - 6 {
            assert_eq!(av1_sequence_color(&AV1C_HDR10[6..6 + n]), None);
        }
    }

    #[test]
    fn av1_reduced_still_picture_header() {
        let bits = concat!(
            "000",      // seq_profile
            "11",       // still_picture, reduced_still_picture_header
            "00000",    // seq_level_idx
            "0000",     // frame_width_bits_minus_1
            "0000",     // frame_height_bits_minus_1
            "00",       // max_frame_width/height_minus_1
            "000",      // superblock, filter intra, intra edge
            "000",      // superres, cdef, restoration
            "00",       // high_bitdepth, mono_chrome
            "1",        // color_description_present_flag
            "00000001", // BT.709 primaries
            "00010010", // HLG transfer
            "00000001", // BT.709 matrix
        );
        let bytes: Vec<u8> = bits
            .as_bytes()
            .chunks(8)
            .map(|c| {
                let s = format!("{:0<8}", std::str::from_utf8(c).unwrap());
                u8::from_str_radix(&s, 2).unwrap()
            })
            .collect();
        assert_eq!(
            av1_sequence_color(&bytes),
            Some(ColorDescription {
                primaries: 1,
                transfer: 18,
                matrix: 1
            })
        );
    }

    #[test]
    fn hevc_static_metadata_sei() {
        let mut nal = vec![HEVC_PREFIX_SEI << 1, 1, SEI_MDCV as u8, 24];
        nal.extend_from_slice(&[0x11; 24]);
        nal.extend_from_slice(&[SEI_CLL as u8, 4, 0x03, 0xE8, 0x01, 0x90, 0x80]);
        let hints = scan_hevc(&length_prefixed(&[nal]), NalFraming::Length(4));
        assert!(hints.mastering_display && hints.content_light_level);
        assert!(!hints.hdr10plus);
    }

    #[test]
    fn leb128_values() {
        assert_eq!(leb128(&[0x05]), Some((5, &[][..])));
        assert_eq!(leb128(&[0x80, 0x01, 9]), Some((128, &[9][..])));
        assert_eq!(leb128(&[0x80]), None);
    }
}
