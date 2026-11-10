use super::*;
use crate::CacheConfig;
use crate::config::boundaries::BoundaryConfig;
use crate::config::health::HealthConfig;

#[test]
fn mutation_section_reaches_resolved_config_and_leaves_the_detection_digest() {
    let resolve = |json: &str| {
        let authored: FallowConfig = serde_json::from_str(json).expect("config deserializes");
        authored.resolve(
            PathBuf::from("/project"),
            OutputFormat::Json,
            1,
            true,
            true,
            None,
        )
    };
    let plain = resolve("{}");
    let with_mutation = resolve(r#"{"mutation":{"minScore":75,"operators":["removed-call"]}}"#);
    assert_eq!(with_mutation.mutation.min_score, Some(75.0));
    assert_eq!(
        with_mutation.mutation.operators,
        Some(vec![crate::MutationOperatorName::RemovedCall])
    );
    assert_eq!(
        plain.detection_config_digest,
        with_mutation.detection_config_digest
    );
    let detecting = resolve(r#"{"ignorePatterns":["gen/**"]}"#);
    assert_ne!(
        plain.detection_config_digest,
        detecting.detection_config_digest
    );
}

/// A single `*` in a config path glob stays within one path segment.
/// Only `**` crosses directories.
#[test]
fn single_star_stays_within_one_segment_in_config_path_globs() {
    let authored: FallowConfig = serde_json::from_str(
            r#"{
                "ignorePatterns": ["gen/*.ts"],
                "ignoreFindings": ["legacy/*.ts"],
                "overrides": [{ "files": ["old/*.ts"], "rules": { "unused-files": "off" } }],
                "ignoreExports": [{ "file": "api/*.ts", "exports": ["*"] }],
                "ignoreCatalogReferences": [{ "package": "react", "consumer": "apps/*/package.json" }],
                "boundaries": {
                    "zones": [{ "name": "ui", "patterns": ["ui/*.ts"] }],
                    "coverage": { "allowUnmatched": ["scripts/*.ts"] }
                }
            }"#,
        )
        .expect("config deserializes");
    let resolved = authored.resolve(
        PathBuf::from("/project"),
        OutputFormat::Json,
        1,
        true,
        true,
        None,
    );

    assert!(resolved.ignore_patterns.is_match("gen/a.ts"));
    assert!(!resolved.ignore_patterns.is_match("gen/deep/a.ts"));

    assert!(resolved.ignore_findings.is_ignored("legacy/a.ts"));
    assert!(!resolved.ignore_findings.is_ignored("legacy/deep/a.ts"));

    let off = |path: &str| {
        resolved
            .resolve_rules_for_path(Path::new(path))
            .unused_files
            == Severity::Off
    };
    assert!(off("/project/old/a.ts"));
    assert!(!off("/project/old/deep/a.ts"));

    let export_rule = &resolved.compiled_ignore_exports[0].matcher;
    assert!(export_rule.is_match("api/a.ts"));
    assert!(!export_rule.is_match("api/deep/a.ts"));

    let catalog_rule = &resolved.compiled_ignore_catalog_references[0];
    assert!(catalog_rule.matches("react", "default", "apps/web/package.json"));
    assert!(!catalog_rule.matches("react", "default", "apps/web/nested/package.json"));

    assert_eq!(resolved.boundaries.classify_zone("ui/a.ts"), Some("ui"));
    assert_eq!(resolved.boundaries.classify_zone("ui/deep/a.ts"), None);

    let allow_unmatched = &resolved.boundaries.coverage.allow_unmatched[0];
    assert!(allow_unmatched.is_match("scripts/a.ts"));
    assert!(!allow_unmatched.is_match("scripts/deep/a.ts"));
}

#[test]
fn workspace_patterns_reach_resolved_config() {
    let authored: FallowConfig =
        serde_json::from_str(r#"{"workspaces":{"patterns":["apps/*/*"]}}"#)
            .expect("workspace patterns deserialize");
    let resolved = authored.resolve(
        PathBuf::from("/project"),
        OutputFormat::Json,
        1,
        true,
        true,
        None,
    );

    assert_eq!(resolved.workspace_patterns, vec!["apps/*/*".to_string()]);
}

#[test]
fn workspace_changed_since_reaches_resolved_config() {
    let authored: FallowConfig = serde_json::from_str(
            r#"{"workspaces":{"changedSince":{"packages/web":"main","packages/legacy":"release/2024.10"}}}"#,
        )
        .expect("workspace baselines deserialize");
    let resolved = authored.resolve(
        PathBuf::from("/project"),
        OutputFormat::Json,
        1,
        true,
        true,
        None,
    );

    assert_eq!(resolved.workspace_changed_since.len(), 2);
    assert_eq!(resolved.workspace_changed_since["packages/web"], "main");
    assert_eq!(
        resolved.workspace_changed_since["packages/legacy"],
        "release/2024.10"
    );
}

#[test]
fn cache_config_hash_keys_on_user_flag_patterns() {
    let builtin = cache_config_hash(&[], &FlagsConfig::default());
    let with_pattern = |function: &str| FlagsConfig {
        sdk_patterns: vec![super::super::flags::SdkPattern {
            function: function.to_string(),
            name_arg: 0,
            provider: None,
        }],
        ..FlagsConfig::default()
    };

    assert_eq!(
        builtin,
        xxhash_rust::xxh3::Xxh3::new().digest(),
        "a config without flag patterns keeps the plugin-only hash"
    );
    assert_ne!(builtin, cache_config_hash(&[], &with_pattern("isOn")));
    assert_ne!(
        cache_config_hash(&[], &with_pattern("isOn")),
        cache_config_hash(&[], &with_pattern("isOff"))
    );
    assert_ne!(
        builtin,
        cache_config_hash(
            &[],
            &FlagsConfig {
                config_object_heuristics: true,
                ..FlagsConfig::default()
            }
        )
    );
    assert_ne!(
        builtin,
        cache_config_hash(
            &[],
            &FlagsConfig {
                env_prefixes: vec!["MYAPP_".to_string()],
                ..FlagsConfig::default()
            }
        )
    );
}

#[test]
fn overrides_deserialize() {
    let json_str = r#"{
            "overrides": [{
                "files": ["*.test.ts"],
                "rules": {
                    "unused-exports": "off"
                }
            }]
        }"#;
    let config: FallowConfig = serde_json::from_str(json_str).unwrap();
    assert_eq!(config.overrides.len(), 1);
    assert_eq!(config.overrides[0].files, vec!["*.test.ts"]);
    assert_eq!(
        config.overrides[0].rules.unused_exports,
        Some(Severity::Off)
    );
    assert_eq!(config.overrides[0].rules.unused_files, None);
}

#[test]
fn resolve_rules_for_path_no_overrides() {
    let config = FallowConfig {
        schema: None,
        minimum_version: None,
        extends: vec![],
        entry: vec![],
        ignore_patterns: vec![],
        ignore_findings: vec![],
        framework: vec![],
        workspaces: None,
        ignore_dependencies: vec![],
        ignore_command_entries: vec![],
        ignore_unresolved_imports: vec![],
        ignore_exports: vec![],
        ignore_catalog_references: vec![],
        ignore_dependency_overrides: vec![],
        ignore_exports_used_in_file: IgnoreExportsUsedInFileConfig::default(),
        used_class_members: vec![],
        ignore_decorators: vec![],
        unused_component_props: crate::UnusedComponentPropsConfig::default(),
        duplicates: DuplicatesConfig::default(),
        similar_code: SimilarCodeConfig::default(),
        mutation: MutationConfig::default(),
        health: HealthConfig::default(),
        rules: RulesConfig::default(),
        boundaries: BoundaryConfig::default(),
        production: false.into(),
        plugins: vec![],
        overrides: vec![],
        regression: None,
        type_aware: crate::TypeAwareConfig::default(),
        audit: crate::config::AuditConfig::default(),
        codeowners: None,
        public_packages: vec![],
        flags: FlagsConfig::default(),
        security: SecurityConfig::default(),
        fix: crate::config::FixConfig::default(),
        resolve: ResolveConfig::default(),
        sealed: false,
        include_entry_exports: false,
        show_cascade: false,
        auto_imports: false,
        fail_on_parse_error: false,
        cache: CacheConfig::default(),
        ..Default::default()
    };
    let resolved = config.resolve(
        PathBuf::from("/project"),
        OutputFormat::Human,
        1,
        true,
        true,
        None,
    );
    let rules = resolved.resolve_rules_for_path(Path::new("/project/src/foo.ts"));
    assert_eq!(rules.unused_files, Severity::Error);
}

#[test]
fn resolve_rules_for_path_with_matching_override() {
    let config = FallowConfig {
        schema: None,
        minimum_version: None,
        extends: vec![],
        entry: vec![],
        ignore_patterns: vec![],
        ignore_findings: vec![],
        framework: vec![],
        workspaces: None,
        ignore_dependencies: vec![],
        ignore_command_entries: vec![],
        ignore_unresolved_imports: vec![],
        ignore_exports: vec![],
        ignore_catalog_references: vec![],
        ignore_dependency_overrides: vec![],
        ignore_exports_used_in_file: IgnoreExportsUsedInFileConfig::default(),
        used_class_members: vec![],
        ignore_decorators: vec![],
        unused_component_props: crate::UnusedComponentPropsConfig::default(),
        duplicates: DuplicatesConfig::default(),
        similar_code: SimilarCodeConfig::default(),
        mutation: MutationConfig::default(),
        health: HealthConfig::default(),
        rules: RulesConfig::default(),
        boundaries: BoundaryConfig::default(),
        production: false.into(),
        plugins: vec![],
        overrides: vec![ConfigOverride {
            files: vec!["**/*.test.ts".to_string()],
            rules: PartialRulesConfig {
                unused_exports: Some(Severity::Off),
                ..Default::default()
            },
        }],
        regression: None,
        type_aware: crate::TypeAwareConfig::default(),
        audit: crate::config::AuditConfig::default(),
        codeowners: None,
        public_packages: vec![],
        flags: FlagsConfig::default(),
        security: SecurityConfig::default(),
        fix: crate::config::FixConfig::default(),
        resolve: ResolveConfig::default(),
        sealed: false,
        include_entry_exports: false,
        show_cascade: false,
        auto_imports: false,
        fail_on_parse_error: false,
        cache: CacheConfig::default(),
        ..Default::default()
    };
    let resolved = config.resolve(
        PathBuf::from("/project"),
        OutputFormat::Human,
        1,
        true,
        true,
        None,
    );

    let test_rules = resolved.resolve_rules_for_path(Path::new("/project/src/utils.test.ts"));
    assert_eq!(test_rules.unused_exports, Severity::Off);
    assert_eq!(test_rules.unused_files, Severity::Error); // not overridden

    let src_rules = resolved.resolve_rules_for_path(Path::new("/project/src/utils.ts"));
    assert_eq!(src_rules.unused_exports, Severity::Error);
}

#[test]
fn resolve_rules_for_path_later_override_wins() {
    let config = FallowConfig {
        schema: None,
        minimum_version: None,
        extends: vec![],
        entry: vec![],
        ignore_patterns: vec![],
        ignore_findings: vec![],
        framework: vec![],
        workspaces: None,
        ignore_dependencies: vec![],
        ignore_command_entries: vec![],
        ignore_unresolved_imports: vec![],
        ignore_exports: vec![],
        ignore_catalog_references: vec![],
        ignore_dependency_overrides: vec![],
        ignore_exports_used_in_file: IgnoreExportsUsedInFileConfig::default(),
        used_class_members: vec![],
        ignore_decorators: vec![],
        unused_component_props: crate::UnusedComponentPropsConfig::default(),
        duplicates: DuplicatesConfig::default(),
        similar_code: SimilarCodeConfig::default(),
        mutation: MutationConfig::default(),
        health: HealthConfig::default(),
        rules: RulesConfig::default(),
        boundaries: BoundaryConfig::default(),
        production: false.into(),
        plugins: vec![],
        overrides: vec![
            ConfigOverride {
                files: vec!["*.ts".to_string()],
                rules: PartialRulesConfig {
                    unused_files: Some(Severity::Warn),
                    ..Default::default()
                },
            },
            ConfigOverride {
                files: vec!["*.test.ts".to_string()],
                rules: PartialRulesConfig {
                    unused_files: Some(Severity::Off),
                    ..Default::default()
                },
            },
        ],
        regression: None,
        type_aware: crate::TypeAwareConfig::default(),
        audit: crate::config::AuditConfig::default(),
        codeowners: None,
        public_packages: vec![],
        flags: FlagsConfig::default(),
        security: SecurityConfig::default(),
        fix: crate::config::FixConfig::default(),
        resolve: ResolveConfig::default(),
        sealed: false,
        include_entry_exports: false,
        show_cascade: false,
        auto_imports: false,
        fail_on_parse_error: false,
        cache: CacheConfig::default(),
        ..Default::default()
    };
    let resolved = config.resolve(
        PathBuf::from("/project"),
        OutputFormat::Human,
        1,
        true,
        true,
        None,
    );

    let rules = resolved.resolve_rules_for_path(Path::new("/project/foo.test.ts"));
    assert_eq!(rules.unused_files, Severity::Off);

    let rules2 = resolved.resolve_rules_for_path(Path::new("/project/foo.ts"));
    assert_eq!(rules2.unused_files, Severity::Warn);
}

#[test]
fn resolve_keeps_a_project_level_rule_in_its_override_entry() {
    let config = FallowConfig {
        schema: None,
        minimum_version: None,
        extends: vec![],
        entry: vec![],
        ignore_patterns: vec![],
        ignore_findings: vec![],
        framework: vec![],
        workspaces: None,
        ignore_dependencies: vec![],
        ignore_command_entries: vec![],
        ignore_unresolved_imports: vec![],
        ignore_exports: vec![],
        ignore_catalog_references: vec![],
        ignore_dependency_overrides: vec![],
        ignore_exports_used_in_file: IgnoreExportsUsedInFileConfig::default(),
        used_class_members: vec![],
        ignore_decorators: vec![],
        unused_component_props: crate::UnusedComponentPropsConfig::default(),
        duplicates: DuplicatesConfig::default(),
        similar_code: SimilarCodeConfig::default(),
        mutation: MutationConfig::default(),
        health: HealthConfig::default(),
        rules: RulesConfig::default(),
        boundaries: BoundaryConfig::default(),
        production: false.into(),
        plugins: vec![],
        overrides: vec![ConfigOverride {
            files: vec!["**/ui/**".to_string()],
            rules: PartialRulesConfig {
                duplicate_exports: Some(Severity::Off),
                unused_files: Some(Severity::Warn),
                ..Default::default()
            },
        }],
        regression: None,
        type_aware: crate::TypeAwareConfig::default(),
        audit: crate::config::AuditConfig::default(),
        codeowners: None,
        public_packages: vec![],
        flags: FlagsConfig::default(),
        security: SecurityConfig::default(),
        fix: crate::config::FixConfig::default(),
        resolve: ResolveConfig::default(),
        sealed: false,
        include_entry_exports: false,
        show_cascade: false,
        auto_imports: false,
        fail_on_parse_error: false,
        cache: CacheConfig::default(),
        ..Default::default()
    };
    let resolved = config.resolve(
        PathBuf::from("/project"),
        OutputFormat::Human,
        1,
        true,
        true,
        None,
    );
    assert_eq!(
        resolved.overrides.len(),
        1,
        "a project-level rule must not drop the override; co-located file-scoped rules still apply"
    );
    let rules = resolved.resolve_rules_for_path(Path::new("/project/ui/dialog.ts"));
    assert_eq!(rules.unused_files, Severity::Warn);
}

/// Helper to build a FallowConfig with minimal boilerplate.
fn make_config(production: bool) -> FallowConfig {
    FallowConfig {
        schema: None,
        minimum_version: None,
        extends: vec![],
        entry: vec![],
        ignore_patterns: vec![],
        ignore_findings: vec![],
        framework: vec![],
        workspaces: None,
        ignore_dependencies: vec![],
        ignore_command_entries: vec![],
        ignore_unresolved_imports: vec![],
        ignore_exports: vec![],
        ignore_catalog_references: vec![],
        ignore_dependency_overrides: vec![],
        ignore_exports_used_in_file: IgnoreExportsUsedInFileConfig::default(),
        used_class_members: vec![],
        ignore_decorators: vec![],
        unused_component_props: crate::UnusedComponentPropsConfig::default(),
        duplicates: DuplicatesConfig::default(),
        similar_code: SimilarCodeConfig::default(),
        mutation: MutationConfig::default(),
        health: HealthConfig::default(),
        rules: RulesConfig::default(),
        boundaries: BoundaryConfig::default(),
        production: production.into(),
        plugins: vec![],
        overrides: vec![],
        regression: None,
        type_aware: crate::TypeAwareConfig::default(),
        audit: crate::config::AuditConfig::default(),
        codeowners: None,
        public_packages: vec![],
        flags: FlagsConfig::default(),
        security: SecurityConfig::default(),
        fix: crate::config::FixConfig::default(),
        resolve: ResolveConfig::default(),
        sealed: false,
        include_entry_exports: false,
        show_cascade: false,
        auto_imports: false,
        fail_on_parse_error: false,
        cache: CacheConfig::default(),
        ..Default::default()
    }
}

#[test]
fn resolve_tracks_rule_pack_sources_in_config_order() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("rule-packs")).unwrap();
    std::fs::write(
        dir.path().join("rule-packs/team-policy.jsonc"),
        r#"{
  "version": 1,
  "name": "team-policy",
  "rules": [
    {
      "id": "no-moment",
      "kind": "banned-import",
      "specifiers": ["moment"]
    }
  ]
}
"#,
    )
    .unwrap();

    let mut config = make_config(false);
    config.rule_packs = vec!["rule-packs/team-policy.jsonc".to_string()];

    let resolved = config.resolve(
        dir.path().to_path_buf(),
        OutputFormat::Human,
        1,
        true,
        true,
        None,
    );

    assert_eq!(resolved.rule_packs.len(), 1);
    assert_eq!(resolved.rule_packs[0].name, "team-policy");
    assert_eq!(
        resolved.rule_pack_sources,
        vec![PathBuf::from("rule-packs/team-policy.jsonc")]
    );
}

