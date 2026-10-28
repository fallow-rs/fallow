use super::*;

#[test]
fn severity_for_kind_maps_configured_fields_and_default() {
    let rules = RulesConfig {
        unused_exports: Severity::Warn,
        unused_types: Severity::Off,
        unresolved_imports: Severity::Warn,
        boundary_violation: Severity::Off,
        ..RulesConfig::default()
    };

    assert_eq!(
        rules.severity_for_kind(IssueKind::UnusedExport),
        Severity::Warn
    );
    assert_eq!(
        rules.severity_for_kind(IssueKind::UnusedType),
        Severity::Off
    );
    assert_eq!(
        rules.severity_for_kind(IssueKind::UnresolvedImport),
        Severity::Warn
    );
    assert_eq!(
        rules.severity_for_kind(IssueKind::BoundaryViolation),
        Severity::Off
    );
    assert_eq!(
        rules.severity_for_kind(IssueKind::UnusedFile),
        Severity::Error
    );
    assert_eq!(
        rules.severity_for_kind(IssueKind::PrivateTypeLeak),
        Severity::Off
    );
    assert_eq!(
        rules.severity_for_kind(IssueKind::Complexity),
        Severity::Error
    );
    assert_eq!(
        rules.severity_for_kind(IssueKind::CodeDuplication),
        Severity::Error
    );
}

#[test]
fn rules_default_severities() {
    let rules = RulesConfig::default();
    assert_eq!(rules.unused_files, Severity::Error);
    assert_eq!(rules.unused_exports, Severity::Error);
    assert_eq!(rules.unused_types, Severity::Error);
    assert_eq!(rules.private_type_leaks, Severity::Off);
    assert_eq!(rules.unused_dependencies, Severity::Error);
    assert_eq!(rules.unused_dev_dependencies, Severity::Warn);
    assert_eq!(rules.unused_optional_dependencies, Severity::Warn);
    assert_eq!(rules.unused_enum_members, Severity::Error);
    assert_eq!(rules.unused_class_members, Severity::Error);
    assert_eq!(rules.unresolved_imports, Severity::Error);
    assert_eq!(rules.unlisted_dependencies, Severity::Error);
    assert_eq!(rules.duplicate_exports, Severity::Error);
    assert_eq!(rules.type_only_dependencies, Severity::Warn);
    assert_eq!(rules.test_only_dependencies, Severity::Warn);
    assert_eq!(rules.circular_dependencies, Severity::Error);
    assert_eq!(rules.boundary_violation, Severity::Error);
    assert_eq!(rules.coverage_gaps, Severity::Off);
    assert_eq!(rules.feature_flags, Severity::Off);
    assert_eq!(rules.stale_suppressions, Severity::Warn);
    assert_eq!(rules.unused_catalog_entries, Severity::Warn);
    assert_eq!(rules.empty_catalog_groups, Severity::Warn);
    assert_eq!(rules.unresolved_catalog_references, Severity::Error);
}

#[test]
fn rules_deserialize_kebab_case() {
    let json_str = r#"{
            "unused-files": "error",
            "unused-exports": "warn",
            "unused-types": "off"
        }"#;
    let rules: RulesConfig = serde_json::from_str(json_str).unwrap();
    assert_eq!(rules.unused_files, Severity::Error);
    assert_eq!(rules.unused_exports, Severity::Warn);
    assert_eq!(rules.unused_types, Severity::Off);
    assert_eq!(rules.unresolved_imports, Severity::Error);
}

#[test]
fn empty_rules_object_matches_default_impl() {
    // Regression for issue #1745: a present-but-empty `rules` object must
    // deserialize field-by-field to exactly the same severities as an
    // absent `rules` key (which fills from `RulesConfig::default()`).
    // A bare `#[serde(default)]` resolves to `Severity::default()` (Error),
    // so any field whose Default-impl value is Warn/Off silently promoted
    // to Error, flipping exit codes (coverage-gaps gated `fallow health`,
    // the component/store/inject rules gated CI on Vue/Svelte/Angular).
    let from_empty: RulesConfig = serde_json::from_str("{}").unwrap();
    assert_eq!(
        from_empty,
        RulesConfig::default(),
        "empty `rules` object must equal RulesConfig::default(); a field whose \
             Default is Warn/Off needs an explicit #[serde(default = ...)]"
    );
}

