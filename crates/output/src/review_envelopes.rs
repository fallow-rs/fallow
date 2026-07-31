//! Review integration output envelopes.

use crate::root_envelopes::{RootEnvelopeMode, attach_telemetry_meta, serialize_named_json_output};
use serde::Serialize;

/// Prefix for the exact review-scope marker appended to generated bodies.
pub const REVIEW_ID_MARKER_PREFIX: &str = "<!-- fallow-review-id: ";

const REVIEW_ID_MARKER_SUFFIX: &str = " -->";

/// Stable identifier used to isolate independent review integrations on the
/// same pull or merge request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(transparent)]
pub struct ReviewId(
    #[cfg_attr(
        feature = "schema",
        schemars(length(min = 1, max = 64), regex(pattern = r"^[A-Za-z0-9._-]+$"))
    )]
    String,
);

impl ReviewId {
    /// Parse and validate a review identifier.
    pub fn parse(value: impl Into<String>) -> Result<Self, String> {
        let value = value.into();
        if value.is_empty() || value.len() > 64 {
            return Err("review id must contain between 1 and 64 bytes".to_owned());
        }
        if !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
        {
            return Err(
                "review id may contain only ASCII letters, digits, '.', '_' and '-'".to_owned(),
            );
        }
        Ok(Self(value))
    }

    /// Return the validated identifier.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Render the exact review-scope marker line.
#[must_use]
pub fn review_id_marker(review_id: &ReviewId) -> String {
    format!(
        "{REVIEW_ID_MARKER_PREFIX}{}{REVIEW_ID_MARKER_SUFFIX}",
        review_id.as_str()
    )
}

/// Parse the sole canonical review-scope marker from a rendered body.
pub fn parse_review_id_marker(body: &str) -> Result<Option<ReviewId>, String> {
    let mut found = None;
    for line in body.lines() {
        if !line.contains("fallow-review-id") {
            continue;
        }
        let value = line
            .strip_prefix(REVIEW_ID_MARKER_PREFIX)
            .and_then(|line| line.strip_suffix(REVIEW_ID_MARKER_SUFFIX))
            .ok_or_else(|| "malformed fallow review-id marker".to_owned())?;
        let review_id = ReviewId::parse(value.to_owned())
            .map_err(|error| format!("invalid fallow review-id marker: {error}"))?;
        if found.replace(review_id).is_some() {
            return Err("duplicate fallow review-id marker".to_owned());
        }
    }
    Ok(found)
}

/// Require a body marker to match the expected review scope exactly.
pub fn validate_review_body_scope(body: &str, review_id: Option<&ReviewId>) -> Result<(), String> {
    let body_review_id = parse_review_id_marker(body)?;
    if body_review_id.as_ref() != review_id {
        return Err("review body marker does not match meta.review_id".to_owned());
    }
    Ok(())
}

/// Whether a rendered body belongs to the expected review scope.
#[must_use]
pub fn body_matches_review_id(body: &str, review_id: Option<&ReviewId>) -> bool {
    validate_review_body_scope(body, review_id).is_ok()
}

/// Envelope emitted by `fallow --format review-github` / `review-gitlab`.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(
    feature = "schema",
    schemars(title = "fallow --format review-github / review-gitlab")
)]
pub struct ReviewEnvelopeOutput {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event: Option<ReviewEnvelopeEvent>,
    pub body: String,
    #[serde(default = "ReviewEnvelopeSummary::empty_default")]
    pub summary: ReviewEnvelopeSummary,
    pub comments: Vec<ReviewComment>,
    #[serde(default = "default_marker_regex")]
    pub marker_regex: String,
    #[serde(default = "default_marker_regex_flags")]
    pub marker_regex_flags: String,
    pub meta: ReviewEnvelopeMeta,
}

/// Envelope emitted by `fallow --format review-github` / `review-gitlab`.
#[doc(hidden)]
#[derive(Debug, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "schema", schemars(rename = "ReviewEnvelopeOutput"))]
#[cfg_attr(
    feature = "schema",
    schemars(title = "fallow --format review-github / review-gitlab")
)]
pub struct ReviewEnvelopeWireOutput<'a> {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    event: Option<ReviewEnvelopeEvent>,
    body: &'a str,
    #[cfg_attr(
        feature = "schema",
        schemars(default = "ReviewEnvelopeSummary::empty_default")
    )]
    summary: &'a ReviewEnvelopeSummary,
    comments: &'a [ReviewComment],
    #[serde(default = "default_marker_regex")]
    marker_regex: &'a str,
    #[serde(default = "default_marker_regex_flags")]
    marker_regex_flags: &'a str,
    meta: ReviewEnvelopeWireMeta<'a>,
}

