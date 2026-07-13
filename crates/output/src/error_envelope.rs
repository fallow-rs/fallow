//! The structured error envelope emitted on stdout for `--format json`.

use serde::Serialize;

/// Structured JSON error emitted on stdout when `--format json` is active and a
/// command fails. It carries no `kind` discriminator: it is distinguished from
/// the kind-tagged success envelopes by the required `error: true` field, and is
/// a document-root branch alongside `FallowOutput` and `CodeClimateOutput` in
/// `docs/output-schema.json`. Agents that pass `--format json` and observe a
/// non-zero exit code parse this shape from stdout.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ErrorOutput {
    /// Always `true`. The discriminator that separates an error document from a
    /// success envelope (which instead carries a `kind`).
    pub error: bool,
    /// Human-readable error message.
    pub message: String,
    /// The process exit code the CLI returns alongside this document.
    pub exit_code: u8,
}

impl ErrorOutput {
    /// Build an error envelope for the given message and exit code.
    #[must_use]
    pub fn new(message: impl Into<String>, exit_code: u8) -> Self {
        Self {
            error: true,
            message: message.into(),
            exit_code,
        }
    }
}
