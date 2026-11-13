// SPDX-License-Identifier: MPL-2.0
//! Injects synthetic dynamic HDR metadata into a raw HEVC (Annex B) stream.
//!
//! ```text
//! pa-hdrmeta dovi <5|8.1|8.4> <in.hevc> <out.hevc>
//! pa-hdrmeta hdr10plus <in.hevc> <out.hevc>
//! ```
//!
//! * `dovi` appends one Dolby Vision RPU (NAL type 62) to every access unit,
//!   generated with `dolby_vision::rpu::generate` (same code as
//!   `dovi_tool generate`). Container signalling (`dvcC`/`dvvC`, the Matroska
//!   BlockAdditionMapping) is written later by FFmpeg when it re-muxes the
//!   stream, see `generate.py`.
//! * `hdr10plus` inserts one ST 2094-40 prefix SEI (ITU-T T.35) in front of the
//!   first slice of every access unit.
//!
//! The picture itself is not changed: for profile 5 the base layer is plain
//! BT.2020 PQ, so players without RPU reshaping show "wrong" colours. That is
//! exactly what the sample is for.

use std::{env, fs, process::ExitCode};

use dolby_vision::rpu::{
    extension_metadata::blocks::{ExtMetadataBlock, ExtMetadataBlockLevel1},
    generate::{GenerateConfig, GenerateProfile, VideoShot},
};
use hdr10plus::{
    hevc::encode_hdr10plus_nal,
    metadata::{BezierCurve, DistributionMaxRgb, Hdr10PlusMetadata},
};

const NAL_VPS: u8 = 32;
const NAL_SPS: u8 = 33;
const NAL_PPS: u8 = 34;
const NAL_AUD: u8 = 35;
const NAL_SEI_PREFIX: u8 = 39;
const START_CODE: &[u8] = &[0, 0, 0, 1];

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    let result = match args.iter().map(String::as_str).collect::<Vec<_>>()[..] {
        ["dovi", profile, input, output] => run(input, output, Mode::Dovi(profile.to_owned())),
        ["hdr10plus", input, output] => run(input, output, Mode::Hdr10Plus),
        _ => Err(
            "usage: pa-hdrmeta dovi <5|8.1|8.4> <in.hevc> <out.hevc>\n       \
                  pa-hdrmeta hdr10plus <in.hevc> <out.hevc>"
                .into(),
        ),
    };
    match result {
        Ok(frames) => {
            eprintln!("pa-hdrmeta: {frames} access units processed");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("pa-hdrmeta: {e}");
            ExitCode::FAILURE
        }
    }
}

enum Mode {
    Dovi(String),
    Hdr10Plus,
}

type Res<T> = Result<T, Box<dyn std::error::Error>>;

fn run(input: &str, output: &str, mode: Mode) -> Res<usize> {
    let data = fs::read(input)?;
    let nals = split_annex_b(&data);
    let aus = group_access_units(&nals);
    if aus.is_empty() {
        return Err("no HEVC access units found".into());
    }

    let mut out = Vec::with_capacity(data.len() + aus.len() * 256);
    match mode {
        Mode::Dovi(profile) => {
            let rpus = dovi_rpus(&profile, aus.len())?;
            for (au, rpu) in aus.iter().zip(rpus) {
                for nal in au {
                    push_nal(&mut out, nal);
                }
                push_nal(&mut out, &rpu); // RPU goes last in the access unit
            }
        }
        Mode::Hdr10Plus => {
            let sei = encode_hdr10plus_nal(&hdr10plus_metadata(), true)?;
            for au in &aus {
                let mut inserted = false;
                for nal in au {
                    if !inserted && is_vcl(nal) {
                        push_nal(&mut out, &sei);
                        inserted = true;
                    }
                    push_nal(&mut out, nal);
                }
            }
        }
    }
    fs::write(output, out)?;
    Ok(aus.len())
}

fn dovi_rpus(profile: &str, frames: usize) -> Res<Vec<Vec<u8>>> {
    let profile = match profile {
        "5" => GenerateProfile::Profile5,
        "8.1" => GenerateProfile::Profile81,
        "8.4" => GenerateProfile::Profile84,
        other => return Err(format!("unsupported DV profile {other} (use 5, 8.1 or 8.4)").into()),
    };
    // One shot, constant L1 (min/max/avg PQ) — enough for parsers and players
    // to recognise and apply the RPU.
    let l1 = ExtMetadataBlock::Level1(ExtMetadataBlockLevel1::from_stats_cm_version(
        0,
        2081,
        1229,
        dolby_vision::rpu::vdr_dm_data::CmVersion::V40,
    ));
    let config = GenerateConfig {
        profile,
        length: frames,
        shots: vec![VideoShot {
            start: 0,
            duration: frames,
            metadata_blocks: vec![l1],
            ..Default::default()
        }],
        ..Default::default()
    };
    let mut rpus = config.generate_rpu_list()?;
    Ok(GenerateConfig::encode_rpus(&mut rpus))
}