#[test]
fn empty_rules_object_preserves_non_error_defaults() {
    // Spot-check the nine fields fixed by #1745 so the intent is explicit
    // even if the wholesale equality assertion above is later relaxed.
    let rules: RulesConfig = serde_json::from_str("{}").unwrap();
    assert_eq!(rules.coverage_gaps, Severity::Off);
    assert_eq!(rules.unused_store_members, Severity::Warn);
    assert_eq!(rules.unprovided_injects, Severity::Warn);
    assert_eq!(rules.unrendered_components, Severity::Warn);
    assert_eq!(rules.unused_component_props, Severity::Warn);
    assert_eq!(rules.unused_component_emits, Severity::Warn);
    assert_eq!(rules.unused_component_inputs, Severity::Warn);
    assert_eq!(rules.unused_component_outputs, Severity::Warn);
    assert_eq!(rules.unused_server_actions, Severity::Warn);
}

#[test]
fn explicit_rule_severity_still_overrides_default() {
    // Control: the fix must not break explicit opt-in. Setting a value
    // still wins over the (now correct) default.
    let rules: RulesConfig =
        serde_json::from_str(r#"{ "coverage-gaps": "error", "unused-component-prop": "off" }"#)
            .unwrap();
    assert_eq!(rules.coverage_gaps, Severity::Error);
    assert_eq!(rules.unused_component_props, Severity::Off);
}

#[test]
fn rules_re_export_cycle_default_is_warn() {
    let rules = RulesConfig::default();
    assert_eq!(rules.re_export_cycle, Severity::Warn);
}

#[test]
fn rules_package_cycle_default_is_warn() {
    let rules = RulesConfig::default();
    assert_eq!(rules.package_cycle, Severity::Warn);
}

#[test]
fn package_cycle_aliases_round_trip_to_the_same_field() {
    for alias in ["package-cycle", "package-cycles"] {
        let json = format!(r#"{{"{alias}": "error"}}"#);
        let partial: PartialRulesConfig = serde_json::from_str(&json)
            .unwrap_or_else(|e| panic!("'{alias}' should deserialize: {e}"));
        assert_eq!(partial.package_cycle, Some(Severity::Error), "{alias}");
        let rules: RulesConfig = serde_json::from_str(&json)
            .unwrap_or_else(|e| panic!("'{alias}' should deserialize: {e}"));
        assert_eq!(rules.package_cycle, Severity::Error, "{alias}");
    }
}

#[test]
fn rules_deserialize_re_export_cycle_aliases() {
    for token in [
        "re-export-cycle",
        "re-export-cycles",
        "reexport-cycle",
        "reexport-cycles",
    ] {
        let json_str = format!(r#"{{ "{token}": "error" }}"#);
        let rules: RulesConfig = serde_json::from_str(&json_str)
            .unwrap_or_else(|e| panic!("alias {token} did not deserialize: {e}"));
        assert_eq!(
            rules.re_export_cycle,
            Severity::Error,
            "alias {token} should set re_export_cycle"
        );
    }
}

#[test]
fn rules_deserialize_circular_dependency_alias() {
    let json_str = r#"{
            "circular-dependency": "off"
        }"#;
    let rules: RulesConfig = serde_json::from_str(json_str).unwrap();
    assert_eq!(rules.circular_dependencies, Severity::Off);
}

#[test]
fn rules_deserialize_boundary_violations_alias() {
    let json_str = r#"{
            "boundary-violations": "off"
        }"#;
    let rules: RulesConfig = serde_json::from_str(json_str).unwrap();
    assert_eq!(rules.boundary_violation, Severity::Off);

    let partial: PartialRulesConfig = serde_json::from_str(json_str).unwrap();
    assert_eq!(partial.boundary_violation, Some(Severity::Off));
}

#[test]
fn rules_deserialize_singular_aliases_for_every_plural_rule() {
    let json_str = r#"{
            "unused-file": "off",
            "unused-export": "off",
            "unused-type": "off",
            "private-type-leak": "warn",
            "unused-dependency": "off",
            "unused-dev-dependency": "off",
            "unused-optional-dependency": "off",
            "unused-enum-member": "off",
            "unused-class-member": "off",
            "unresolved-import": "off",
            "unlisted-dependency": "off",
            "duplicate-export": "off",
            "type-only-dependency": "off",
            "test-only-dependency": "off",
            "coverage-gap": "warn",
            "feature-flag": "warn",
            "stale-suppression": "off",
            "suppression-reason": "warn",
            "unused-catalog-entry": "error",
            "empty-catalog-group": "error",
            "unresolved-catalog-reference": "warn"
        }"#;

    let rules: RulesConfig = serde_json::from_str(json_str).unwrap();
    assert_eq!(rules.unused_files, Severity::Off);
    assert_eq!(rules.unused_exports, Severity::Off);
    assert_eq!(rules.unused_types, Severity::Off);
    assert_eq!(rules.private_type_leaks, Severity::Warn);
    assert_eq!(rules.unused_dependencies, Severity::Off);
    assert_eq!(rules.unused_dev_dependencies, Severity::Off);
    assert_eq!(rules.unused_optional_dependencies, Severity::Off);
    assert_eq!(rules.unused_enum_members, Severity::Off);
    assert_eq!(rules.unused_class_members, Severity::Off);
    assert_eq!(rules.unresolved_imports, Severity::Off);
    assert_eq!(rules.unlisted_dependencies, Severity::Off);
    assert_eq!(rules.duplicate_exports, Severity::Off);
    assert_eq!(rules.type_only_dependencies, Severity::Off);
    assert_eq!(rules.test_only_dependencies, Severity::Off);
    assert_eq!(rules.coverage_gaps, Severity::Warn);
    assert_eq!(rules.feature_flags, Severity::Warn);
    assert_eq!(rules.stale_suppressions, Severity::Off);
    assert_eq!(rules.require_suppression_reason, Severity::Warn);
    assert_eq!(rules.unused_catalog_entries, Severity::Error);
    assert_eq!(rules.empty_catalog_groups, Severity::Error);
    assert_eq!(rules.unresolved_catalog_references, Severity::Warn);

    let partial: PartialRulesConfig = serde_json::from_str(json_str).unwrap();
    assert_eq!(partial.unused_files, Some(Severity::Off));
    assert_eq!(partial.unused_exports, Some(Severity::Off));
    assert_eq!(partial.unused_types, Some(Severity::Off));
    assert_eq!(partial.private_type_leaks, Some(Severity::Warn));
    assert_eq!(partial.unused_dependencies, Some(Severity::Off));
    assert_eq!(partial.unused_dev_dependencies, Some(Severity::Off));
    assert_eq!(partial.unused_optional_dependencies, Some(Severity::Off));
    assert_eq!(partial.unused_enum_members, Some(Severity::Off));
    assert_eq!(partial.unused_class_members, Some(Severity::Off));
    assert_eq!(partial.unresolved_imports, Some(Severity::Off));
    assert_eq!(partial.unlisted_dependencies, Some(Severity::Off));
    assert_eq!(partial.duplicate_exports, Some(Severity::Off));
    assert_eq!(partial.type_only_dependencies, Some(Severity::Off));
    assert_eq!(partial.test_only_dependencies, Some(Severity::Off));
    assert_eq!(partial.coverage_gaps, Some(Severity::Warn));
    assert_eq!(partial.feature_flags, Some(Severity::Warn));
    assert_eq!(partial.stale_suppressions, Some(Severity::Off));
    assert_eq!(partial.require_suppression_reason, Some(Severity::Warn));
    assert_eq!(partial.unused_catalog_entries, Some(Severity::Error));
    assert_eq!(partial.empty_catalog_groups, Some(Severity::Error));
    assert_eq!(partial.unresolved_catalog_references, Some(Severity::Warn));
}