#[test]
fn resolve_production_forces_dev_deps_off() {
    let resolved = make_config(true).resolve(
        PathBuf::from("/project"),
        OutputFormat::Human,
        1,
        true,
        true,
        None,
    );
    assert_eq!(
        resolved.rules.unused_dev_dependencies,
        Severity::Off,
        "production mode should force unused_dev_dependencies to off"
    );
}

#[test]
fn resolve_production_forces_optional_deps_off() {
    let resolved = make_config(true).resolve(
        PathBuf::from("/project"),
        OutputFormat::Human,
        1,
        true,
        true,
        None,
    );
    assert_eq!(
        resolved.rules.unused_optional_dependencies,
        Severity::Off,
        "production mode should force unused_optional_dependencies to off"
    );
}

#[test]
fn resolve_production_preserves_other_rules() {
    let resolved = make_config(true).resolve(
        PathBuf::from("/project"),
        OutputFormat::Human,
        1,
        true,
        true,
        None,
    );
    assert_eq!(resolved.rules.unused_files, Severity::Error);
    assert_eq!(resolved.rules.unused_exports, Severity::Error);
    assert_eq!(resolved.rules.unused_dependencies, Severity::Error);
}

#[test]
fn resolve_non_production_keeps_dev_deps_default() {
    let resolved = make_config(false).resolve(
        PathBuf::from("/project"),
        OutputFormat::Human,
        1,
        true,
        true,
        None,
    );
    assert_eq!(
        resolved.rules.unused_dev_dependencies,
        Severity::Warn,
        "non-production should keep default severity"
    );
    assert_eq!(resolved.rules.unused_optional_dependencies, Severity::Warn);
}

