//! Typed sticky PR comment envelope.

use serde::{Deserialize, Serialize};

/// Rendered PR comment body plus posting signals for provider adapters.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrCommentEnvelope {
    pub marker_id: String,
    pub body: String,
    pub is_clean: bool,
    pub details_url: Option<String>,
    pub check_summary: Option<String>,
    pub truncation: PrCommentTruncation,
}

impl PrCommentEnvelope {
    #[must_use]
    pub fn body(&self) -> &str {
        &self.body
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrCommentTruncation {
    pub truncated: bool,
    pub shown_findings: usize,
    pub total_findings: usize,
}
