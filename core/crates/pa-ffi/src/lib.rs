// SPDX-License-Identifier: MPL-2.0
//! UniFFI facade of the project_alpha core.
//!
//! This crate is the only place where the core API is exported to Kotlin and
//! Swift (ADR-0001). Every domain lives in its own module `src/<domain>.rs`;
//! adding a domain means adding exactly one `mod` line below
//! (docs/ROADMAP.md §0.5).
//!
//! Bindings are generated, not committed: run `just core bindings`.

uniffi::setup_scaffolding!();

mod playback;
mod runtime;
mod secrets;
mod version;

pub use playback::*;
pub use runtime::*;
pub use secrets::*;
pub use version::core_version;