#[test]
fn severity_from_str() {
    assert_eq!("error".parse::<Severity>().unwrap(), Severity::Error);
    assert_eq!("warn".parse::<Severity>().unwrap(), Severity::Warn);
    assert_eq!("warning".parse::<Severity>().unwrap(), Severity::Warn);
    assert_eq!("off".parse::<Severity>().unwrap(), Severity::Off);
    assert_eq!("none".parse::<Severity>().unwrap(), Severity::Off);
    assert!("invalid".parse::<Severity>().is_err());
}

#[test]
fn apply_partial_only_some_fields() {
    let mut rules = RulesConfig::default();
    let partial = PartialRulesConfig {
        unused_files: Some(Severity::Warn),
        unused_exports: Some(Severity::Off),
        ..Default::default()
    };
    rules.apply_partial(&partial);
    assert_eq!(rules.unused_files, Severity::Warn);
    assert_eq!(rules.unused_exports, Severity::Off);
    assert_eq!(rules.unused_types, Severity::Error);
    assert_eq!(rules.unresolved_imports, Severity::Error);
    assert_eq!(rules.require_suppression_reason, Severity::Off);
}

#[test]
fn require_suppression_reason_deserializes_canonical_and_alias() {
    let rules: RulesConfig = serde_json::from_str(
        r#"{
                "require-suppression-reason": "error"
            }"#,
    )
    .unwrap();
    assert_eq!(rules.require_suppression_reason, Severity::Error);

    let partial: PartialRulesConfig = serde_json::from_str(
        r#"{
                "suppression-reason": "warn"
            }"#,
    )
    .unwrap();
    assert_eq!(partial.require_suppression_reason, Some(Severity::Warn));
}

