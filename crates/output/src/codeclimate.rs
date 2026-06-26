use serde::Serialize;
use serde_json::Value;

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

/// Serialize typed CodeClimate issues to the wire-shape JSON array.
///
/// Infallible: `CodeClimateIssue` contains only strings, integers, arrays, and
/// enums serialized as fixed strings.
#[must_use]
#[expect(
    clippy::expect_used,
    reason = "CodeClimateIssue contains only infallibly serializable fields"
)]
pub fn codeclimate_issues_to_value(issues: &[CodeClimateIssue]) -> Value {
    serde_json::to_value(issues).expect("CodeClimateIssue serializes infallibly")
}

/// Add a top-level string property to each serialized CodeClimate issue.
///
/// Grouped CLI outputs use this to attach `owner` or `group` while keeping the
/// issue array shape and path lookup contract in `fallow-output`.
pub fn annotate_codeclimate_issues(
    value: &mut Value,
    field: &'static str,
    mut value_for_path: impl FnMut(&str) -> String,
) {
    if let Some(items) = value.as_array_mut() {
        for issue in items {
            let path = issue
                .pointer("/location/path")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            if let Some(object) = issue.as_object_mut() {
                object.insert(field.to_string(), Value::String(value_for_path(&path)));
            }
        }
    }
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

    #[test]
    fn codeclimate_issues_to_value_serializes_bare_array() {
        let value = codeclimate_issues_to_value(&[]);
        assert!(value.is_array());
    }

    #[test]
    fn annotate_codeclimate_issues_adds_property_from_location_path() {
        let mut value = serde_json::json!([
            {
                "type": "issue",
                "location": { "path": "src/app.ts", "lines": { "begin": 3 } }
            }
        ]);

        annotate_codeclimate_issues(&mut value, "owner", |path| format!("team:{path}"));

        assert_eq!(value[0]["owner"], "team:src/app.ts");
    }
}
