//! Guard report contracts for pre-edit architecture guidance.

/// Per-file guard report for one or more requested paths.
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct GuardReport {
    /// Reports in the same order as the requested files.
    pub files: Vec<GuardFileReport>,
}

/// Guard information for a single project-root-relative path.
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct GuardFileReport {
    /// Project-root-relative path using forward slashes.
    pub path: String,
    /// Whether the path currently exists on disk.
    pub exists: bool,
    /// Boundary zone classification for this file, when any zone matches.
    pub zone: Option<GuardZone>,
    /// Boundary rules that apply to this file.
    pub boundary: GuardBoundary,
    /// Rule-pack policy rules in scope for this file.
    pub policy_rules: Vec<GuardPolicyRule>,
    /// Effective severities for rule families relevant to guard output.
    pub severities: GuardSeverities,
    /// Human-readable notes for unrestricted or degraded cases.
    pub notes: Vec<String>,
}

/// Boundary zone matched by a guard target.
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct GuardZone {
    /// Zone name from the boundary configuration.
    pub name: String,
    /// Configured glob patterns that define the zone.
    pub patterns: Vec<String>,
}

/// Boundary permissions and call restrictions for a guard target.
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct GuardBoundary {
    /// Whether boundary zones are configured at all.
    pub configured: bool,
    /// Whether boundary imports are unrestricted for this file.
    pub unrestricted: bool,
    /// Zones this file may import from.
    pub allowed_zones: Vec<String>,
    /// Zones this file may import from with type-only imports.
    pub allowed_type_only_zones: Vec<String>,
    /// Forbidden callee patterns for the file's zone.
    pub forbidden_calls: Vec<String>,
    /// Whether boundary coverage requires this file to belong to a zone.
    pub coverage_required: bool,
}

/// Rule-pack policy rule that applies to a guard target.
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct GuardPolicyRule {
    /// Rule-pack name.
    pub pack: String,
    /// Rule id inside the pack.
    pub rule_id: String,
    /// Rule kind in kebab-case.
    pub kind: String,
    /// Matcher patterns for the rule, such as callees, import specifiers, or effects.
    pub patterns: Vec<String>,
    /// Optional rule-authored remediation message.
    pub message: Option<String>,
    /// Effective severity for this rule at the target path.
    pub severity: String,
    /// Scoped suppression token for this specific policy rule.
    pub suppress_token: String,
}

/// Effective guard-relevant rule severities for a target path.
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct GuardSeverities {
    /// Effective severity of boundary-violation findings.
    pub boundary_violation: String,
    /// Effective severity of policy-violation findings.
    pub policy_violation: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guard_file_report_serializes_expected_wire_shape() {
        let report = GuardReport {
            files: vec![GuardFileReport {
                path: "src/domain/user.ts".to_string(),
                exists: false,
                zone: None,
                boundary: GuardBoundary {
                    configured: true,
                    unrestricted: true,
                    allowed_zones: vec![],
                    allowed_type_only_zones: vec![],
                    forbidden_calls: vec!["child_process.*".to_string()],
                    coverage_required: true,
                },
                policy_rules: vec![GuardPolicyRule {
                    pack: "team-policy".to_string(),
                    rule_id: "pure-domain".to_string(),
                    kind: "banned-effect".to_string(),
                    patterns: vec!["network".to_string()],
                    message: Some("Inject effects via ports.".to_string()),
                    severity: "warn".to_string(),
                    suppress_token: "policy-violation:team-policy/pure-domain".to_string(),
                }],
                severities: GuardSeverities {
                    boundary_violation: "error".to_string(),
                    policy_violation: "warn".to_string(),
                },
                notes: vec!["Files outside every zone are unrestricted.".to_string()],
            }],
        };

        let json = serde_json::to_value(report).unwrap();
        let file = &json["files"][0];
        assert_eq!(file["path"], "src/domain/user.ts");
        assert_eq!(file["exists"], false);
        assert!(file["zone"].is_null());
        assert_eq!(file["boundary"]["allowed_zones"], serde_json::json!([]));
        assert_eq!(
            file["boundary"]["allowed_type_only_zones"],
            serde_json::json!([])
        );
        assert_eq!(file["boundary"]["coverage_required"], true);
        assert_eq!(file["policy_rules"][0]["rule_id"], "pure-domain");
        assert_eq!(
            file["policy_rules"][0]["suppress_token"],
            "policy-violation:team-policy/pure-domain"
        );
        assert_eq!(file["severities"]["boundary_violation"], "error");
    }
}