#[test]
fn complexity_rules_default_to_error_and_deserialize() {
    let rules = RulesConfig::default();
    assert_eq!(rules.complexity_cyclomatic, Severity::Error);
    assert_eq!(rules.complexity_cognitive, Severity::Error);
    assert_eq!(rules.complexity_crap, Severity::Error);

    let partial: PartialRulesConfig =
        serde_json::from_str(r#"{ "complexity-crap": "warn", "complexity-cognitive": "off" }"#)
            .unwrap();
    assert_eq!(partial.complexity_crap, Some(Severity::Warn));
    assert_eq!(partial.complexity_cognitive, Some(Severity::Off));
    assert_eq!(partial.complexity_cyclomatic, None);
}

#[test]
fn complexity_severity_takes_the_most_severe_contributing_rule() {
    let rules = RulesConfig {
        complexity_cyclomatic: Severity::Warn,
        complexity_cognitive: Severity::Off,
        complexity_crap: Severity::Error,
        ..RulesConfig::default()
    };
    assert_eq!(
        rules.complexity_severity(true, false, true),
        Severity::Error
    );
    assert_eq!(rules.complexity_severity(true, true, false), Severity::Warn);
    assert_eq!(rules.complexity_severity(false, true, false), Severity::Off);
    assert_eq!(
        rules.complexity_severity(true, false, false),
        Severity::Warn
    );
    assert_eq!(
        rules.complexity_severity(false, false, false),
        Severity::Off
    );
}

#[test]
fn severity_display() {
    assert_eq!(Severity::Error.to_string(), "error");
    assert_eq!(Severity::Warn.to_string(), "warn");
    assert_eq!(Severity::Off.to_string(), "off");
}

#[test]
fn apply_partial_all_none_changes_nothing() {
    let mut rules = RulesConfig::default();
    let original = rules.clone();
    let partial = PartialRulesConfig::default(); // all None
    rules.apply_partial(&partial);
    assert_eq!(rules.unused_files, original.unused_files);
    assert_eq!(rules.unused_exports, original.unused_exports);
    assert_eq!(
        rules.type_only_dependencies,
        original.type_only_dependencies
    );
}

#[test]
fn apply_partial_all_fields_set() {
    let mut rules = RulesConfig::default();
    let partial = PartialRulesConfig {
        unused_files: Some(Severity::Off),
        unused_exports: Some(Severity::Off),
        unused_types: Some(Severity::Off),
        private_type_leaks: Some(Severity::Off),
        unused_dependencies: Some(Severity::Off),
        unused_dev_dependencies: Some(Severity::Off),
        unused_optional_dependencies: Some(Severity::Off),
        unused_enum_members: Some(Severity::Off),
        unused_class_members: Some(Severity::Off),
        unused_store_members: Some(Severity::Off),
        unprovided_injects: Some(Severity::Off),
        unrendered_components: Some(Severity::Off),
        unused_component_props: Some(Severity::Off),
        absent_component_props: Some(Severity::Off),
        unused_component_emits: Some(Severity::Off),
        unused_component_inputs: Some(Severity::Off),
        unused_component_outputs: Some(Severity::Off),
        unused_svelte_events: Some(Severity::Off),
        unused_effect_layers: Some(Severity::Off),
        unused_tagged_errors: Some(Severity::Off),
        unproduced_catch_tags: Some(Severity::Off),
        unused_server_actions: Some(Severity::Off),
        unused_load_data_keys: Some(Severity::Off),
        prop_drilling: Some(Severity::Off),
        thin_wrapper: Some(Severity::Off),
        duplicate_prop_shape: Some(Severity::Off),
        css_token_drift: Some(Severity::Off),
        css_duplicate_block: Some(Severity::Off),
        css_selector_complexity: Some(Severity::Off),
        css_dead_surface: Some(Severity::Off),
        css_broken_reference: Some(Severity::Off),
        complexity_cyclomatic: Some(Severity::Off),
        complexity_cognitive: Some(Severity::Off),
        complexity_crap: Some(Severity::Off),
        complexity_file_lines: Some(Severity::Off),
        unresolved_imports: Some(Severity::Off),
        unlisted_dependencies: Some(Severity::Off),
        duplicate_exports: Some(Severity::Off),
        type_only_dependencies: Some(Severity::Off),
        test_only_dependencies: Some(Severity::Off),
        dev_dependencies_in_production: Some(Severity::Off),
        circular_dependencies: Some(Severity::Off),
        re_export_cycle: Some(Severity::Off),
        package_cycle: Some(Severity::Off),
        boundary_violation: Some(Severity::Off),
        coverage_gaps: Some(Severity::Off),
        feature_flags: Some(Severity::Off),
        stale_suppressions: Some(Severity::Off),
        require_suppression_reason: Some(Severity::Off),
        unused_catalog_entries: Some(Severity::Off),
        empty_catalog_groups: Some(Severity::Off),
        unresolved_catalog_references: Some(Severity::Off),
        unused_dependency_overrides: Some(Severity::Off),
        misconfigured_dependency_overrides: Some(Severity::Off),
        security_client_server_leak: Some(Severity::Off),
        security_sink: Some(Severity::Off),
        policy_violation: Some(Severity::Off),
        invalid_client_export: Some(Severity::Off),
        mixed_client_server_barrel: Some(Severity::Off),
        misplaced_directive: Some(Severity::Off),
        route_collision: Some(Severity::Off),
        dynamic_segment_name_conflict: Some(Severity::Off),
        deprecated_exports_in_use: Some(Severity::Off),
        weakened_tests: Some(Severity::Off),
        added_suppression: Some(Severity::Off),
        test_only_branch: Some(Severity::Off),
        diverged_clone_sibling: Some(Severity::Off),
        added_dependency: Some(Severity::Off),
    };
    rules.apply_partial(&partial);
    assert_eq!(rules.unused_files, Severity::Off);
    assert_eq!(rules.private_type_leaks, Severity::Off);
    assert_eq!(rules.circular_dependencies, Severity::Off);
    assert_eq!(rules.type_only_dependencies, Severity::Off);
    assert_eq!(rules.test_only_dependencies, Severity::Off);
    assert_eq!(rules.boundary_violation, Severity::Off);
    assert_eq!(rules.coverage_gaps, Severity::Off);
    assert_eq!(rules.feature_flags, Severity::Off);
    assert_eq!(rules.stale_suppressions, Severity::Off);
    assert_eq!(rules.require_suppression_reason, Severity::Off);
    assert_eq!(rules.security_sink, Severity::Off);
    assert_eq!(rules.policy_violation, Severity::Off);
    assert_eq!(rules.invalid_client_export, Severity::Off);
    assert_eq!(rules.mixed_client_server_barrel, Severity::Off);
    assert_eq!(rules.misplaced_directive, Severity::Off);
    assert_eq!(rules.unrendered_components, Severity::Off);
    assert_eq!(rules.unused_component_props, Severity::Off);
    assert_eq!(rules.unused_component_emits, Severity::Off);
    assert_eq!(rules.unused_component_inputs, Severity::Off);
    assert_eq!(rules.unused_component_outputs, Severity::Off);
    assert_eq!(rules.unused_svelte_events, Severity::Off);
    assert_eq!(rules.route_collision, Severity::Off);
    assert_eq!(rules.dynamic_segment_name_conflict, Severity::Off);
}

#[test]
fn diff_risk_rules_default_and_parse_from_kebab_keys() {
    let rules = RulesConfig::default();
    assert_eq!(rules.weakened_tests, Severity::Warn);
    assert_eq!(rules.added_suppression, Severity::Warn);
    assert_eq!(rules.test_only_branch, Severity::Warn);
    assert_eq!(rules.diverged_clone_sibling, Severity::Off);
    assert_eq!(rules.added_dependency, Severity::Off);

    let json = r#"{"weakened-tests": "error", "added-dependency": "warn"}"#;
    let partial: PartialRulesConfig = serde_json::from_str(json).expect("partial rules");
    let mut merged = RulesConfig::default();
    merged.apply_partial(&partial);
    assert_eq!(merged.weakened_tests, Severity::Error);
    assert_eq!(merged.added_dependency, Severity::Warn);
    assert_eq!(merged.added_suppression, Severity::Warn);
    assert_eq!(
        merged.severity_for_kind(IssueKind::WeakenedTests),
        Severity::Error
    );
    assert_eq!(
        merged.severity_for_kind(IssueKind::AddedDependency),
        Severity::Warn
    );
}

#[test]
fn policy_violation_defaults_to_warn() {
    let rules = RulesConfig::default();
    assert_eq!(rules.policy_violation, Severity::Warn);
}

#[test]
fn policy_violation_accepts_plural_alias() {
    let json = r#"{ "policy-violations": "error" }"#;
    let rules: RulesConfig = serde_json::from_str(json).unwrap();
    assert_eq!(rules.policy_violation, Severity::Error);
}

#[test]
fn severity_from_str_case_insensitive() {
    assert_eq!("ERROR".parse::<Severity>().unwrap(), Severity::Error);
    assert_eq!("Warn".parse::<Severity>().unwrap(), Severity::Warn);
    assert_eq!("OFF".parse::<Severity>().unwrap(), Severity::Off);
    assert_eq!("Warning".parse::<Severity>().unwrap(), Severity::Warn);
    assert_eq!("NONE".parse::<Severity>().unwrap(), Severity::Off);
}

#[test]
fn severity_from_str_invalid_returns_error() {
    let result = "critical".parse::<Severity>();
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(
        err.contains("unknown severity"),
        "Expected descriptive error, got: {err}"
    );
}

/// Pins the list's length only. This does NOT compare against
/// `RulesConfig`, so it cannot catch a rule added to the struct and
/// missing here. It exists to make an accidental deletion loud, nothing
/// more. See the note on [`KNOWN_RULE_NAMES`].
#[test]
fn known_rule_names_list_length_is_pinned() {
    assert_eq!(KNOWN_RULE_NAMES.len(), 123);
}

/// The reverse of `known_rule_names_covers_every_struct_field`. That one
/// proves every real rule is listed; this one proves every listed name is
/// real. A stale entry is not inert: `closest_known_rule_name` suggests
/// from this list, so a removed or renamed rule left behind here gets
/// offered to a user as the fix for their typo.
#[test]
fn known_rule_names_holds_no_unreachable_entry() {
    let serialized = serde_json::to_value(RulesConfig::default()).expect("RulesConfig serializes");
    let canonical = serialized
        .as_object()
        .expect("RulesConfig serializes to an object");
    let source = include_str!("rules.rs");

    let unreachable: Vec<&&str> = KNOWN_RULE_NAMES
        .iter()
        .filter(|name| !canonical.contains_key(**name))
        .filter(|name| !source.contains(&format!("alias = \"{name}\"")))
        .collect();

    assert!(
        unreachable.is_empty(),
        "KNOWN_RULE_NAMES entries that are neither a serialized rule nor a declared \
             serde alias: {unreachable:?}. closest_known_rule_name suggests from this list, \
             so each one can be offered to a user as a rule that does not exist."
    );
}

#[test]
fn known_rule_names_has_no_duplicates() {
    let mut sorted: Vec<&str> = KNOWN_RULE_NAMES.to_vec();
    sorted.sort_unstable();
    let original_len = sorted.len();
    sorted.dedup();
    assert_eq!(
        sorted.len(),
        original_len,
        "KNOWN_RULE_NAMES contains a duplicate"
    );
}

#[test]
fn known_rule_names_covers_every_serde_alias_in_source() {
    let source = include_str!("rules.rs");

    let mut aliases_found = Vec::new();
    for line in source.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("//") {
            continue;
        }
        let Some(after) = trimmed.split("alias = \"").nth(1) else {
            continue;
        };
        let Some(end) = after.find('"') else {
            continue;
        };
        let alias = &after[..end];
        if alias.is_empty() || !alias.chars().all(|c| c.is_ascii_lowercase() || c == '-') {
            continue;
        }
        aliases_found.push(alias.to_owned());
    }

    assert_eq!(
        aliases_found.len(),
        126,
        "expected 126 source-level alias attrs (63 per struct); got {}: {:?}",
        aliases_found.len(),
        aliases_found
    );

    for alias in &aliases_found {
        assert!(
            KNOWN_RULE_NAMES.contains(&alias.as_str()),
            "serde alias '{alias}' is in rules.rs source but missing from KNOWN_RULE_NAMES"
        );
    }
}