/// `meta` block inside [`ReviewEnvelopeOutput`].
#[derive(Debug, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "schema", schemars(rename = "ReviewEnvelopeMeta"))]
struct ReviewEnvelopeWireMeta<'a> {
    schema: ReviewEnvelopeSchema,
    provider: ReviewProvider,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    check_conclusion: Option<ReviewCheckConclusion>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    review_id: Option<&'a ReviewId>,
}

fn serialize_review_contract_json_output<T: Serialize>(
    output: T,
    kind: &'static str,
    mode: RootEnvelopeMode,
    analysis_run_id: Option<&str>,
) -> Result<serde_json::Value, serde_json::Error> {
    let mut value = serialize_named_json_output(output, kind, mode)?;
    attach_telemetry_meta(&mut value, analysis_run_id);
    Ok(value)
}

/// Serialize the review envelope contract emitted by CI review formats.
///
/// # Errors
///
/// Returns a serde error when the review envelope cannot be converted to JSON.
pub fn serialize_review_envelope_json_output(
    output: ReviewEnvelopeOutput,
    mode: RootEnvelopeMode,
    analysis_run_id: Option<&str>,
) -> Result<serde_json::Value, serde_json::Error> {
    serialize_review_contract_json_output(output, "review-envelope", mode, analysis_run_id)
}

/// Serialize a scoped review envelope through the canonical derived wire type.
pub fn serialize_scoped_review_envelope_json_output(
    output: &ReviewEnvelopeOutput,
    review_id: &ReviewId,
    mode: RootEnvelopeMode,
    analysis_run_id: Option<&str>,
) -> Result<serde_json::Value, String> {
    validate_review_envelope_scope(output, Some(review_id))?;
    let wire = ReviewEnvelopeWireOutput {
        event: output.event,
        body: &output.body,
        summary: &output.summary,
        comments: &output.comments,
        marker_regex: &output.marker_regex,
        marker_regex_flags: &output.marker_regex_flags,
        meta: ReviewEnvelopeWireMeta {
            schema: output.meta.schema,
            provider: output.meta.provider,
            check_conclusion: output.meta.check_conclusion,
            review_id: Some(review_id),
        },
    };
    serialize_review_contract_json_output(wire, "review-envelope", mode, analysis_run_id)
        .map_err(|error| error.to_string())
}

fn validate_review_envelope_scope(
    output: &ReviewEnvelopeOutput,
    review_id: Option<&ReviewId>,
) -> Result<(), String> {
    validate_review_body_scope(&output.body, review_id)?;
    for comment in &output.comments {
        let body = match comment {
            ReviewComment::GitHub(comment) => &comment.body,
            ReviewComment::GitLab(comment) => &comment.body,
        };
        validate_review_body_scope(body, review_id)?;
    }
    Ok(())
}

/// Default for [`ReviewEnvelopeOutput::marker_regex`].
#[must_use]
pub fn default_marker_regex() -> String {
    MARKER_REGEX_V2.to_owned()
}

/// Default for [`ReviewEnvelopeOutput::marker_regex_flags`].
#[must_use]
pub fn default_marker_regex_flags() -> String {
    MARKER_REGEX_FLAGS_V2.to_owned()
}

/// Canonical v2 marker-regex literal.
pub const MARKER_REGEX_V2: &str =
    r"^<!-- fallow-fingerprint:v2: ((?:[a-z]+:)?[0-9a-f]{16}) -->\s*$";

/// Canonical v2 marker-regex flags.
pub const MARKER_REGEX_FLAGS_V2: &str = "m";

/// Summary block on [`ReviewEnvelopeOutput`].
#[derive(Debug, Clone, Serialize, Default)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ReviewEnvelopeSummary {
    pub body: String,
    pub fingerprint: String,
}

impl ReviewEnvelopeSummary {
    /// Empty-default factory for [`ReviewEnvelopeOutput::summary`].
    #[must_use]
    #[allow(
        dead_code,
        reason = "referenced via serde default attr; no direct callsite until Deserialize is derived"
    )]
    pub fn empty_default() -> Self {
        Self::default()
    }
}

/// Singleton GitHub review-event marker.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub enum ReviewEnvelopeEvent {
    #[serde(rename = "COMMENT")]
    Comment,
}

/// Per-line review comment. Schema is an `anyOf` between GitHub and GitLab
/// shapes; at runtime every entry in a single envelope comes from the same
/// provider because the envelope is built from one provider's branch in
/// `crates/cli/src/report/ci/review.rs::render_review_envelope`.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(untagged)]
pub enum ReviewComment {
    GitHub(GitHubReviewComment),
    GitLab(GitLabReviewComment),
}

