use serde::Serialize;

/// Envelope emitted by `fallow --format codeclimate` and
/// `fallow --format gitlab-codequality`. GitLab Code Quality consumes the
/// same shape. The wire form is a bare JSON array, not an object.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(
    feature = "schema",
    schemars(title = "fallow --format codeclimate / gitlab-codequality")
)]
#[serde(transparent)]
#[allow(
    dead_code,
    reason = "schema-source-of-truth wrapper: runtime emits a Vec<CodeClimateIssue> directly; this newtype exists so schemars can title and document the bare-array shape for the drift gate."
)]
pub struct CodeClimateOutput(pub Vec<CodeClimateIssue>);

/// Single CodeClimate-compatible issue inside [`CodeClimateOutput`].
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct CodeClimateIssue {
    #[serde(rename = "type")]
    pub kind: CodeClimateIssueKind,
    pub check_name: String,
    pub description: String,
    pub categories: Vec<String>,
    pub severity: CodeClimateSeverity,
    pub fingerprint: String,
    pub location: CodeClimateLocation,
}

/// Discriminator value for [`CodeClimateIssue::kind`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "lowercase")]
pub enum CodeClimateIssueKind {
    /// The only valid CodeClimate type today.
    Issue,
}

/// CodeClimate severity scale.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "lowercase")]
pub enum CodeClimateSeverity {
    /// Informational. Reserved for future severity mappings; not produced
    /// by the current runtime path (which only emits Minor / Major /
    /// Critical via `severity_to_codeclimate` and the health / runtime-
    /// coverage match arms).
    #[allow(
        dead_code,
        reason = "schema-source-of-truth: documents the full CodeClimate severity spec; runtime never produces this variant today."
    )]
    Info,
    /// Minor finding.
    Minor,
    /// Major finding.
    Major,
    /// Critical finding.
    Critical,
    /// Blocker (highest severity). Reserved for future severity
    /// mappings; not produced by the current runtime path.
    #[allow(
        dead_code,
        reason = "schema-source-of-truth: documents the full CodeClimate severity spec; runtime never produces this variant today."
    )]
    Blocker,
}

/// Location block inside [`CodeClimateIssue::location`].
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct CodeClimateLocation {
    /// File path relative to the analysed root.
    pub path: String,
    /// Wrapper carrying the begin line so the schema lines up with
    /// CodeClimate's spec.
    pub lines: CodeClimateLines,
}

/// `lines.begin` for [`CodeClimateLocation`].
#[derive(Debug, Clone, Copy, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct CodeClimateLines {
    /// 1-based start line.
    pub begin: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codeclimate_issue_serializes_spec_shape() {
        let issue = CodeClimateIssue {
            kind: CodeClimateIssueKind::Issue,
            check_name: "fallow/test".to_string(),
            description: "description".to_string(),
            categories: vec!["Bug Risk".to_string()],
            severity: CodeClimateSeverity::Major,
            fingerprint: "abc123".to_string(),
            location: CodeClimateLocation {
                path: "src/app.ts".to_string(),
                lines: CodeClimateLines { begin: 7 },
            },
        };

        let value = serde_json::to_value(issue).expect("CodeClimate issue serializes");
        assert_eq!(value["type"], "issue");
        assert_eq!(value["severity"], "major");
        assert_eq!(value["location"]["lines"]["begin"], 7);
    }

    #[test]
    fn output_serializes_as_bare_array() {
        let output = CodeClimateOutput(Vec::new());
        let value = serde_json::to_value(output).expect("CodeClimate output serializes");
        assert!(value.is_array());
    }
}
