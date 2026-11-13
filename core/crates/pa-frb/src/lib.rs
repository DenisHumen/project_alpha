// SPDX-License-Identifier: MPL-2.0
//! flutter_rust_bridge facade of the project_alpha core (ADR-0001).
//!
//! Dart reaches the core through the package `clients/flutter/packages/pa_core`.
//! The public API lives in [`api`]: every domain is its own module
//! `src/api/<domain>.rs`, adding a domain means one `pub mod` line in
//! `src/api/mod.rs`. Swift and Kotlin use `pa-ffi` (UniFFI) instead; both
//! facades call the same core crates, so the answers match.
//!
//! `src/frb_generated.rs` is produced by `flutter_rust_bridge_codegen generate`
//! (`just core frb`); do not edit it by hand.

pub mod api;
// Generated and formatted by the codegen; `cargo fmt` would reorder imports.
#[rustfmt::skip]
mod frb_generated;