/// GitHub pull-request review comment.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct GitHubReviewComment {
    pub path: String,
    pub line: u32,
    pub side: GitHubReviewSide,
    pub body: String,
    pub fingerprint: String,
    #[serde(default, skip_serializing_if = "is_false")]
    pub truncated: bool,
}

/// Singleton side discriminator for [`GitHubReviewComment::side`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub enum GitHubReviewSide {
    #[serde(rename = "RIGHT")]
    Right,
}

/// GitLab merge-request discussion comment.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct GitLabReviewComment {
    pub body: String,
    pub position: GitLabReviewPosition,
    pub fingerprint: String,
    #[serde(default, skip_serializing_if = "is_false")]
    pub truncated: bool,
}

/// Helper for `skip_serializing_if = "is_false"` on `truncated` fields.
#[must_use]
#[allow(
    clippy::trivially_copy_pass_by_ref,
    reason = "serde's skip_serializing_if requires fn(&T) -> bool"
)]
pub fn is_false(value: &bool) -> bool {
    !*value
}

/// `position` block inside [`GitLabReviewComment`]. Mirrors the GitLab
/// merge-request discussion-position API.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct GitLabReviewPosition {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_sha: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_sha: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub head_sha: Option<String>,
    pub position_type: GitLabReviewPositionType,
    pub old_path: String,
    pub new_path: String,
    pub new_line: u32,
}

/// Singleton position-type discriminator for [`GitLabReviewPosition`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "lowercase")]
pub enum GitLabReviewPositionType {
    Text,
}

/// `meta` block inside [`ReviewEnvelopeOutput`].
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ReviewEnvelopeMeta {
    pub schema: ReviewEnvelopeSchema,
    pub provider: ReviewProvider,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub check_conclusion: Option<ReviewCheckConclusion>,
}

/// Schema-version discriminator for the review envelope.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub enum ReviewEnvelopeSchema {
    /// Historical first release of the review envelope format.
    #[serde(rename = "fallow-review-envelope/v1")]
    #[allow(
        dead_code,
        reason = "kept for forward-compat with v1 historical inputs once Deserialize is derived"
    )]
    V1,
    /// Issue #528 review envelope format.
    #[serde(rename = "fallow-review-envelope/v2")]
    V2,
}

/// Review-envelope provider tag.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "lowercase")]
pub enum ReviewProvider {
    /// GitHub pull-request review envelope.
    Github,
    /// GitLab merge-request discussion envelope.
    Gitlab,
}

/// `meta.check_conclusion` for the GitHub review envelope. Maps to the
/// GitHub Checks API conclusion field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "lowercase")]
pub enum ReviewCheckConclusion {
    /// No findings.
    Success,
    /// Findings but none gated as failure.
    Neutral,
    /// At least one finding gated as failure.
    Failure,
}

/// Envelope emitted by `fallow ci reconcile-review --format json`. Used by
/// CI integrations to drive comment carry-over and stale-comment cleanup
/// across PR / MR revisions.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(
    feature = "schema",
    schemars(title = "fallow ci reconcile-review --format json")
)]
pub struct ReviewReconcileOutput {
    pub schema: ReviewReconcileSchema,
    pub provider: ReviewProvider,
    pub target: Option<String>,
    pub dry_run: bool,
    pub comments: u32,
    pub current_fingerprints: u32,
    pub existing_fingerprints: u32,
    pub new_fingerprints: u32,
    pub stale_fingerprints: u32,
    pub new: Vec<String>,
    pub stale: Vec<String>,
    pub provider_warning: Option<String>,
    pub resolution_comments_posted: u32,
    pub threads_resolved: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub apply_hint: Option<String>,
    pub apply_errors: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub failed_fingerprints: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unapplied_fingerprints: Vec<String>,
}

/// Serialize the review reconcile contract.
///
/// # Errors
///
/// Returns a serde error when the review reconcile output cannot be converted
/// to JSON.
pub fn serialize_review_reconcile_json_output(
    output: ReviewReconcileOutput,
    mode: RootEnvelopeMode,
    analysis_run_id: Option<&str>,
) -> Result<serde_json::Value, serde_json::Error> {
    serialize_review_contract_json_output(output, "review-reconcile", mode, analysis_run_id)
}

