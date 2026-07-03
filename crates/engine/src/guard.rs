//! Typed guard report assembly for pre-edit architecture guidance.

use std::fmt;
use std::path::{Component, Path};

use fallow_config::{ResolvedBoundaryConfig, ResolvedConfig, RulePackRule, RulePackRuleKind};
use fallow_types::guard::{
    GuardBoundary, GuardFileReport, GuardPolicyRule, GuardReport, GuardSeverities, GuardZone,
};

/// Error returned when a guard target cannot be represented safely.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GuardError {
    /// The requested target is outside the resolved project root.
    OutsideRoot(String),
}

impl fmt::Display for GuardError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OutsideRoot(path) => write!(f, "guard target is outside project root: {path}"),
        }
    }
}

impl std::error::Error for GuardError {}

/// Build a typed guard report for one or more target files.
///
/// Paths may be project-relative or absolute under `config.root`. Returned
/// paths are project-root-relative and use forward slashes.
///
/// # Errors
///
/// Returns [`GuardError::OutsideRoot`] for absolute paths outside the project
/// root or relative paths containing parent-directory traversal.
pub fn build_guard_report(
    config: &ResolvedConfig,
    files: &[String],
) -> Result<GuardReport, GuardError> {
    let mut reports = Vec::with_capacity(files.len());
    for file in files {
        reports.push(build_file_report(config, file)?);
    }
    Ok(GuardReport { files: reports })
}

fn build_file_report(config: &ResolvedConfig, input: &str) -> Result<GuardFileReport, GuardError> {
    let rel_path = normalize_target_path(config, input)?;
    let full_path = config.root.join(&rel_path);
    let rules = config.resolve_rules_for_path(&full_path);
    let zone_name = config.boundaries.classify_zone(&rel_path);
    let zone = zone_name.and_then(|name| guard_zone(&config.boundaries, name));
    let notes = guard_notes(config, zone_name);

    Ok(GuardFileReport {
        exists: full_path.exists(),
        boundary: guard_boundary(&config.boundaries, &rel_path, zone_name),
        policy_rules: guard_policy_rules(config, &rel_path, rules.policy_violation),
        severities: GuardSeverities {
            boundary_violation: rules.boundary_violation.to_string(),
            policy_violation: rules.policy_violation.to_string(),
        },
        path: rel_path,
        zone,
        notes,
    })
}

fn normalize_target_path(config: &ResolvedConfig, input: &str) -> Result<String, GuardError> {
    let normalized = input.replace('\\', "/");
    let path = Path::new(&normalized);
    if looks_windows_absolute(&normalized) && !path.is_absolute() {
        return Err(GuardError::OutsideRoot(input.to_string()));
    }
    let relative = if path.is_absolute() {
        path.strip_prefix(&config.root)
            .map_err(|_| GuardError::OutsideRoot(input.to_string()))?
    } else {
        path
    };
    normalize_relative_path(relative, input)
}

fn looks_windows_absolute(path: &str) -> bool {
    let bytes = path.as_bytes();
    bytes.len() >= 3 && bytes[1] == b':' && bytes[2] == b'/'
}

fn normalize_relative_path(path: &Path, original: &str) -> Result<String, GuardError> {
    let mut parts = Vec::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::Normal(part) => parts.push(part.to_string_lossy().replace('\\', "/")),
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(GuardError::OutsideRoot(original.to_string()));
            }
        }
    }
    Ok(parts.join("/"))
}

fn guard_zone(boundaries: &ResolvedBoundaryConfig, name: &str) -> Option<GuardZone> {
    boundaries
        .zones
        .iter()
        .find(|zone| zone.name == name)
        .map(|zone| GuardZone {
            name: zone.name.clone(),
            patterns: zone.patterns.clone(),
        })
}

fn guard_boundary(
    boundaries: &ResolvedBoundaryConfig,
    rel_path: &str,
    zone_name: Option<&str>,
) -> GuardBoundary {
    let configured = boundaries_configured(boundaries);
    let coverage_required = zone_name.is_none()
        && boundaries.coverage.require_all_files
        && !boundaries.allows_unmatched(rel_path);

    let Some(zone_name) = zone_name else {
        return GuardBoundary {
            configured,
            unrestricted: true,
            allowed_zones: Vec::new(),
            allowed_type_only_zones: Vec::new(),
            forbidden_calls: Vec::new(),
            coverage_required,
        };
    };

    let forbidden_calls = boundaries
        .calls_forbidden_by_zone
        .get(zone_name)
        .cloned()
        .unwrap_or_default();
    let Some(rule) = boundaries
        .rules
        .iter()
        .find(|rule| rule.from_zone == zone_name)
    else {
        return GuardBoundary {
            configured,
            unrestricted: true,
            allowed_zones: Vec::new(),
            allowed_type_only_zones: Vec::new(),
            forbidden_calls,
            coverage_required,
        };
    };

    let mut allowed_zones = vec![zone_name.to_string()];
    allowed_zones.extend(rule.allowed_zones.iter().cloned());
    allowed_zones.sort();
    allowed_zones.dedup();

    GuardBoundary {
        configured,
        unrestricted: false,
        allowed_zones,
        allowed_type_only_zones: rule.allow_type_only_zones.clone(),
        forbidden_calls,
        coverage_required,
    }
}