#[test]
fn resolve_production_flag_stored() {
    let resolved = make_config(true).resolve(
        PathBuf::from("/project"),
        OutputFormat::Human,
        1,
        true,
        true,
        None,
    );
    assert!(resolved.production);

    let resolved2 = make_config(false).resolve(
        PathBuf::from("/project"),
        OutputFormat::Human,
        1,
        true,
        true,
        None,
    );
    assert!(!resolved2.production);
}

#[test]
fn resolve_default_ignores_node_modules() {
    let resolved = make_config(false).resolve(
        PathBuf::from("/project"),
        OutputFormat::Human,
        1,
        true,
        true,
        None,
    );
    assert!(
        resolved
            .ignore_patterns
            .is_match("node_modules/lodash/index.js")
    );
    assert!(
        resolved
            .ignore_patterns
            .is_match("packages/a/node_modules/react/index.js")
    );
}

#[test]
fn resolve_default_ignores_dist() {
    let resolved = make_config(false).resolve(
        PathBuf::from("/project"),
        OutputFormat::Human,
        1,
        true,
        true,
        None,
    );
    assert!(resolved.ignore_patterns.is_match("dist/bundle.js"));
    assert!(
        resolved
            .ignore_patterns
            .is_match("packages/ui/dist/index.js")
    );
}