/// Schema-version discriminator for the review reconcile envelope.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub enum ReviewReconcileSchema {
    /// First release of the review reconcile format.
    #[serde(rename = "fallow-review-reconcile/v1")]
    V1,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn legacy_review_envelope(body: &str) -> ReviewEnvelopeOutput {
        ReviewEnvelopeOutput {
            event: None,
            body: body.to_owned(),
            summary: ReviewEnvelopeSummary::default(),
            comments: Vec::new(),
            marker_regex: default_marker_regex(),
            marker_regex_flags: default_marker_regex_flags(),
            meta: ReviewEnvelopeMeta {
                schema: ReviewEnvelopeSchema::V2,
                provider: ReviewProvider::Github,
                check_conclusion: None,
            },
        }
    }

    #[test]
    fn review_envelope_json_output_uses_output_owned_root_contract() {
        let output = legacy_review_envelope("body");

        let value = serialize_review_envelope_json_output(
            output,
            RootEnvelopeMode::Tagged,
            Some("run-review"),
        )
        .expect("review envelope should serialize");

        assert_eq!(value["kind"], "review-envelope");
        assert_eq!(value["_meta"]["telemetry"]["analysis_run_id"], "run-review");
    }

    #[test]
    fn legacy_serializer_is_byte_shape_equal_to_the_public_dto() {
        let output = legacy_review_envelope("body");
        let mut expected =
            serde_json::to_value(&output).expect("legacy review envelope should serialize");
        crate::apply_root_kind(&mut expected, "review-envelope", RootEnvelopeMode::Tagged);
        let actual = serialize_review_envelope_json_output(output, RootEnvelopeMode::Tagged, None)
            .expect("review envelope should serialize");

        assert_eq!(
            serde_json::to_vec(&actual).expect("actual review envelope should encode"),
            serde_json::to_vec(&expected).expect("expected review envelope should encode")
        );
        assert!(actual["meta"].get("review_id").is_none());
    }

    #[test]
    fn scoped_serializer_adds_only_typed_review_id_to_legacy_shape() {
        let review_id = ReviewId::parse("frontend").expect("review id should be valid");
        let output = legacy_review_envelope("body\n<!-- fallow-review-id: frontend -->");
        let value = serialize_scoped_review_envelope_json_output(
            &output,
            &review_id,
            RootEnvelopeMode::Tagged,
            None,
        )
        .expect("scoped review envelope should serialize");

        assert_eq!(value["meta"]["review_id"], "frontend");
        assert_eq!(value["meta"]["provider"], "github");
    }

    #[cfg(feature = "schema")]
    #[test]
    fn canonical_wire_schema_has_one_optional_typed_review_id() {
        let schema = serde_json::to_value(schemars::schema_for!(ReviewEnvelopeWireOutput<'static>))
            .expect("review envelope schema should serialize");
        let meta = &schema["$defs"]["ReviewEnvelopeMeta"];
        let required = schema["required"]
            .as_array()
            .expect("review envelope schema should list required fields");

        assert!(!required.iter().any(|field| field == "summary"));
        assert!(meta["properties"]["review_id"].is_object());
        assert!(
            !meta["required"]
                .as_array()
                .is_some_and(|required| required.iter().any(|field| field == "review_id"))
        );
    }

    #[test]
    fn review_reconcile_json_output_uses_output_owned_root_contract() {
        let output = ReviewReconcileOutput {
            schema: ReviewReconcileSchema::V1,
            provider: ReviewProvider::Github,
            target: None,
            dry_run: true,
            comments: 0,
            current_fingerprints: 0,
            existing_fingerprints: 0,
            new_fingerprints: 0,
            stale_fingerprints: 0,
            new: Vec::new(),
            stale: Vec::new(),
            provider_warning: None,
            resolution_comments_posted: 0,
            threads_resolved: 0,
            apply_hint: None,
            apply_errors: Vec::new(),
            failed_fingerprints: Vec::new(),
            unapplied_fingerprints: Vec::new(),
        };

        let value = serialize_review_reconcile_json_output(
            output,
            RootEnvelopeMode::Tagged,
            Some("run-reconcile"),
        )
        .expect("review reconcile should serialize");

        assert_eq!(value["kind"], "review-reconcile");
        assert_eq!(
            value["_meta"]["telemetry"]["analysis_run_id"],
            "run-reconcile"
        );
    }

    #[test]
    fn review_id_accepts_only_the_schema_character_set_and_length() {
        assert_eq!(
            ReviewId::parse("frontend.review-1")
                .expect("review id should be valid")
                .as_str(),
            "frontend.review-1"
        );
        assert!(ReviewId::parse("").is_err());
        assert!(ReviewId::parse("has space").is_err());
        assert!(ReviewId::parse("é").is_err());
        assert!(ReviewId::parse("a".repeat(65)).is_err());
    }
}
