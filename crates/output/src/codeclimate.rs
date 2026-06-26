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
    /// Optional owner attribution used by grouped dead-code output.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<String>,
    /// Optional grouping attribution used by grouped health and duplication
    /// output.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
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

/// Fields needed to build one CodeClimate issue.
///
/// Callers decide what should be reported. This crate owns how that decision is
/// shaped into the stable CodeClimate / GitLab Code Quality wire contract.
#[derive(Debug, Clone, Copy)]
pub struct CodeClimateIssueInput<'a> {
    pub check_name: &'a str,
    pub description: &'a str,
    pub severity: CodeClimateSeverity,
    pub category: &'a str,
    pub path: &'a str,
    pub begin_line: Option<u32>,
    pub fingerprint: &'a str,
}

/// Optional grouped CodeClimate annotation field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodeClimateAnnotationField {
    /// Dead-code grouped output uses the top-level `owner` property.
    Owner,
    /// Health and duplication grouped output use the top-level `group`
    /// property.
    Group,
}

/// Compute a deterministic fingerprint hash from key fields.
///
/// Uses FNV-1a (64-bit) for guaranteed cross-version stability. `DefaultHasher`
/// is intentionally not used because it is not specified across Rust versions.
#[must_use]
pub fn codeclimate_fingerprint_hash(parts: &[&str]) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for part in parts {
        for byte in part.bytes() {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(0x0100_0000_01b3);
        }
        hash ^= 0xff;
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    format!("{hash:016x}")
}

/// Build a single CodeClimate issue from a stable contract descriptor.
#[must_use]
pub fn build_codeclimate_issue(input: CodeClimateIssueInput<'_>) -> CodeClimateIssue {
    CodeClimateIssue {
        kind: CodeClimateIssueKind::Issue,
        check_name: input.check_name.to_string(),
        description: input.description.to_string(),
        categories: vec![input.category.to_string()],
        severity: input.severity,
        fingerprint: input.fingerprint.to_string(),
        location: CodeClimateLocation {
            path: input.path.to_string(),
            lines: CodeClimateLines {
                begin: input.begin_line.unwrap_or(1),
            },
        },
        owner: None,
        group: None,
    }
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

/// Add a top-level grouped property to each typed CodeClimate issue.
///
/// Grouped CLI outputs use this to attach `owner` or `group` while keeping the
/// issue array shape and path lookup contract in `fallow-output`.
pub fn annotate_codeclimate_issues(
    issues: &mut [CodeClimateIssue],
    field: CodeClimateAnnotationField,
    mut value_for_path: impl FnMut(&str) -> String,
) {
    for issue in issues {
        let value = value_for_path(&issue.location.path);
        match field {
            CodeClimateAnnotationField::Owner => issue.owner = Some(value),
            CodeClimateAnnotationField::Group => issue.group = Some(value),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codeclimate_issue_serializes_spec_shape() {
        let issue = build_codeclimate_issue(CodeClimateIssueInput {
            check_name: "fallow/test",
            description: "description",
            category: "Bug Risk",
            severity: CodeClimateSeverity::Major,
            fingerprint: "abc123",
            path: "src/app.ts",
            begin_line: Some(7),
        });

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
    fn build_codeclimate_issue_defaults_missing_line_to_one() {
        let issue = build_codeclimate_issue(CodeClimateIssueInput {
            check_name: "fallow/test",
            description: "description",
            category: "Bug Risk",
            severity: CodeClimateSeverity::Minor,
            fingerprint: "abc123",
            path: "src/app.ts",
            begin_line: None,
        });

        assert_eq!(issue.location.lines.begin, 1);
    }

    #[test]
    fn codeclimate_fingerprint_parts_are_separated() {
        assert_ne!(
            codeclimate_fingerprint_hash(&["ab", "c"]),
            codeclimate_fingerprint_hash(&["a", "bc"])
        );
    }

    #[test]
    fn annotate_codeclimate_issues_adds_owner_from_location_path() {
        let mut issues = vec![build_codeclimate_issue(CodeClimateIssueInput {
            check_name: "fallow/test",
            description: "description",
            category: "Bug Risk",
            severity: CodeClimateSeverity::Minor,
            fingerprint: "abc123",
            path: "src/app.ts",
            begin_line: Some(3),
        })];

        annotate_codeclimate_issues(&mut issues, CodeClimateAnnotationField::Owner, |path| {
            format!("team:{path}")
        });
        let value = codeclimate_issues_to_value(&issues);

        assert_eq!(value[0]["owner"], "team:src/app.ts");
    }

    #[test]
    fn annotate_codeclimate_issues_adds_group_from_location_path() {
        let mut issues = vec![build_codeclimate_issue(CodeClimateIssueInput {
            check_name: "fallow/test",
            description: "description",
            category: "Bug Risk",
            severity: CodeClimateSeverity::Minor,
            fingerprint: "abc123",
            path: "src/app.ts",
            begin_line: Some(3),
        })];

        annotate_codeclimate_issues(&mut issues, CodeClimateAnnotationField::Group, |path| {
            format!("group:{path}")
        });
        let value = codeclimate_issues_to_value(&issues);

        assert_eq!(value[0]["group"], "group:src/app.ts");
        assert!(value[0].get("owner").is_none());
    }
}