/// Issue #2638: source discovery maps a `GlobSet` match index back to the
/// pattern text through this layout, so the split point and the order of
/// the built-ins are an output contract, not an implementation detail.
/// A future reordering of `compile_ignore_patterns` fails here first.
#[test]
fn the_compiled_ignore_union_puts_user_patterns_first_and_the_built_ins_in_order() {
    let mut config = make_config(false);
    config.ignore_patterns = vec!["vendor/**".to_owned(), "legacy/**".to_owned()];
    let resolved = config.resolve(
        PathBuf::from("/project"),
        OutputFormat::Human,
        1,
        true,
        true,
        None,
    );

    assert_eq!(resolved.user_ignore_pattern_count, 2);
    assert_eq!(
        resolved.ignore_patterns.len(),
        2 + DEFAULT_IGNORE_PATTERNS.len(),
        "the union only ever adds"
    );
    assert_eq!(
        resolved.ignore_patterns.matches("vendor/a.ts"),
        vec![0],
        "a user pattern keeps its configured index"
    );
    for (offset, pattern) in DEFAULT_IGNORE_PATTERNS.iter().enumerate() {
        let sample = match *pattern {
            "**/node_modules/**" => "node_modules/react/index.js",
            "**/dist/**" => "dist/a.ts",
            "**/build/**" => "build/a.ts",
            "**/.git/**" => ".git/hooks/a.js",
            "**/coverage/**" => "coverage/a.ts",
            "**/*.min.js" => "a.min.js",
            "**/*.min.mjs" => "a.min.mjs",
            "**/*.min.cjs" => "a.min.cjs",
            "**/*.bundle.js" => "a.bundle.js",
            other => panic!("no sample path for built-in ignore {other}"),
        };
        assert_eq!(
            resolved.ignore_patterns.matches(sample),
            vec![resolved.user_ignore_pattern_count + offset],
            "{pattern} must sit at its DEFAULT_IGNORE_PATTERNS offset"
        );
    }
}

/// A file both a user pattern and a built-in match reports the user index
/// first, which is how discovery tells an explicit project choice from a
/// built-in the user never asked for (issue #2638).
#[test]
fn a_user_pattern_that_overlaps_a_built_in_matches_at_the_lower_index() {
    let mut config = make_config(false);
    config.ignore_patterns = vec!["dist/**".to_owned()];
    let resolved = config.resolve(
        PathBuf::from("/project"),
        OutputFormat::Human,
        1,
        true,
        true,
        None,
    );

    let matches = resolved.ignore_patterns.matches("dist/a.ts");
    assert_eq!(matches.first(), Some(&0));
    assert!(matches.len() > 1, "the built-in still matches too");
}

#[test]
fn resolve_default_ignores_build_at_any_depth() {
    let resolved = make_config(false).resolve(
        PathBuf::from("/project"),
        OutputFormat::Human,
        1,
        true,
        true,
        None,
    );
    assert!(
        resolved.ignore_patterns.is_match("build/output.js"),
        "root build/ should be ignored"
    );
    assert!(
        resolved.ignore_patterns.is_match("src/build/helper.ts"),
        "nested build/ should be ignored, like dist/ and coverage/"
    );
    assert!(
        resolved
            .ignore_patterns
            .is_match("projects/app/build/index.js"),
        "build/ inside a workspace package should be ignored"
    );
}