#[test]
fn re_export_cycle_aliases_all_round_trip_to_the_same_field() {
    for alias in [
        "re-export-cycle",
        "re-export-cycles",
        "reexport-cycle",
        "reexport-cycles",
    ] {
        let json = format!(r#"{{"{alias}": "warn"}}"#);
        let partial: PartialRulesConfig = serde_json::from_str(&json)
            .unwrap_or_else(|e| panic!("'{alias}' should deserialize: {e}"));
        assert_eq!(
            partial.re_export_cycle,
            Some(Severity::Warn),
            "'{alias}' should set re_export_cycle to Warn"
        );
        let serialized = serde_json::to_value(&partial).unwrap();
        let map = serialized.as_object().unwrap();
        assert_eq!(
            map.len(),
            1,
            "'{alias}' should resolve to exactly one field, got: {map:?}"
        );
    }
}

#[test]
fn every_known_rule_name_round_trips_through_partial() {
    for &name in KNOWN_RULE_NAMES {
        let json = format!(r#"{{"{name}": "warn"}}"#);
        let partial: PartialRulesConfig = serde_json::from_str(&json)
            .unwrap_or_else(|e| panic!("'{name}' should deserialize: {e}"));

        let serialized = serde_json::to_value(&partial).unwrap();
        let map = serialized.as_object().unwrap();
        assert_eq!(
            map.len(),
            1,
            "'{name}' should resolve to exactly one field, got: {map:?}"
        );
    }
}

#[test]
fn known_rule_names_covers_every_struct_field() {
    let json = serde_json::to_value(RulesConfig::default()).unwrap();
    let obj = json.as_object().unwrap();
    for key in obj.keys() {
        assert!(
            KNOWN_RULE_NAMES.contains(&key.as_str()),
            "field '{key}' is serialized but missing from KNOWN_RULE_NAMES"
        );
    }
}

#[test]
fn closest_known_rule_name_suggests_for_obvious_typo() {
    assert_eq!(
        closest_known_rule_name("unsued-files"),
        Some("unused-files")
    );
    assert_eq!(
        closest_known_rule_name("circular-dependnecy"),
        Some("circular-dependency")
    );
    assert_eq!(
        closest_known_rule_name("unused-dep"),
        None,
        "too short for a confident suggestion"
    );
}

#[test]
fn closest_known_rule_name_returns_none_for_novel_input() {
    assert_eq!(closest_known_rule_name("totally-fabricated"), None);
    assert_eq!(closest_known_rule_name("foo"), None);
}

#[test]
fn closest_known_rule_name_is_case_insensitive() {
    assert_eq!(
        closest_known_rule_name("UNSUED-FILES"),
        Some("unused-files")
    );
}

#[test]
fn closest_known_rule_name_returns_none_for_exact_match() {
    assert_eq!(closest_known_rule_name("unused-files"), None);
}

#[test]
fn find_unknown_rule_keys_flags_typo() {
    let v = serde_json::json!({
        "unsued-files": "warn",
        "unused-exports": "off",
    });
    let unknown = find_unknown_rule_keys(&v, "rules");
    assert_eq!(unknown.len(), 1);
    assert_eq!(unknown[0].key, "unsued-files");
    assert_eq!(unknown[0].context, "rules");
    assert_eq!(unknown[0].suggestion, Some("unused-files"));
}

#[test]
fn find_unknown_rule_keys_passes_aliases() {
    let v = serde_json::json!({
        "unused-file": "warn",
        "circular-dependency": "off",
        "boundary-violations": "warn",
    });
    let unknown = find_unknown_rule_keys(&v, "rules");
    assert!(
        unknown.is_empty(),
        "documented aliases must not flag as unknown: {unknown:?}"
    );
}

#[test]
fn find_unknown_rule_keys_returns_multiple_typos() {
    let v = serde_json::json!({
        "unsued-files": "warn",
        "circular-dependnecy": "off",
    });
    let unknown = find_unknown_rule_keys(&v, "rules");
    assert_eq!(unknown.len(), 2);
}

#[test]
fn find_unknown_rule_keys_carries_context() {
    let v = serde_json::json!({ "unsued-files": "warn" });
    let unknown = find_unknown_rule_keys(&v, "overrides[2].rules");
    assert_eq!(unknown[0].context, "overrides[2].rules");
}

#[test]
fn find_unknown_rule_keys_empty_when_not_object() {
    let v = serde_json::json!(null);
    assert!(find_unknown_rule_keys(&v, "rules").is_empty());

    let v = serde_json::json!([1, 2, 3]);
    assert!(find_unknown_rule_keys(&v, "rules").is_empty());
}

#[test]
fn find_unknown_rule_keys_no_suggestion_for_novel_name() {
    let v = serde_json::json!({ "totally-fabricated-rule": "warn" });
    let unknown = find_unknown_rule_keys(&v, "rules");
    assert_eq!(unknown.len(), 1);
    assert_eq!(unknown[0].suggestion, None);
}

#[test]
fn partial_rules_empty_json() {
    let partial: PartialRulesConfig = serde_json::from_str("{}").unwrap();
    assert!(partial.unused_files.is_none());
    assert!(partial.unused_exports.is_none());
    assert!(partial.unused_types.is_none());
    assert!(partial.unused_dependencies.is_none());
    assert!(partial.circular_dependencies.is_none());
    assert!(partial.boundary_violation.is_none());
    assert!(partial.coverage_gaps.is_none());
    assert!(partial.feature_flags.is_none());
    assert!(partial.stale_suppressions.is_none());
}

#[test]
fn partial_rules_subset_json() {
    let json = r#"{
            "unused-files": "warn",
            "circular-dependencies": "off"
        }"#;
    let partial: PartialRulesConfig = serde_json::from_str(json).unwrap();
    assert_eq!(partial.unused_files, Some(Severity::Warn));
    assert_eq!(partial.circular_dependencies, Some(Severity::Off));
    assert!(partial.unused_exports.is_none());
}

#[test]
fn partial_rules_deserialize_circular_dependency_alias() {
    let json = r#"{
            "circular-dependency": "warn"
        }"#;
    let partial: PartialRulesConfig = serde_json::from_str(json).unwrap();
    assert_eq!(partial.circular_dependencies, Some(Severity::Warn));
}