fn guard_notes(config: &ResolvedConfig, zone_name: Option<&str>) -> Vec<String> {
    let mut notes = Vec::new();
    if boundaries_configured(&config.boundaries) && zone_name.is_none() {
        notes.push("Files outside every zone are unrestricted for boundary checks.".to_string());
    }
    if !boundaries_configured(&config.boundaries) && config.rule_packs.is_empty() {
        notes.push("No boundary zones or rule packs are configured.".to_string());
    }
    if zone_name.is_some() {
        notes.push("Same-zone imports are always allowed.".to_string());
    }
    notes
}

fn boundaries_configured(boundaries: &ResolvedBoundaryConfig) -> bool {
    !boundaries.zones.is_empty() || !boundaries.logical_groups.is_empty()
}

fn guard_policy_rules(
    config: &ResolvedConfig,
    rel_path: &str,
    master_severity: fallow_config::Severity,
) -> Vec<GuardPolicyRule> {
    if master_severity == fallow_config::Severity::Off {
        return Vec::new();
    }

    crate::core_backend::rules_applying_to_path(config, rel_path)
        .into_iter()
        .filter_map(|(pack, rule)| guard_policy_rule(pack, rule, master_severity))
        .collect()
}

fn guard_policy_rule(
    pack: &str,
    rule: &RulePackRule,
    master_severity: fallow_config::Severity,
) -> Option<GuardPolicyRule> {
    let severity = rule.severity.unwrap_or(master_severity);
    if severity == fallow_config::Severity::Off {
        return None;
    }

    Some(GuardPolicyRule {
        pack: pack.to_string(),
        rule_id: rule.id.clone(),
        kind: rule_kind(rule.kind).to_string(),
        patterns: rule_patterns(rule),
        message: rule.message.clone(),
        severity: severity.to_string(),
        suppress_token: format!("policy-violation:{pack}/{}", rule.id),
    })
}

const fn rule_kind(kind: RulePackRuleKind) -> &'static str {
    match kind {
        RulePackRuleKind::BannedCall => "banned-call",
        RulePackRuleKind::BannedImport => "banned-import",
        RulePackRuleKind::BannedEffect => "banned-effect",
    }
}

