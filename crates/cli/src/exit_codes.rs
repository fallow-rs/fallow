//! Shared non-default process exit codes for public CLI workflows.
//!
//! Keep command-specific producers and the machine-readable capability
//! manifest on these constants so agents never receive a stale copied ladder.

pub const RESOURCE_UNAVAILABLE_EXIT_CODE: u8 = 3;
pub const RUNTIME_COVERAGE_SIDECAR_EXIT_CODE: u8 = 4;
pub const RUNTIME_COVERAGE_INPUT_EXIT_CODE: u8 = 5;
pub const RUNTIME_COVERAGE_INTERNAL_EXIT_CODE: u8 = 6;
pub const NETWORK_EXIT_CODE: u8 = 7;
pub const SECURITY_GATE_EXIT_CODE: u8 = 8;
pub const COVERAGE_UPLOAD_VALIDATION_EXIT_CODE: u8 = 10;
pub const COVERAGE_UPLOAD_PAYLOAD_TOO_LARGE_EXIT_CODE: u8 = 11;
pub const COVERAGE_UPLOAD_AUTH_REJECTED_EXIT_CODE: u8 = 12;
pub const COVERAGE_UPLOAD_SERVER_ERROR_EXIT_CODE: u8 = 13;
