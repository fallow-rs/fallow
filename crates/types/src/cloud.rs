//! Cross-surface strings for the cloud runtime-coverage path.
//!
//! `fallow coverage analyze --cloud` and the MCP `get_cloud_runtime_context`
//! tool refuse the same call for the same reason, and an agent reads both.
//! Keeping the refusal text here makes the two surfaces say one sentence
//! instead of holding two copies that drift apart.

/// Refusal emitted when no Fallow Cloud API key is available. Names the
/// environment variable first because that is the only route the MCP tool
/// has; the flag and the command line follow for the CLI surface.
pub const CLOUD_API_KEY_MISSING_MESSAGE: &str = "Cloud runtime coverage requires an API key.\n\nSet FALLOW_API_KEY or pass --api-key:\n\n  FALLOW_API_KEY=fallow_live_... fallow coverage analyze --cloud --repo owner/repo";