#[test]
fn partial_rules_all_fields_json() {
    let json = r#"{
            "unused-files": "error",
            "unused-exports": "warn",
            "unused-types": "off",
            "unused-dependencies": "error",
            "unused-dev-dependencies": "warn",
            "unused-optional-dependencies": "off",
            "unused-enum-members": "error",
            "unused-class-members": "warn",
            "unresolved-imports": "off",
            "unlisted-dependencies": "error",
            "duplicate-exports": "warn",
            "type-only-dependencies": "off",
            "test-only-dependencies": "error",
            "circular-dependencies": "warn",
            "boundary-violation": "off",
            "coverage-gaps": "warn",
            "feature-flags": "error",
            "stale-suppressions": "off"
        }"#;
    let partial: PartialRulesConfig = serde_json::from_str(json).unwrap();
    assert_eq!(partial.unused_files, Some(Severity::Error));
    assert_eq!(partial.unused_exports, Some(Severity::Warn));
    assert_eq!(partial.unused_types, Some(Severity::Off));
    assert_eq!(partial.unused_dependencies, Some(Severity::Error));
    assert_eq!(partial.unused_dev_dependencies, Some(Severity::Warn));
    assert_eq!(partial.unused_optional_dependencies, Some(Severity::Off));
    assert_eq!(partial.unused_enum_members, Some(Severity::Error));
    assert_eq!(partial.unused_class_members, Some(Severity::Warn));
    assert_eq!(partial.unresolved_imports, Some(Severity::Off));
    assert_eq!(partial.unlisted_dependencies, Some(Severity::Error));
    assert_eq!(partial.duplicate_exports, Some(Severity::Warn));
    assert_eq!(partial.type_only_dependencies, Some(Severity::Off));
    assert_eq!(partial.test_only_dependencies, Some(Severity::Error));
    assert_eq!(partial.circular_dependencies, Some(Severity::Warn));
    assert_eq!(partial.boundary_violation, Some(Severity::Off));
    assert_eq!(partial.coverage_gaps, Some(Severity::Warn));
    assert_eq!(partial.feature_flags, Some(Severity::Error));
    assert_eq!(partial.stale_suppressions, Some(Severity::Off));
}