#[test]
fn resolve_default_ignores_match_build_only_as_a_whole_segment() {
    let resolved = make_config(false).resolve(
        PathBuf::from("/project"),
        OutputFormat::Human,
        1,
        true,
        true,
        None,
    );
    assert!(!resolved.ignore_patterns.is_match("src/build.ts"));
    assert!(!resolved.ignore_patterns.is_match("src/rebuild/helper.ts"));
    assert!(!resolved.ignore_patterns.is_match("src/buildings/a.ts"));
    assert!(!resolved.ignore_patterns.is_match("src/prebuild/a.ts"));
}

/// A workspace package directory named `build` keeps its entry in workspace
/// discovery, because a declared member holding a manifest survives
/// `ignorePatterns`, but everything inside it is filtered out: the source
/// files never reach the walker in `crates/core/src/discover/walk.rs`, and
/// the manifest never reaches the dependency filter in
/// `crates/core/src/analyze/unused_deps.rs`. Both consumers read this one
/// globset, so pin both paths here and keep the consequence a deliberate
/// choice rather than a documentation guess.
#[test]
fn resolve_default_ignores_cover_a_workspace_package_named_build() {
    let resolved = make_config(false).resolve(
        PathBuf::from("/project"),
        OutputFormat::Human,
        1,
        true,
        true,
        None,
    );
    assert!(
        resolved
            .ignore_patterns
            .is_match("packages/build/package.json"),
        "the manifest stops contributing unused-dependency findings"
    );
    assert!(
        resolved
            .ignore_patterns
            .is_match("packages/build/src/index.ts"),
        "the package's source stops being analyzed entirely"
    );
    assert!(
        resolved
            .ignore_patterns
            .is_match("packages/build/src/nested/deep.ts"),
        "including source below the package's own subdirectories"
    );
}

/// A framework config inside a nested `build/` directory is filtered out of
/// discovery with everything else under the segment, so the path aliases it
/// declares are lost and imports through them are reported as unlisted
/// dependencies. Pinned so that consequence is a recorded choice.
#[test]
fn resolve_default_ignores_cover_a_framework_config_inside_build() {
    let resolved = make_config(false).resolve(
        PathBuf::from("/project"),
        OutputFormat::Human,
        1,
        true,
        true,
        None,
    );
    assert!(
        resolved
            .ignore_patterns
            .is_match("app/build/webpack.config.js")
    );
}

#[test]
fn resolve_default_ignores_minified_files() {
    let resolved = make_config(false).resolve(
        PathBuf::from("/project"),
        OutputFormat::Human,
        1,
        true,
        true,
        None,
    );
    assert!(resolved.ignore_patterns.is_match("vendor/jquery.min.js"));
    assert!(resolved.ignore_patterns.is_match("lib/utils.min.mjs"));
    assert!(resolved.ignore_patterns.is_match("lib/legacy.min.cjs"));
    assert!(resolved.ignore_patterns.is_match("public/app.bundle.js"));
    assert!(
        resolved
            .ignore_patterns
            .is_match("src/vendor/app.bundle.js")
    );
    // Hand-written source with a similar name stays analyzed.
    assert!(!resolved.ignore_patterns.is_match("src/bundle.ts"));
    assert!(!resolved.ignore_patterns.is_match("src/app.cjs"));
}

#[test]
fn resolve_max_file_size_bytes_default_and_unlimited() {
    // Unset keeps the built-in default.
    assert_eq!(
        resolve_max_file_size_bytes(None),
        Some(DEFAULT_MAX_FILE_SIZE_BYTES)
    );
    // `0` means no limit.
    assert_eq!(resolve_max_file_size_bytes(Some(0)), None);
    // Any other value is that many megabytes in bytes.
    assert_eq!(resolve_max_file_size_bytes(Some(2)), Some(2 * 1024 * 1024));
    assert_eq!(DEFAULT_MAX_FILE_SIZE_MB, 5);
}

#[test]
fn resolve_sets_default_max_file_size() {
    let resolved = make_config(false).resolve(
        PathBuf::from("/project"),
        OutputFormat::Human,
        1,
        true,
        true,
        None,
    );
    assert_eq!(
        resolved.max_file_size_bytes,
        Some(DEFAULT_MAX_FILE_SIZE_BYTES)
    );
}

#[test]
fn resolve_default_ignores_git() {
    let resolved = make_config(false).resolve(
        PathBuf::from("/project"),
        OutputFormat::Human,
        1,
        true,
        true,
        None,
    );
    assert!(resolved.ignore_patterns.is_match(".git/objects/ab/123.js"));
}

#[test]
fn resolve_default_ignores_coverage() {
    let resolved = make_config(false).resolve(
        PathBuf::from("/project"),
        OutputFormat::Human,
        1,
        true,
        true,
        None,
    );
    assert!(
        resolved
            .ignore_patterns
            .is_match("coverage/lcov-report/index.js")
    );
}

#[test]
fn resolve_source_files_not_ignored_by_default() {
    let resolved = make_config(false).resolve(
        PathBuf::from("/project"),
        OutputFormat::Human,
        1,
        true,
        true,
        None,
    );
    assert!(!resolved.ignore_patterns.is_match("src/index.ts"));
    assert!(
        !resolved
            .ignore_patterns
            .is_match("src/components/Button.tsx")
    );
    assert!(!resolved.ignore_patterns.is_match("lib/utils.js"));
}

#[test]
fn resolve_custom_ignore_patterns_merged_with_defaults() {
    let mut config = make_config(false);
    config.ignore_patterns = vec!["**/__generated__/**".to_string()];
    let resolved = config.resolve(
        PathBuf::from("/project"),
        OutputFormat::Human,
        1,
        true,
        true,
        None,
    );
    assert!(
        resolved
            .ignore_patterns
            .is_match("src/__generated__/types.ts")
    );
    assert!(resolved.ignore_patterns.is_match("node_modules/foo/bar.js"));
}