fn rule_patterns(rule: &RulePackRule) -> Vec<String> {
    match rule.kind {
        RulePackRuleKind::BannedCall => rule.callees.clone(),
        RulePackRuleKind::BannedImport => rule.specifiers.clone(),
        RulePackRuleKind::BannedEffect => rule
            .effects
            .iter()
            .map(|effect| effect.as_str().to_string())
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fallow_config::{
        BoundaryCallsConfig, BoundaryConfig, BoundaryCoverageConfig, BoundaryRule, BoundaryZone,
        EffectKind, FallowConfig, ForbiddenCallRule, ForbiddenCallee, OutputFormat, RulePackDef,
        RulePackRule, RulePackRuleKind, RulesConfig, Severity,
    };
    use std::fs;

    fn rule(id: &str, kind: RulePackRuleKind) -> RulePackRule {
        RulePackRule {
            id: id.to_string(),
            kind,
            callees: Vec::new(),
            specifiers: Vec::new(),
            effects: Vec::new(),
            ignore_type_only: false,
            files: Vec::new(),
            exclude: Vec::new(),
            message: None,
            severity: None,
        }
    }

    fn pack(rules: Vec<RulePackRule>) -> RulePackDef {
        RulePackDef {
            schema: None,
            version: 1,
            name: "team-policy".to_string(),
            description: None,
            rules,
        }
    }

    fn resolve(root: &Path, configure: impl FnOnce(&mut FallowConfig)) -> ResolvedConfig {
        let mut config = FallowConfig {
            rules: RulesConfig {
                policy_violation: Severity::Warn,
                ..RulesConfig::default()
            },
            ..FallowConfig::default()
        };
        configure(&mut config);
        config.resolve(root.to_path_buf(), OutputFormat::Json, 1, true, true, None)
    }

    #[test]
    fn zoned_file_reports_allow_rule_and_forbidden_call() {
        let temp = tempfile::tempdir().expect("tempdir");
        fs::create_dir_all(temp.path().join("src/domain")).expect("create dir");
        fs::write(temp.path().join("src/domain/user.ts"), "").expect("write file");
        let config = resolve(temp.path(), |config| {
            config.boundaries = BoundaryConfig {
                zones: vec![
                    BoundaryZone {
                        name: "domain".to_string(),
                        patterns: vec!["src/domain/**".to_string()],
                        auto_discover: Vec::new(),
                        root: None,
                    },
                    BoundaryZone {
                        name: "shared".to_string(),
                        patterns: vec!["src/shared/**".to_string()],
                        auto_discover: Vec::new(),
                        root: None,
                    },
                ],
                rules: vec![BoundaryRule {
                    from: "domain".to_string(),
                    allow: vec!["shared".to_string()],
                    allow_type_only: vec!["ui".to_string()],
                }],
                calls: BoundaryCallsConfig {
                    forbidden: vec![ForbiddenCallRule {
                        from: "domain".to_string(),
                        callee: ForbiddenCallee::Single("child_process.*".to_string()),
                    }],
                },
                ..BoundaryConfig::default()
            };
        });

        let report =
            build_guard_report(&config, &["src/domain/user.ts".to_string()]).expect("report");
        let file = &report.files[0];

        assert!(file.exists);
        assert_eq!(
            file.zone.as_ref().map(|zone| zone.name.as_str()),
            Some("domain")
        );
        assert!(!file.boundary.unrestricted);
        assert_eq!(file.boundary.allowed_zones, vec!["domain", "shared"]);
        assert_eq!(file.boundary.allowed_type_only_zones, vec!["ui"]);
        assert_eq!(file.boundary.forbidden_calls, vec!["child_process.*"]);
        assert!(file.notes.iter().any(|note| note.contains("Same-zone")));
    }

    #[test]
    fn unzoned_file_reports_required_coverage() {
        let temp = tempfile::tempdir().expect("tempdir");
        let config = resolve(temp.path(), |config| {
            config.boundaries = BoundaryConfig {
                zones: vec![BoundaryZone {
                    name: "domain".to_string(),
                    patterns: vec!["src/domain/**".to_string()],
                    auto_discover: Vec::new(),
                    root: None,
                }],
                coverage: BoundaryCoverageConfig {
                    require_all_files: true,
                    allow_unmatched: vec!["src/generated/**".to_string()],
                },
                ..BoundaryConfig::default()
            };
        });

        let report =
            build_guard_report(&config, &["src/ui/button.ts".to_string()]).expect("report");
        let file = &report.files[0];

        assert!(file.zone.is_none());
        assert!(file.boundary.unrestricted);
        assert!(file.boundary.coverage_required);
        assert!(
            file.notes
                .iter()
                .any(|note| note.contains("outside every zone"))
        );

        let allowed =
            build_guard_report(&config, &["src/generated/client.ts".to_string()]).expect("report");
        assert!(!allowed.files[0].boundary.coverage_required);
    }

    #[test]
    fn pack_rule_scope_filters_policy_rules() {
        let temp = tempfile::tempdir().expect("tempdir");
        let mut domain_rule = rule("pure-domain", RulePackRuleKind::BannedEffect);
        domain_rule.effects = vec![EffectKind::Network];
        domain_rule.files = vec!["src/domain/**".to_string()];
        let mut excluded_rule = rule("no-generated-process", RulePackRuleKind::BannedCall);
        excluded_rule.callees = vec!["child_process.*".to_string()];
        excluded_rule.exclude = vec!["src/domain/**".to_string()];
        let mut config = resolve(temp.path(), |_| {});
        config.rule_packs = vec![pack(vec![domain_rule, excluded_rule])];

        let report =
            build_guard_report(&config, &["src/domain/user.ts".to_string()]).expect("report");
        let rules = &report.files[0].policy_rules;

        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].rule_id, "pure-domain");
        assert_eq!(rules[0].kind, "banned-effect");
        assert_eq!(rules[0].patterns, vec!["network"]);
        assert_eq!(
            rules[0].suppress_token,
            "policy-violation:team-policy/pure-domain"
        );
        assert_eq!(rules[0].severity, "warn");
    }

    #[test]
    fn nonexistent_target_reports_exists_false() {
        let temp = tempfile::tempdir().expect("tempdir");
        let config = resolve(temp.path(), |_| {});

        let report = build_guard_report(&config, &["src/missing.ts".to_string()]).expect("report");

        assert_eq!(report.files[0].path, "src/missing.ts");
        assert!(!report.files[0].exists);
    }

    #[test]
    fn path_outside_root_errors() {
        let temp = tempfile::tempdir().expect("tempdir");
        let config = resolve(temp.path(), |_| {});

        let err = build_guard_report(&config, &["../outside.ts".to_string()]).unwrap_err();

        assert!(matches!(err, GuardError::OutsideRoot(_)));
    }
}