#[test]
fn partial_rules_none_fields_not_serialized() {
    let partial = PartialRulesConfig::default();
    let json = serde_json::to_string(&partial).unwrap();
    assert_eq!(
        json, "{}",
        "all-None partial should serialize to empty object"
    );
}

#[test]
fn partial_rules_some_fields_serialized() {
    let partial = PartialRulesConfig {
        unused_files: Some(Severity::Warn),
        ..Default::default()
    };
    let json = serde_json::to_string(&partial).unwrap();
    assert!(json.contains("unused-files"));
    assert!(!json.contains("unused-exports"));
}

#[test]
fn severity_json_deserialization() {
    let error: Severity = serde_json::from_str(r#""error""#).unwrap();
    assert_eq!(error, Severity::Error);

    let warn: Severity = serde_json::from_str(r#""warn""#).unwrap();
    assert_eq!(warn, Severity::Warn);

    let off: Severity = serde_json::from_str(r#""off""#).unwrap();
    assert_eq!(off, Severity::Off);
}

#[test]
fn severity_invalid_json_value_rejected() {
    let result: Result<Severity, _> = serde_json::from_str(r#""critical""#);
    assert!(result.is_err());
}

#[test]
fn severity_default_is_error() {
    assert_eq!(Severity::default(), Severity::Error);
}

#[test]
fn rules_config_json_roundtrip() {
    let rules = RulesConfig {
        unused_files: Severity::Warn,
        unused_exports: Severity::Off,
        type_only_dependencies: Severity::Error,
        ..RulesConfig::default()
    };
    let json = serde_json::to_string(&rules).unwrap();
    let restored: RulesConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(restored.unused_files, Severity::Warn);
    assert_eq!(restored.unused_exports, Severity::Off);
    assert_eq!(restored.type_only_dependencies, Severity::Error);
    assert_eq!(restored.unused_dependencies, Severity::Error); // default
}

#[test]
fn apply_partial_preserves_type_only_default() {
    let mut rules = RulesConfig::default();
    let partial = PartialRulesConfig {
        unused_files: Some(Severity::Off),
        ..Default::default()
    };
    rules.apply_partial(&partial);
    assert_eq!(rules.type_only_dependencies, Severity::Warn);
    assert_eq!(rules.test_only_dependencies, Severity::Warn);
}