#[test]
fn resolve_normalizes_leading_dot_ignore_patterns() {
    let mut config = make_config(false);
    config.ignore_patterns = vec!["./src/generated/**".to_string()];
    let resolved = config.resolve(
        PathBuf::from("/project"),
        OutputFormat::Human,
        1,
        true,
        true,
        None,
    );

    assert!(resolved.ignore_patterns.is_match("src/generated/client.ts"));
    assert!(
        !resolved
            .ignore_patterns
            .is_match("./src/generated/client.ts")
    );
}

#[test]
fn resolve_normalizes_leading_dot_ignore_unresolved_imports() {
    let mut config = make_config(false);
    config.ignore_unresolved_imports = vec!["./src/generated/**".to_string()];
    let resolved = config.resolve(
        PathBuf::from("/project"),
        OutputFormat::Human,
        1,
        true,
        true,
        None,
    );

    assert!(
        resolved
            .ignore_unresolved_imports
            .iter()
            .any(|matcher| matcher.is_match("src/generated/client"))
    );
    assert!(
        !resolved
            .ignore_unresolved_imports
            .iter()
            .any(|matcher| matcher.is_match("./src/generated/client"))
    );
}

#[test]
fn resolve_passes_through_entry_patterns() {
    let mut config = make_config(false);
    config.entry = vec!["src/**/*.ts".to_string(), "lib/**/*.js".to_string()];
    let resolved = config.resolve(
        PathBuf::from("/project"),
        OutputFormat::Human,
        1,
        true,
        true,
        None,
    );
    assert_eq!(resolved.entry_patterns, vec!["src/**/*.ts", "lib/**/*.js"]);
}

#[test]
fn resolve_passes_through_ignore_dependencies() {
    let mut config = make_config(false);
    config.ignore_dependencies = vec!["postcss".to_string(), "autoprefixer".to_string()];
    let resolved = config.resolve(
        PathBuf::from("/project"),
        OutputFormat::Human,
        1,
        true,
        true,
        None,
    );
    assert!(resolved.ignore_dependencies.is_ignored("postcss"));
    assert!(resolved.ignore_dependencies.is_ignored("autoprefixer"));
    assert!(!resolved.ignore_dependencies.is_ignored("postcss-cli"));
}

#[test]
fn resolve_passes_through_ignore_command_entries() {
    let mut config = make_config(false);
    config.ignore_command_entries = vec!["my-codegen".to_string()];
    let resolved = config.resolve(
        PathBuf::from("/project"),
        OutputFormat::Human,
        1,
        true,
        true,
        None,
    );
    assert_eq!(resolved.ignore_command_entries, vec!["my-codegen"]);
}

#[test]
fn resolve_compiles_ignore_unresolved_imports_as_raw_specifier_globs() {
    let mut config = make_config(false);
    config.ignore_unresolved_imports = vec![
        "@example/icons".to_string(),
        "@example/icons/**".to_string(),
        "../generated/**".to_string(),
    ];
    let resolved = config.resolve(
        PathBuf::from("/project"),
        OutputFormat::Human,
        1,
        true,
        true,
        None,
    );

    assert!(
        resolved
            .ignore_unresolved_imports
            .iter()
            .any(|matcher| matcher.is_match("@example/icons"))
    );
    assert!(
        resolved
            .ignore_unresolved_imports
            .iter()
            .any(|matcher| matcher.is_match("@example/icons/metadata"))
    );
    assert!(
        resolved
            .ignore_unresolved_imports
            .iter()
            .any(|matcher| matcher.is_match("../generated/client"))
    );
}

#[test]
fn ignore_unresolved_imports_subpath_glob_does_not_match_bare_specifier() {
    let mut config = make_config(false);
    config.ignore_unresolved_imports = vec!["@example/icons/**".to_string()];
    let resolved = config.resolve(
        PathBuf::from("/project"),
        OutputFormat::Human,
        1,
        true,
        true,
        None,
    );

    assert!(
        !resolved.ignore_unresolved_imports[0].is_match("@example/icons"),
        "globset treats @example/icons/** as subpaths only; list the bare specifier separately"
    );
    assert!(resolved.ignore_unresolved_imports[0].is_match("@example/icons/metadata"));
}

#[test]
fn resolve_sets_cache_dir() {
    let resolved = make_config(false).resolve(
        PathBuf::from("/my/project"),
        OutputFormat::Human,
        1,
        true,
        true,
        None,
    );
    assert_eq!(resolved.cache_dir, PathBuf::from("/my/project/.fallow"));
}

#[test]
fn resolve_uses_relative_configured_cache_dir_from_root() {
    let config = FallowConfig {
        cache: crate::CacheConfig {
            dir: Some(PathBuf::from(".cache/fallow")),
            ..Default::default()
        },
        ..make_config(false)
    };
    let resolved = config.resolve(
        PathBuf::from("/my/project"),
        OutputFormat::Human,
        1,
        false,
        true,
        None,
    );
    assert_eq!(
        resolved.cache_dir,
        PathBuf::from("/my/project/.cache/fallow")
    );
}

#[test]
fn a_configured_cache_dir_outside_the_project_is_ignored() {
    for dir in [
        "/home/user",
        "../shared",
        "sub/../..",
        ".",
        "",
        ".git/hooks",
    ] {
        assert_eq!(
            config_cache_dir(Path::new("/my/project"), Some(PathBuf::from(dir))),
            PathBuf::from("/my/project/.fallow"),
            "{dir}"
        );
    }
}

#[test]
fn a_relative_root_does_not_accept_an_outside_cache_dir() {
    let dir = config_cache_dir(Path::new("."), Some(PathBuf::from("/home/user")));
    assert!(dir.ends_with(".fallow"), "{}", dir.display());
}

#[test]
fn a_configured_cache_dir_inside_the_project_is_kept() {
    for (dir, expected) in [
        (".cache/fallow", "/my/project/.cache/fallow"),
        ("/my/project/build/cache", "/my/project/build/cache"),
        ("sub/../cache", "/my/project/cache"),
    ] {
        assert_eq!(
            config_cache_dir(Path::new("/my/project"), Some(PathBuf::from(dir))),
            PathBuf::from(expected),
            "{dir}"
        );
    }
}

