//! The language-neutral report contract of the fallow command-line tools.
//!
//! This crate holds the types that every fallow report shares: the JSON error
//! document and the verdict of a gate. A tool that emits a fallow report
//! depends on this crate, so two tools cannot write a different shape for the
//! same member.
//!
//! The crate depends on no other fallow crate and on no JavaScript type. The
//! `schema` feature derives `schemars::JsonSchema` for each type.
//!
//! The crate version follows the fallow release. The wire version is
//! [`REPORT_PROTOCOL_VERSION`].
#![warn(missing_docs)]
#![cfg_attr(
    test,
    allow(
        clippy::expect_used,
        reason = "tests use expect to keep serialization assertions concise"
    )
)]

mod error_envelope;
pub mod exit_code;
mod gate;

pub use error_envelope::ErrorOutput;
pub use gate::{GateFile, GateFileReason, GateOutcome, GateStatus};

/// The version of the wire contract that this crate describes.
///
/// A change that removes a member or changes the type of a member increments
/// this value. An additive optional member does not change it.
pub const REPORT_PROTOCOL_VERSION: u32 = 1;
