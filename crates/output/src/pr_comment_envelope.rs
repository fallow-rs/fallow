//! Typed sticky PR comment envelope.

use serde::{Deserialize, Serialize};

/// Rendered PR comment body plus posting signals for provider adapters.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrCommentEnvelope {
    /// Identity token embedded in the body's HTML marker; posting adapters use
    /// it to find and update the existing sticky comment.
    pub marker_id: String,
    /// Full rendered comment body, marker included.
    pub body: String,
    /// True when the run produced no review-visible findings and its gate
    /// passed; adapters skip creating a first comment only for clean runs.
    pub is_clean: bool,
    /// Link to hosted report output, when available.
    pub details_url: Option<String>,
    /// One-line status label for check or status APIs, when computed.
    pub check_summary: Option<String>,
    /// How many findings the body shows versus how many the run produced.
    pub truncation: PrCommentTruncation,
}

impl PrCommentEnvelope {
    /// Full rendered comment body, marker included.
    #[must_use]
    pub fn body(&self) -> &str {
        &self.body
    }
}

/// Finding-count truncation metadata for a rendered PR comment.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrCommentTruncation {
    /// True when the body shows fewer findings than the run produced.
    pub truncated: bool,
    /// Findings rendered in the body.
    pub shown_findings: usize,
    /// Findings the run produced.
    pub total_findings: usize,
}