#[test]
fn an_absolute_cache_dir_still_comes_from_the_environment() {
    let config = FallowConfig {
        cache: crate::CacheConfig {
            dir: Some(PathBuf::from("/home/user")),
            ..Default::default()
        },
        ..make_config(false)
    };
    let mut resolved = config.resolve(
        PathBuf::from("/my/project"),
        OutputFormat::Human,
        1,
        false,
        true,
        None,
    );
    assert_eq!(resolved.cache_dir, PathBuf::from("/my/project/.fallow"));
    resolved.override_cache_dir(PathBuf::from("/tmp/fallow-cache"));
    assert_eq!(resolved.cache_dir, PathBuf::from("/tmp/fallow-cache"));
}

#[test]
fn cache_dir_override_wins_over_configured_cache_dir() {
    let config = FallowConfig {
        cache: crate::CacheConfig {
            dir: Some(PathBuf::from(".cache/from-config")),
            ..Default::default()
        },
        ..make_config(false)
    };
    let mut resolved = config.resolve(
        PathBuf::from("/my/project"),
        OutputFormat::Human,
        1,
        false,
        true,
        None,
    );

    resolved.override_cache_dir(PathBuf::from(".cache/from-env"));
    assert_eq!(
        resolved.cache_dir,
        PathBuf::from("/my/project/.cache/from-env")
    );
    resolved.override_cache_dir(PathBuf::from("/tmp/fallow-cache"));
    assert_eq!(resolved.cache_dir, PathBuf::from("/tmp/fallow-cache"));
}

fn resolved_with_cache_dir(root: &str, cache_dir: &str) -> ResolvedConfig {
    let mut resolved = make_config(false).resolve(
        PathBuf::from(root),
        OutputFormat::Human,
        1,
        false,
        true,
        None,
    );
    resolved.override_cache_dir(PathBuf::from(cache_dir));
    resolved
}

#[test]
fn scoped_shared_cache_dir_gives_each_root_its_own_subdirectory() {
    let mut first = resolved_with_cache_dir("/work/app", "/var/cache/fallow");
    let mut second = resolved_with_cache_dir("/other/app", "/var/cache/fallow");
    first.scope_shared_cache_dir_to_root();
    second.scope_shared_cache_dir_to_root();

    assert_eq!(
        first.cache_dir.parent(),
        Some(Path::new("/var/cache/fallow"))
    );
    assert_eq!(
        second.cache_dir.parent(),
        Some(Path::new("/var/cache/fallow"))
    );
    assert_ne!(
        first.cache_dir, second.cache_dir,
        "two roots with the same folder name must not share a cache"
    );
    let name = first
        .cache_dir
        .file_name()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    assert!(name.starts_with("app-"), "the folder name leads: {name}");
}

#[test]
fn scoped_cache_dir_is_stable_for_one_root() {
    let mut first = resolved_with_cache_dir("/work/app", "/var/cache/fallow");
    let mut again = resolved_with_cache_dir("/work/app", "/var/cache/fallow");
    first.scope_shared_cache_dir_to_root();
    again.scope_shared_cache_dir_to_root();
    assert_eq!(first.cache_dir, again.cache_dir);
}

#[test]
fn scoping_keeps_a_cache_dir_inside_the_root() {
    let mut relative = resolved_with_cache_dir("/work/app", ".cache/fallow");
    relative.scope_shared_cache_dir_to_root();
    assert_eq!(relative.cache_dir, PathBuf::from("/work/app/.cache/fallow"));

    let mut absolute = resolved_with_cache_dir("/work/app", "/work/app/.fallow");
    absolute.scope_shared_cache_dir_to_root();
    assert_eq!(absolute.cache_dir, PathBuf::from("/work/app/.fallow"));
}

#[test]
fn resolve_passes_through_thread_count() {
    let resolved = make_config(false).resolve(
        PathBuf::from("/project"),
        OutputFormat::Human,
        8,
        true,
        true,
        None,
    );
    assert_eq!(resolved.threads, 8);
}

#[test]
fn resolve_passes_through_quiet_flag() {
    let resolved = make_config(false).resolve(
        PathBuf::from("/project"),
        OutputFormat::Human,
        1,
        true,
        false,
        None,
    );
    assert!(!resolved.quiet);

    let resolved2 = make_config(false).resolve(
        PathBuf::from("/project"),
        OutputFormat::Human,
        1,
        true,
        true,
        None,
    );
    assert!(resolved2.quiet);
}

#[test]
fn resolve_passes_through_no_cache_flag() {
    let resolved_no_cache = make_config(false).resolve(
        PathBuf::from("/project"),
        OutputFormat::Human,
        1,
        true,
        true,
        None,
    );
    assert!(resolved_no_cache.no_cache);

    let resolved_with_cache = make_config(false).resolve(
        PathBuf::from("/project"),
        OutputFormat::Human,
        1,
        false,
        true,
        None,
    );
    assert!(!resolved_with_cache.no_cache);
}

#[test]
#[should_panic(expected = "validated at config load time")]
fn resolve_panics_on_unvalidated_invalid_override_glob() {
    let mut config = make_config(false);
    config.overrides = vec![ConfigOverride {
        files: vec!["[invalid".to_string()],
        rules: PartialRulesConfig {
            unused_files: Some(Severity::Off),
            ..Default::default()
        },
    }];
    let _ = config.resolve(
        PathBuf::from("/project"),
        OutputFormat::Human,
        1,
        true,
        true,
        None,
    );
}

#[test]
fn resolve_override_with_empty_files_skipped() {
    let mut config = make_config(false);
    config.overrides = vec![ConfigOverride {
        files: vec![],
        rules: PartialRulesConfig {
            unused_files: Some(Severity::Off),
            ..Default::default()
        },
    }];
    let resolved = config.resolve(
        PathBuf::from("/project"),
        OutputFormat::Human,
        1,
        true,
        true,
        None,
    );
    assert!(
        resolved.overrides.is_empty(),
        "override with no file patterns should be skipped"
    );
}

#[test]
fn resolve_multiple_valid_overrides() {
    let mut config = make_config(false);
    config.overrides = vec![
        ConfigOverride {
            files: vec!["*.test.ts".to_string()],
            rules: PartialRulesConfig {
                unused_exports: Some(Severity::Off),
                ..Default::default()
            },
        },
        ConfigOverride {
            files: vec!["*.stories.tsx".to_string()],
            rules: PartialRulesConfig {
                unused_files: Some(Severity::Off),
                ..Default::default()
            },
        },
    ];
    let resolved = config.resolve(
        PathBuf::from("/project"),
        OutputFormat::Human,
        1,
        true,
        true,
        None,
    );
    assert_eq!(resolved.overrides.len(), 2);
}