fn hdr10plus_metadata() -> Hdr10PlusMetadata {
    let percentages = [1u8, 5, 10, 25, 50, 75, 90, 95, 99];
    let percentiles = [0u32, 1000, 2000, 4000, 8000, 16000, 32000, 64000, 90000];
    Hdr10PlusMetadata {
        profile: "B".into(),
        itu_t_t35_country_code: 0xB5,
        itu_t_t35_terminal_provider_code: 0x003C,
        itu_t_t35_terminal_provider_oriented_code: 0x0001,
        application_identifier: 4,
        application_version: 1,
        num_windows: 1,
        targeted_system_display_maximum_luminance: 400,
        maxscl: [90000, 90000, 90000],
        average_maxrgb: 10000,
        num_distribution_maxrgb_percentiles: 9,
        distribution_maxrgb: percentages
            .iter()
            .zip(percentiles)
            .map(|(&percentage, percentile)| DistributionMaxRgb {
                percentage,
                percentile,
            })
            .collect(),
        tone_mapping_flag: true,
        bezier_curve: Some(BezierCurve {
            knee_point_x: 0,
            knee_point_y: 0,
            num_bezier_curve_anchors: 9,
            bezier_curve_anchors: vec![102, 205, 307, 410, 512, 614, 717, 819, 922],
        }),
        ..Default::default()
    }
}

/// Splits an Annex B byte stream into NAL units (without start codes).
fn split_annex_b(data: &[u8]) -> Vec<&[u8]> {
    let mut starts = Vec::new(); // (start_code_pos, payload_pos)
    let mut i = 0;
    while i + 3 <= data.len() {
        if data[i] == 0 && data[i + 1] == 0 && data[i + 2] == 1 {
            let sc = if i > 0 && data[i - 1] == 0 { i - 1 } else { i };
            starts.push((sc, i + 3));
            i += 3;
        } else {
            i += 1;
        }
    }
    let mut nals = Vec::with_capacity(starts.len());
    for (k, &(_, payload)) in starts.iter().enumerate() {
        let end = starts.get(k + 1).map_or(data.len(), |&(sc, _)| sc);
        let mut nal = &data[payload..end];
        while let [rest @ .., 0] = nal {
            nal = rest; // trailing_zero_8bits
        }
        if !nal.is_empty() {
            nals.push(nal);
        }
    }
    nals
}

fn nal_type(nal: &[u8]) -> u8 {
    (nal[0] >> 1) & 0x3F
}

fn is_vcl(nal: &[u8]) -> bool {
    nal_type(nal) < 32
}

fn first_slice_in_pic(nal: &[u8]) -> bool {
    is_vcl(nal) && nal.len() > 2 && nal[2] & 0x80 != 0
}

/// Groups NAL units into access units (ITU-T H.265 §7.4.2.4.4, simplified for
/// single-layer encoder output).
fn group_access_units<'a>(nals: &[&'a [u8]]) -> Vec<Vec<&'a [u8]>> {
    let mut aus: Vec<Vec<&[u8]>> = Vec::new();
    let mut current: Vec<&[u8]> = Vec::new();
    let mut seen_vcl = false;
    for &nal in nals {
        let t = nal_type(nal);
        let starts_au = matches!(t, NAL_AUD | NAL_VPS | NAL_SPS | NAL_PPS | NAL_SEI_PREFIX)
            || (41..=44).contains(&t)
            || (48..=55).contains(&t)
            || first_slice_in_pic(nal);
        if seen_vcl && starts_au {
            aus.push(std::mem::take(&mut current));
            seen_vcl = false;
        }
        seen_vcl |= is_vcl(nal);
        current.push(nal);
    }
    if seen_vcl {
        aus.push(current);
    }
    aus
}

fn push_nal(out: &mut Vec<u8>, nal: &[u8]) {
    out.extend_from_slice(START_CODE);
    out.extend_from_slice(nal);
}
