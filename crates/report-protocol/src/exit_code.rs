//! The process exit codes of the report contract.
//!
//! Some commands define more exit codes. These three have the same meaning in
//! every fallow tool.

/// No enforced gate failed.
pub const EXIT_OK: u8 = 0;

/// An enforced gate failed. The run found problems, the tool did not fail.
pub const EXIT_GATE_FAILED: u8 = 1;

/// The input was not valid, or the tool could not complete the run.
pub const EXIT_INVALID_INPUT: u8 = 2;