#[test]
fn ignore_export_rule_deserialize() {
    let json = r#"{"file": "src/types/*.ts", "exports": ["*"]}"#;
    let rule: IgnoreExportRule = serde_json::from_str(json).unwrap();
    assert_eq!(rule.file, "src/types/*.ts");
    assert_eq!(rule.exports, vec!["*"]);
}

#[test]
fn ignore_export_rule_specific_exports() {
    let json = r#"{"file": "src/constants.ts", "exports": ["FOO", "BAR", "BAZ"]}"#;
    let rule: IgnoreExportRule = serde_json::from_str(json).unwrap();
    assert_eq!(rule.exports.len(), 3);
    assert!(rule.exports.contains(&"FOO".to_string()));
}

mod proptests {
    use super::*;
    use proptest::prelude::*;

    fn arb_resolved_config(production: bool) -> ResolvedConfig {
        make_config(production).resolve(
            PathBuf::from("/project"),
            OutputFormat::Human,
            1,
            true,
            true,
            None,
        )
    }

    proptest! {
        /// Resolved config always has non-empty ignore patterns (defaults are always added).
        #[test]
        fn resolved_config_has_default_ignores(production in any::<bool>()) {
            let resolved = arb_resolved_config(production);
            prop_assert!(
                resolved.ignore_patterns.is_match("node_modules/foo/bar.js"),
                "Default ignore should match node_modules"
            );
            prop_assert!(
                resolved.ignore_patterns.is_match("dist/bundle.js"),
                "Default ignore should match dist"
            );
        }

        /// Production mode always forces dev and optional deps to Off.
        #[test]
        fn production_forces_dev_deps_off(_unused in Just(())) {
            let resolved = arb_resolved_config(true);
            prop_assert_eq!(
                resolved.rules.unused_dev_dependencies,
                Severity::Off,
                "Production should force unused_dev_dependencies off"
            );
            prop_assert_eq!(
                resolved.rules.unused_optional_dependencies,
                Severity::Off,
                "Production should force unused_optional_dependencies off"
            );
        }

        /// Non-production mode preserves default severity for dev deps.
        #[test]
        fn non_production_preserves_dev_deps_default(_unused in Just(())) {
            let resolved = arb_resolved_config(false);
            prop_assert_eq!(
                resolved.rules.unused_dev_dependencies,
                Severity::Warn,
                "Non-production should keep default dev dep severity"
            );
        }

        /// Default cache dir is root/.fallow.
        #[test]
        fn cache_dir_defaults_to_root_fallow(dir_suffix in "[a-zA-Z0-9_]{1,20}") {
            let root = PathBuf::from(format!("/project/{dir_suffix}"));
            let expected_cache = root.join(".fallow");
            let resolved = make_config(false).resolve(
                root,
                OutputFormat::Human,
                1,
                true,
                true,
                None,
            );
            prop_assert_eq!(
                resolved.cache_dir, expected_cache,
                "Default cache dir should be root/.fallow"
            );
        }

        /// Thread count is always passed through exactly.
        #[test]
        fn threads_passed_through(threads in 1..64usize) {
            let resolved = make_config(false).resolve(
                PathBuf::from("/project"),
                OutputFormat::Human,
                threads,
                true,
                true, None,
            );
            prop_assert_eq!(
                resolved.threads, threads,
                "Thread count should be passed through"
            );
        }

        /// Custom ignore patterns are merged with defaults, not replacing them.
        /// Uses a pattern regex that cannot match node_modules paths, so the
        /// assertion proves the default pattern is what provides the match.
        #[test]
        fn custom_ignores_dont_replace_defaults(pattern in "[a-z_]{1,10}/[a-z_]{1,10}") {
            let mut config = make_config(false);
            config.ignore_patterns = vec![pattern];
            let resolved = config.resolve(
                PathBuf::from("/project"),
                OutputFormat::Human,
                1,
                true,
                true, None,
            );
            prop_assert!(
                resolved.ignore_patterns.is_match("node_modules/foo/bar.js"),
                "Default node_modules ignore should still be active"
            );
        }
    }
}

#[test]
fn resolve_expands_boundary_preset() {
    use crate::config::boundaries::BoundaryPreset;

    let mut config = make_config(false);
    config.boundaries.preset = Some(BoundaryPreset::Hexagonal);
    let resolved = config.resolve(
        PathBuf::from("/project"),
        OutputFormat::Human,
        1,
        true,
        true,
        None,
    );
    assert_eq!(resolved.boundaries.zones.len(), 3);
    assert_eq!(resolved.boundaries.rules.len(), 3);
    assert_eq!(resolved.boundaries.zones[0].name, "adapters");
    assert_eq!(
        resolved.boundaries.classify_zone("src/adapters/http.ts"),
        Some("adapters")
    );
}

#[test]
fn resolve_boundary_preset_with_user_override() {
    use crate::config::boundaries::{BoundaryPreset, BoundaryZone};

    let mut config = make_config(false);
    config.boundaries.preset = Some(BoundaryPreset::Hexagonal);
    config.boundaries.zones = vec![BoundaryZone {
        name: "domain".to_string(),
        patterns: vec!["src/core/**".to_string()],
        auto_discover: vec![],
        root: None,
    }];
    let resolved = config.resolve(
        PathBuf::from("/project"),
        OutputFormat::Human,
        1,
        true,
        true,
        None,
    );
    assert_eq!(resolved.boundaries.zones.len(), 3);
    assert_eq!(
        resolved.boundaries.classify_zone("src/core/user.ts"),
        Some("domain")
    );
    assert_eq!(
        resolved.boundaries.classify_zone("src/domain/user.ts"),
        None
    );
}

#[test]
fn resolve_no_preset_unchanged() {
    let config = make_config(false);
    let resolved = config.resolve(
        PathBuf::from("/project"),
        OutputFormat::Human,
        1,
        true,
        true,
        None,
    );
    assert!(resolved.boundaries.is_empty());
}
