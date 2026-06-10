use fallow_config::{
    BoundaryCallsConfig, BoundaryConfig, BoundaryCoverageConfig, BoundaryPreset, BoundaryRule,
    BoundaryZone, DuplicatesConfig, FallowConfig, FlagsConfig, HealthConfig, OutputFormat,
    ResolveConfig, RulesConfig, Severity,
};

use super::common::fixture_path;

fn create_boundary_config(
    root: std::path::PathBuf,
    boundaries: BoundaryConfig,
) -> fallow_config::ResolvedConfig {
    create_boundary_config_with_entry(root, boundaries, "src/ui/App.ts")
}

fn create_boundary_config_with_entry(
    root: std::path::PathBuf,
    boundaries: BoundaryConfig,
    entry: &str,
) -> fallow_config::ResolvedConfig {
    FallowConfig {
        schema: None,
        extends: vec![],
        entry: vec![entry.to_string()],
        ignore_patterns: vec![],
        framework: vec![],
        workspaces: None,
        ignore_dependencies: vec![],
        ignore_unresolved_imports: vec![],
        ignore_exports: vec![],
        ignore_catalog_references: vec![],
        ignore_dependency_overrides: vec![],
        ignore_exports_used_in_file: fallow_config::IgnoreExportsUsedInFileConfig::default(),
        used_class_members: vec![],
        ignore_decorators: vec![],
        duplicates: DuplicatesConfig::default(),
        health: HealthConfig::default(),
        rules: RulesConfig {
            boundary_violation: Severity::Error,
            ..RulesConfig::default()
        },
        boundaries,
        production: false.into(),
        plugins: vec![],
        dynamically_loaded: vec![],
        overrides: vec![],
        regression: None,
        audit: fallow_config::AuditConfig::default(),
        codeowners: None,
        public_packages: vec![],
        flags: FlagsConfig::default(),
        security: fallow_config::SecurityConfig::default(),
        fix: fallow_config::FixConfig::default(),
        resolve: ResolveConfig::default(),
        sealed: false,
        include_entry_exports: false,
        auto_imports: false,
        cache: fallow_config::CacheConfig::default(),
    }
    .resolve(root, OutputFormat::Human, 4, true, true, None)
}

#[test]
fn detects_boundary_violation() {
    let root = fixture_path("boundary-violations");
    let boundaries = BoundaryConfig {
        coverage: BoundaryCoverageConfig::default(),
        calls: BoundaryCallsConfig::default(),
        preset: None,
        zones: vec![
            BoundaryZone {
                name: "ui".to_string(),
                patterns: vec!["src/ui/**".to_string()],
                auto_discover: vec![],
                root: None,
            },
            BoundaryZone {
                name: "db".to_string(),
                patterns: vec!["src/db/**".to_string()],
                auto_discover: vec![],
                root: None,
            },
            BoundaryZone {
                name: "shared".to_string(),
                patterns: vec!["src/shared/**".to_string()],
                auto_discover: vec![],
                root: None,
            },
        ],
        rules: vec![
            BoundaryRule {
                from: "ui".to_string(),
                allow: vec!["shared".to_string()],
                allow_type_only: vec![],
            },
            BoundaryRule {
                from: "db".to_string(),
                allow: vec!["shared".to_string()],
                allow_type_only: vec![],
            },
        ],
    };
    let config = create_boundary_config(root, boundaries);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    assert_eq!(
        results.boundary_violations.len(),
        1,
        "expected 1 boundary violation, got: {:?}",
        results
            .boundary_violations
            .iter()
            .map(|v| format!("{} -> {}", v.violation.from_zone, v.violation.to_zone))
            .collect::<Vec<_>>()
    );

    let v = &results.boundary_violations[0];
    assert_eq!(v.violation.from_zone, "ui");
    assert_eq!(v.violation.to_zone, "db");
    assert!(
        v.violation
            .from_path
            .to_string_lossy()
            .replace('\\', "/")
            .ends_with("src/ui/App.ts"),
        "from_path should end with src/ui/App.ts, got: {}",
        v.violation.from_path.display()
    );
    assert!(
        v.violation
            .to_path
            .to_string_lossy()
            .replace('\\', "/")
            .ends_with("src/db/query.ts"),
        "to_path should end with src/db/query.ts, got: {}",
        v.violation.to_path.display()
    );
}

#[test]
fn no_violations_when_boundaries_disabled() {
    let root = fixture_path("boundary-violations");
    let config = super::common::create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    assert!(
        results.boundary_violations.is_empty(),
        "no boundary violations expected with default config"
    );
}

#[test]
fn reports_unmatched_files_when_boundary_coverage_is_required() {
    let root = fixture_path("boundary-violations");
    let boundaries = BoundaryConfig {
        zones: vec![BoundaryZone {
            name: "ui".to_string(),
            patterns: vec!["src/ui/**".to_string()],
            auto_discover: vec![],
            root: None,
        }],
        coverage: BoundaryCoverageConfig {
            require_all_files: true,
            allow_unmatched: vec![],
        },
        ..BoundaryConfig::default()
    };
    let config = create_boundary_config(root, boundaries);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let paths = results
        .boundary_coverage_violations
        .iter()
        .map(|v| v.violation.path.to_string_lossy().replace('\\', "/"))
        .collect::<Vec<_>>();
    assert!(
        paths.iter().any(|p| p.ends_with("src/generated/client.ts")),
        "expected generated client to be reported as unmatched, got {paths:?}"
    );
}

#[test]
fn allow_unmatched_excludes_boundary_coverage_findings() {
    let root = fixture_path("boundary-violations");
    let boundaries = BoundaryConfig {
        zones: vec![BoundaryZone {
            name: "ui".to_string(),
            patterns: vec!["src/ui/**".to_string()],
            auto_discover: vec![],
            root: None,
        }],
        coverage: BoundaryCoverageConfig {
            require_all_files: true,
            allow_unmatched: vec!["src/generated/**".to_string(), "src/db/**".to_string()],
        },
        ..BoundaryConfig::default()
    };
    let config = create_boundary_config(root, boundaries);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    assert!(
        results.boundary_coverage_violations.iter().all(|v| !v
            .violation
            .path
            .to_string_lossy()
            .replace('\\', "/")
            .ends_with("src/generated/client.ts")),
        "allowUnmatched should suppress generated client coverage findings"
    );
}

fn calls_boundaries(forbidden: Vec<fallow_config::ForbiddenCallRule>) -> BoundaryConfig {
    BoundaryConfig {
        zones: vec![
            BoundaryZone {
                name: "domain".to_string(),
                patterns: vec!["src/domain/**".to_string()],
                auto_discover: vec![],
                root: None,
            },
            BoundaryZone {
                name: "ui".to_string(),
                patterns: vec!["src/ui/**".to_string()],
                auto_discover: vec![],
                root: None,
            },
        ],
        calls: BoundaryCallsConfig { forbidden },
        ..BoundaryConfig::default()
    }
}

fn forbid_call(from: &str, callee: &str) -> fallow_config::ForbiddenCallRule {
    fallow_config::ForbiddenCallRule {
        from: from.to_string(),
        callee: fallow_config::ForbiddenCallee::Single(callee.to_string()),
    }
}

#[test]
fn reports_forbidden_calls_from_zoned_files() {
    let root = fixture_path("boundary-violations");
    let boundaries = calls_boundaries(vec![
        forbid_call("domain", "child_process.*"),
        forbid_call("domain", "console.*"),
    ]);
    let config = create_boundary_config_with_entry(root, boundaries, "src/**/*.ts");
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let entries: Vec<(String, String, String, String)> = results
        .boundary_call_violations
        .iter()
        .map(|v| {
            (
                v.violation.path.to_string_lossy().replace('\\', "/"),
                v.violation.zone.clone(),
                v.violation.callee.clone(),
                v.violation.pattern.clone(),
            )
        })
        .collect();

    assert!(
        entries.iter().any(|(path, zone, callee, pattern)| {
            path.ends_with("src/domain/policy.ts")
                && zone == "domain"
                && callee == "execSync"
                && pattern == "child_process.*"
        }),
        "expected the named-import execSync call to canonicalize and fire, got {entries:?}"
    );
    assert!(
        entries.iter().any(|(path, zone, callee, pattern)| {
            path.ends_with("src/domain/policy.ts")
                && zone == "domain"
                && callee == "console.log"
                && pattern == "console.*"
        }),
        "expected the global console.log call to fire on the written path, got {entries:?}"
    );
    assert!(
        entries
            .iter()
            .all(|(path, _, _, _)| !path.ends_with("src/ui/App.ts")),
        "ui zone has no forbidden-call rule, so its console.log must stay quiet: {entries:?}"
    );
    assert!(
        entries
            .iter()
            .all(|(path, _, _, _)| !path.ends_with("src/domain/suppressed.ts")),
        "file-level boundary-violation suppression must be consumed: {entries:?}"
    );
}

#[test]
fn no_forbidden_call_findings_without_calls_config() {
    let root = fixture_path("boundary-violations");
    let boundaries = calls_boundaries(vec![]);
    let config = create_boundary_config_with_entry(root, boundaries, "src/**/*.ts");
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    assert!(results.boundary_call_violations.is_empty());
}

#[test]
fn no_violations_when_rule_is_off() {
    let root = fixture_path("boundary-violations");
    let boundaries = BoundaryConfig {
        coverage: BoundaryCoverageConfig::default(),
        calls: BoundaryCallsConfig::default(),
        preset: None,
        zones: vec![
            BoundaryZone {
                name: "ui".to_string(),
                patterns: vec!["src/ui/**".to_string()],
                auto_discover: vec![],
                root: None,
            },
            BoundaryZone {
                name: "db".to_string(),
                patterns: vec!["src/db/**".to_string()],
                auto_discover: vec![],
                root: None,
            },
        ],
        rules: vec![BoundaryRule {
            from: "ui".to_string(),
            allow: vec![],
            allow_type_only: vec![],
        }],
    };
    let config = FallowConfig {
        schema: None,
        extends: vec![],
        entry: vec!["src/ui/App.ts".to_string()],
        ignore_patterns: vec![],
        framework: vec![],
        workspaces: None,
        ignore_dependencies: vec![],
        ignore_unresolved_imports: vec![],
        ignore_exports: vec![],
        ignore_catalog_references: vec![],
        ignore_dependency_overrides: vec![],
        ignore_exports_used_in_file: fallow_config::IgnoreExportsUsedInFileConfig::default(),
        used_class_members: vec![],
        ignore_decorators: vec![],
        duplicates: DuplicatesConfig::default(),
        health: HealthConfig::default(),
        rules: RulesConfig {
            boundary_violation: Severity::Off,
            ..RulesConfig::default()
        },
        boundaries,
        production: false.into(),
        plugins: vec![],
        dynamically_loaded: vec![],
        overrides: vec![],
        regression: None,
        audit: fallow_config::AuditConfig::default(),
        codeowners: None,
        public_packages: vec![],
        flags: FlagsConfig::default(),
        security: fallow_config::SecurityConfig::default(),
        fix: fallow_config::FixConfig::default(),
        resolve: ResolveConfig::default(),
        sealed: false,
        include_entry_exports: false,
        auto_imports: false,
        cache: fallow_config::CacheConfig::default(),
    }
    .resolve(root, OutputFormat::Human, 4, true, true, None);

    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    assert!(
        results.boundary_violations.is_empty(),
        "boundary violations should be empty when rule is off"
    );
}

#[test]
fn preset_detects_boundary_violation() {
    let root = fixture_path("boundary-preset");
    let boundaries = BoundaryConfig {
        coverage: BoundaryCoverageConfig::default(),
        calls: BoundaryCallsConfig::default(),
        preset: Some(BoundaryPreset::Hexagonal),
        zones: vec![],
        rules: vec![],
    };
    let config = FallowConfig {
        schema: None,
        extends: vec![],
        entry: vec!["src/adapters/http.ts".to_string()],
        ignore_patterns: vec![],
        framework: vec![],
        workspaces: None,
        ignore_dependencies: vec![],
        ignore_unresolved_imports: vec![],
        ignore_exports: vec![],
        ignore_catalog_references: vec![],
        ignore_dependency_overrides: vec![],
        ignore_exports_used_in_file: fallow_config::IgnoreExportsUsedInFileConfig::default(),
        used_class_members: vec![],
        ignore_decorators: vec![],
        duplicates: DuplicatesConfig::default(),
        health: HealthConfig::default(),
        rules: RulesConfig {
            boundary_violation: Severity::Error,
            ..RulesConfig::default()
        },
        boundaries,
        production: false.into(),
        plugins: vec![],
        dynamically_loaded: vec![],
        overrides: vec![],
        regression: None,
        audit: fallow_config::AuditConfig::default(),
        codeowners: None,
        public_packages: vec![],
        flags: FlagsConfig::default(),
        security: fallow_config::SecurityConfig::default(),
        fix: fallow_config::FixConfig::default(),
        resolve: ResolveConfig::default(),
        sealed: false,
        include_entry_exports: false,
        auto_imports: false,
        cache: fallow_config::CacheConfig::default(),
    }
    .resolve(root, OutputFormat::Human, 4, true, true, None);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    assert_eq!(
        results.boundary_violations.len(),
        1,
        "expected 1 boundary violation, got: {:?}",
        results
            .boundary_violations
            .iter()
            .map(|v| format!(
                "{} ({}) -> {} ({})",
                v.violation.from_zone,
                v.violation.from_path.display(),
                v.violation.to_zone,
                v.violation.to_path.display()
            ))
            .collect::<Vec<_>>()
    );

    let v = &results.boundary_violations[0];
    assert_eq!(v.violation.from_zone, "adapters");
    assert_eq!(v.violation.to_zone, "domain");
}

#[test]
fn root_field_classifies_per_subtree() {
    let root = fixture_path("boundary-root");
    let boundaries = BoundaryConfig {
        coverage: BoundaryCoverageConfig::default(),
        calls: BoundaryCallsConfig::default(),
        preset: None,
        zones: vec![
            BoundaryZone {
                name: "ui".to_string(),
                patterns: vec!["src/**".to_string()],
                auto_discover: vec![],
                root: Some("packages/app/".to_string()),
            },
            BoundaryZone {
                name: "domain".to_string(),
                patterns: vec!["src/**".to_string()],
                auto_discover: vec![],
                root: Some("packages/core/".to_string()),
            },
        ],
        rules: vec![
            BoundaryRule {
                from: "ui".to_string(),
                allow: vec![],
                allow_type_only: vec![],
            },
            BoundaryRule {
                from: "domain".to_string(),
                allow: vec![],
                allow_type_only: vec![],
            },
        ],
    };
    let config = FallowConfig {
        schema: None,
        extends: vec![],
        entry: vec!["packages/app/src/login.tsx".to_string()],
        ignore_patterns: vec![],
        framework: vec![],
        workspaces: None,
        ignore_dependencies: vec![],
        ignore_unresolved_imports: vec![],
        ignore_exports: vec![],
        ignore_catalog_references: vec![],
        ignore_dependency_overrides: vec![],
        ignore_exports_used_in_file: fallow_config::IgnoreExportsUsedInFileConfig::default(),
        used_class_members: vec![],
        ignore_decorators: vec![],
        duplicates: DuplicatesConfig::default(),
        health: HealthConfig::default(),
        rules: RulesConfig {
            boundary_violation: Severity::Error,
            ..RulesConfig::default()
        },
        boundaries,
        production: false.into(),
        plugins: vec![],
        dynamically_loaded: vec![],
        overrides: vec![],
        regression: None,
        audit: fallow_config::AuditConfig::default(),
        codeowners: None,
        public_packages: vec![],
        flags: FlagsConfig::default(),
        security: fallow_config::SecurityConfig::default(),
        fix: fallow_config::FixConfig::default(),
        resolve: ResolveConfig::default(),
        sealed: false,
        include_entry_exports: false,
        auto_imports: false,
        cache: fallow_config::CacheConfig::default(),
    }
    .resolve(root, OutputFormat::Human, 4, true, true, None);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    assert_eq!(
        results.boundary_violations.len(),
        1,
        "expected 1 boundary violation, got: {:?}",
        results
            .boundary_violations
            .iter()
            .map(|v| format!(
                "{} ({}) -> {} ({})",
                v.violation.from_zone,
                v.violation.from_path.display(),
                v.violation.to_zone,
                v.violation.to_path.display()
            ))
            .collect::<Vec<_>>()
    );

    let v = &results.boundary_violations[0];
    assert_eq!(v.violation.from_zone, "ui");
    assert_eq!(v.violation.to_zone, "domain");
    assert!(
        v.violation
            .from_path
            .to_string_lossy()
            .replace('\\', "/")
            .ends_with("packages/app/src/login.tsx"),
        "from_path should end with packages/app/src/login.tsx, got: {}",
        v.violation.from_path.display()
    );
    assert!(
        v.violation
            .to_path
            .to_string_lossy()
            .replace('\\', "/")
            .ends_with("packages/core/src/order.ts"),
        "to_path should end with packages/core/src/order.ts, got: {}",
        v.violation.to_path.display()
    );
}

#[test]
fn root_field_genuinely_disambiguates_flat_patterns() {
    let root = fixture_path("boundary-root");

    let flat_boundaries = BoundaryConfig {
        coverage: BoundaryCoverageConfig::default(),
        calls: BoundaryCallsConfig::default(),
        preset: None,
        zones: vec![BoundaryZone {
            name: "ui".to_string(),
            patterns: vec!["packages/*/src/**".to_string()],
            auto_discover: vec![],
            root: None,
        }],
        rules: vec![BoundaryRule {
            from: "ui".to_string(),
            allow: vec![],
            allow_type_only: vec![],
        }],
    };
    let flat_config = FallowConfig {
        schema: None,
        extends: vec![],
        entry: vec!["packages/app/src/login.tsx".to_string()],
        ignore_patterns: vec![],
        framework: vec![],
        workspaces: None,
        ignore_dependencies: vec![],
        ignore_unresolved_imports: vec![],
        ignore_exports: vec![],
        ignore_catalog_references: vec![],
        ignore_dependency_overrides: vec![],
        ignore_exports_used_in_file: fallow_config::IgnoreExportsUsedInFileConfig::default(),
        used_class_members: vec![],
        ignore_decorators: vec![],
        duplicates: DuplicatesConfig::default(),
        health: HealthConfig::default(),
        rules: RulesConfig {
            boundary_violation: Severity::Error,
            ..RulesConfig::default()
        },
        boundaries: flat_boundaries,
        production: false.into(),
        plugins: vec![],
        dynamically_loaded: vec![],
        overrides: vec![],
        regression: None,
        audit: fallow_config::AuditConfig::default(),
        codeowners: None,
        public_packages: vec![],
        flags: FlagsConfig::default(),
        security: fallow_config::SecurityConfig::default(),
        fix: fallow_config::FixConfig::default(),
        resolve: ResolveConfig::default(),
        sealed: false,
        include_entry_exports: false,
        auto_imports: false,
        cache: fallow_config::CacheConfig::default(),
    }
    .resolve(root.clone(), OutputFormat::Human, 4, true, true, None);
    let flat_results = fallow_core::analyze(&flat_config).expect("analysis should succeed");
    assert!(
        flat_results.boundary_violations.is_empty(),
        "without root, both files share the same zone so self-imports are allowed"
    );

    let scoped_boundaries = BoundaryConfig {
        coverage: BoundaryCoverageConfig::default(),
        calls: BoundaryCallsConfig::default(),
        preset: None,
        zones: vec![
            BoundaryZone {
                name: "ui".to_string(),
                patterns: vec!["src/**".to_string()],
                auto_discover: vec![],
                root: Some("packages/app/".to_string()),
            },
            BoundaryZone {
                name: "domain".to_string(),
                patterns: vec!["src/**".to_string()],
                auto_discover: vec![],
                root: Some("packages/core/".to_string()),
            },
        ],
        rules: vec![BoundaryRule {
            from: "ui".to_string(),
            allow: vec![],
            allow_type_only: vec![],
        }],
    };
    let scoped_config = FallowConfig {
        schema: None,
        extends: vec![],
        entry: vec!["packages/app/src/login.tsx".to_string()],
        ignore_patterns: vec![],
        framework: vec![],
        workspaces: None,
        ignore_dependencies: vec![],
        ignore_unresolved_imports: vec![],
        ignore_exports: vec![],
        ignore_catalog_references: vec![],
        ignore_dependency_overrides: vec![],
        ignore_exports_used_in_file: fallow_config::IgnoreExportsUsedInFileConfig::default(),
        used_class_members: vec![],
        ignore_decorators: vec![],
        duplicates: DuplicatesConfig::default(),
        health: HealthConfig::default(),
        rules: RulesConfig {
            boundary_violation: Severity::Error,
            ..RulesConfig::default()
        },
        boundaries: scoped_boundaries,
        production: false.into(),
        plugins: vec![],
        dynamically_loaded: vec![],
        overrides: vec![],
        regression: None,
        audit: fallow_config::AuditConfig::default(),
        codeowners: None,
        public_packages: vec![],
        flags: FlagsConfig::default(),
        security: fallow_config::SecurityConfig::default(),
        fix: fallow_config::FixConfig::default(),
        resolve: ResolveConfig::default(),
        sealed: false,
        include_entry_exports: false,
        auto_imports: false,
        cache: fallow_config::CacheConfig::default(),
    }
    .resolve(root, OutputFormat::Human, 4, true, true, None);
    let scoped_results = fallow_core::analyze(&scoped_config).expect("analysis should succeed");
    assert_eq!(
        scoped_results.boundary_violations.len(),
        1,
        "with root, the cross-package import is now a cross-zone violation"
    );
    let v = &scoped_results.boundary_violations[0];
    assert_eq!(v.violation.from_zone, "ui");
    assert_eq!(v.violation.to_zone, "domain");
}

#[test]
fn auto_discover_isolates_child_boundary_zones() {
    let root = fixture_path("boundary-auto-discover");
    let boundaries = BoundaryConfig {
        coverage: BoundaryCoverageConfig::default(),
        calls: BoundaryCallsConfig::default(),
        preset: None,
        zones: vec![
            BoundaryZone {
                name: "app".to_string(),
                patterns: vec!["src/app/**".to_string()],
                auto_discover: vec![],
                root: None,
            },
            BoundaryZone {
                name: "features".to_string(),
                patterns: vec![],
                auto_discover: vec!["src/features".to_string()],
                root: None,
            },
            BoundaryZone {
                name: "shared".to_string(),
                patterns: vec!["src/shared/**".to_string()],
                auto_discover: vec![],
                root: None,
            },
        ],
        rules: vec![
            BoundaryRule {
                from: "app".to_string(),
                allow: vec!["features".to_string(), "shared".to_string()],
                allow_type_only: vec![],
            },
            BoundaryRule {
                from: "features".to_string(),
                allow: vec!["shared".to_string()],
                allow_type_only: vec![],
            },
        ],
    };
    let config = create_boundary_config_with_entry(root, boundaries, "src/app/page.ts");
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    assert_eq!(
        results.boundary_violations.len(),
        1,
        "expected only the cross-feature import to violate boundaries, got: {:?}",
        results
            .boundary_violations
            .iter()
            .map(|v| format!("{} -> {}", v.violation.from_zone, v.violation.to_zone))
            .collect::<Vec<_>>()
    );

    let v = &results.boundary_violations[0];
    assert_eq!(v.violation.from_zone, "features/auth");
    assert_eq!(v.violation.to_zone, "features/billing");
    assert!(
        v.violation
            .from_path
            .to_string_lossy()
            .replace('\\', "/")
            .ends_with("src/features/auth/login.ts"),
        "from_path should end with src/features/auth/login.ts, got: {}",
        v.violation.from_path.display()
    );
    assert!(
        v.violation
            .to_path
            .to_string_lossy()
            .replace('\\', "/")
            .ends_with("src/features/billing/invoice.ts"),
        "to_path should end with src/features/billing/invoice.ts, got: {}",
        v.violation.to_path.display()
    );
}

#[test]
fn bulletproof_preset_detects_violation() {
    let root = fixture_path("boundary-bulletproof");
    let boundaries = BoundaryConfig {
        coverage: BoundaryCoverageConfig::default(),
        calls: BoundaryCallsConfig::default(),
        preset: Some(BoundaryPreset::Bulletproof),
        zones: vec![],
        rules: vec![],
    };
    let config = FallowConfig {
        schema: None,
        extends: vec![],
        entry: vec!["src/app/page.ts".to_string()],
        ignore_patterns: vec![],
        framework: vec![],
        workspaces: None,
        ignore_dependencies: vec![],
        ignore_unresolved_imports: vec![],
        ignore_exports: vec![],
        ignore_catalog_references: vec![],
        ignore_dependency_overrides: vec![],
        ignore_exports_used_in_file: fallow_config::IgnoreExportsUsedInFileConfig::default(),
        used_class_members: vec![],
        ignore_decorators: vec![],
        duplicates: DuplicatesConfig::default(),
        health: HealthConfig::default(),
        rules: RulesConfig {
            boundary_violation: Severity::Error,
            ..RulesConfig::default()
        },
        boundaries,
        production: false.into(),
        plugins: vec![],
        dynamically_loaded: vec![],
        overrides: vec![],
        regression: None,
        audit: fallow_config::AuditConfig::default(),
        codeowners: None,
        public_packages: vec![],
        flags: FlagsConfig::default(),
        security: fallow_config::SecurityConfig::default(),
        fix: fallow_config::FixConfig::default(),
        resolve: ResolveConfig::default(),
        sealed: false,
        include_entry_exports: false,
        auto_imports: false,
        cache: fallow_config::CacheConfig::default(),
    }
    .resolve(root, OutputFormat::Human, 4, true, true, None);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    assert_eq!(
        results.boundary_violations.len(),
        1,
        "expected 1 boundary violation, got: {:?}",
        results
            .boundary_violations
            .iter()
            .map(|v| format!(
                "{} ({}) -> {} ({})",
                v.violation.from_zone,
                v.violation.from_path.display(),
                v.violation.to_zone,
                v.violation.to_path.display()
            ))
            .collect::<Vec<_>>()
    );

    let v = &results.boundary_violations[0];
    assert_eq!(v.violation.from_zone, "features/auth");
    assert_eq!(v.violation.to_zone, "app");
    assert!(
        v.violation
            .from_path
            .to_string_lossy()
            .replace('\\', "/")
            .ends_with("src/features/auth/login.ts"),
        "from_path should end with src/features/auth/login.ts, got: {}",
        v.violation.from_path.display()
    );
    assert!(
        v.violation
            .to_path
            .to_string_lossy()
            .replace('\\', "/")
            .ends_with("src/app/page.ts"),
        "to_path should end with src/app/page.ts, got: {}",
        v.violation.to_path.display()
    );
}

/// Regression for the Bulletproof preset's `autoDiscover` strict mode. The
/// preset classifies top-level files inside `src/features/` under the parent
/// `features` zone while discovered child zones keep sibling isolation. This
/// test pins both sides of that behavior:
///
/// 1. `src/features/index.ts` (the barrel) re-exports `auth/login` and
///    `types`. The parent `features` rule must allow the discovered
///    `features/auth` child, otherwise it would surface a false positive.
/// 2. `src/features/types.ts` imports `src/app/page`. Under the preset
///    `types.ts` must classify as `features`, so this strict-mode violation
///    is reported.
#[test]
fn bulletproof_top_level_features_file_is_strict_without_barrel_false_positive() {
    let root = fixture_path("boundary-bulletproof-toplevel");
    let boundaries = BoundaryConfig {
        coverage: BoundaryCoverageConfig::default(),
        calls: BoundaryCallsConfig::default(),
        preset: Some(BoundaryPreset::Bulletproof),
        zones: vec![],
        rules: vec![],
    };
    let config = FallowConfig {
        schema: None,
        extends: vec![],
        entry: vec!["src/app/page.ts".to_string()],
        ignore_patterns: vec![],
        framework: vec![],
        workspaces: None,
        ignore_dependencies: vec![],
        ignore_unresolved_imports: vec![],
        ignore_exports: vec![],
        ignore_catalog_references: vec![],
        ignore_dependency_overrides: vec![],
        ignore_exports_used_in_file: fallow_config::IgnoreExportsUsedInFileConfig::default(),
        used_class_members: vec![],
        ignore_decorators: vec![],
        duplicates: DuplicatesConfig::default(),
        health: HealthConfig::default(),
        rules: RulesConfig {
            boundary_violation: Severity::Error,
            ..RulesConfig::default()
        },
        boundaries,
        production: false.into(),
        plugins: vec![],
        dynamically_loaded: vec![],
        overrides: vec![],
        regression: None,
        audit: fallow_config::AuditConfig::default(),
        codeowners: None,
        public_packages: vec![],
        flags: FlagsConfig::default(),
        security: fallow_config::SecurityConfig::default(),
        fix: fallow_config::FixConfig::default(),
        resolve: ResolveConfig::default(),
        sealed: false,
        include_entry_exports: false,
        auto_imports: false,
        cache: fallow_config::CacheConfig::default(),
    }
    .resolve(root, OutputFormat::Human, 4, true, true, None);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    assert_eq!(
        results.boundary_violations.len(),
        1,
        "Bulletproof preset should allow the src/features/index.ts barrel to \
         re-export discovered children, but still report strict top-level \
         feature files that import forbidden zones. Got: {:?}",
        results
            .boundary_violations
            .iter()
            .map(|v| format!(
                "{} ({}) -> {} ({})",
                v.violation.from_zone,
                v.violation.from_path.display(),
                v.violation.to_zone,
                v.violation.to_path.display()
            ))
            .collect::<Vec<_>>()
    );
    let v = &results.boundary_violations[0];
    assert_eq!(v.violation.from_zone, "features");
    assert_eq!(v.violation.to_zone, "app");
    assert!(
        v.violation
            .from_path
            .to_string_lossy()
            .replace('\\', "/")
            .ends_with("src/features/types.ts"),
        "from_path should end with src/features/types.ts, got: {}",
        v.violation.from_path.display()
    );
    assert!(
        v.violation
            .to_path
            .to_string_lossy()
            .replace('\\', "/")
            .ends_with("src/app/page.ts"),
        "to_path should end with src/app/page.ts, got: {}",
        v.violation.to_path.display()
    );
}

/// Build a ui/db/shared boundary config for the boundary-type-only
/// fixture. `allow_type_only_db` is the list of zones the `ui` rule
/// admits type-only imports from. Both `ui` and `db` already allow
/// `shared` in their regular `allow` list (matches the fixture's
/// shared/utils.ts importers).
fn type_only_boundaries(allow_type_only_db: Vec<String>) -> BoundaryConfig {
    BoundaryConfig {
        coverage: BoundaryCoverageConfig::default(),
        calls: BoundaryCallsConfig::default(),
        preset: None,
        zones: vec![
            BoundaryZone {
                name: "ui".to_string(),
                patterns: vec!["src/ui/**".to_string()],
                auto_discover: vec![],
                root: None,
            },
            BoundaryZone {
                name: "db".to_string(),
                patterns: vec!["src/db/**".to_string()],
                auto_discover: vec![],
                root: None,
            },
            BoundaryZone {
                name: "shared".to_string(),
                patterns: vec!["src/shared/**".to_string()],
                auto_discover: vec![],
                root: None,
            },
        ],
        rules: vec![
            BoundaryRule {
                from: "ui".to_string(),
                allow: vec!["shared".to_string()],
                allow_type_only: allow_type_only_db,
            },
            BoundaryRule {
                from: "db".to_string(),
                allow: vec!["shared".to_string()],
                allow_type_only: vec![],
            },
        ],
    }
}

/// Collect violation paths relative to the fixture root, with forward
/// slashes, for stable cross-platform assertions.
fn collect_violation_from_paths(
    results: &fallow_types::results::AnalysisResults,
    fixture_root: &std::path::Path,
) -> std::collections::BTreeSet<String> {
    results
        .boundary_violations
        .iter()
        .map(|v| {
            v.violation
                .from_path
                .strip_prefix(fixture_root)
                .map_or_else(
                    |_| v.violation.from_path.to_string_lossy().replace('\\', "/"),
                    |p| p.to_string_lossy().replace('\\', "/"),
                )
        })
        .collect()
}

#[test]
fn allow_type_only_admits_whole_decl_inline_and_namespace_type_imports() {
    let root = fixture_path("boundary-type-only");
    let boundaries = type_only_boundaries(vec!["db".to_string()]);
    let config = create_boundary_config_with_entry(root.clone(), boundaries, "src/ui/App.ts");
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let from_paths = collect_violation_from_paths(&results, &root);

    let expected: std::collections::BTreeSet<String> = [
        "src/ui/value.ts",
        "src/ui/mixed.ts",
        "src/ui/side_effect.ts",
        "src/ui/sibling_imports.ts",
    ]
    .iter()
    .map(|s| (*s).to_string())
    .collect();

    assert_eq!(
        from_paths, expected,
        "allowTypeOnly=[db] should admit type_only.ts, inline_type.ts, namespace_type.ts, \
         type_reexport.ts and still flag value.ts, mixed.ts, side_effect.ts, sibling_imports.ts"
    );
}

#[test]
fn mixed_edge_violation_anchors_on_the_value_import_line_not_the_type_only_one() {
    let root = fixture_path("boundary-type-only");
    let boundaries = type_only_boundaries(vec!["db".to_string()]);
    let config = create_boundary_config_with_entry(root.clone(), boundaries, "src/ui/App.ts");
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let sibling = results
        .boundary_violations
        .iter()
        .find(|v| {
            v.violation
                .from_path
                .to_string_lossy()
                .replace('\\', "/")
                .ends_with("src/ui/sibling_imports.ts")
        })
        .expect("sibling_imports.ts must produce a violation");

    let source = std::fs::read_to_string(root.join("src/ui/sibling_imports.ts"))
        .expect("fixture must exist");
    let value_line = source
        .lines()
        .enumerate()
        .find(|(_, l)| l.contains("import { runQuery }"))
        .map(|(i, _)| (i as u32) + 1)
        .expect("value import line must exist in fixture");
    let type_only_line = source
        .lines()
        .enumerate()
        .find(|(_, l)| l.contains("import type { Query }"))
        .map(|(i, _)| (i as u32) + 1)
        .expect("type-only import line must exist in fixture");

    assert_eq!(
        sibling.violation.line, value_line,
        "violation must anchor on the value-import line ({value_line}), \
         not the type-only line ({type_only_line}); got {}",
        sibling.violation.line
    );
}

#[test]
fn empty_allow_type_only_flags_every_cross_zone_import() {
    let root = fixture_path("boundary-type-only");
    let boundaries = type_only_boundaries(vec![]);
    let config = create_boundary_config_with_entry(root.clone(), boundaries, "src/ui/App.ts");
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let from_paths = collect_violation_from_paths(&results, &root);

    let expected: std::collections::BTreeSet<String> = [
        "src/ui/type_only.ts",
        "src/ui/inline_type.ts",
        "src/ui/namespace_type.ts",
        "src/ui/value.ts",
        "src/ui/mixed.ts",
        "src/ui/side_effect.ts",
        "src/ui/type_reexport.ts",
        "src/ui/sibling_imports.ts",
    ]
    .iter()
    .map(|s| (*s).to_string())
    .collect();

    assert_eq!(
        from_paths, expected,
        "default (empty) allowTypeOnly must preserve pre-feature behavior: \
         every ui -> db importer in the fixture fires"
    );
}

#[test]
fn allow_type_only_with_unlisted_zone_does_not_admit_db_imports() {
    let root = fixture_path("boundary-type-only");
    let boundaries = type_only_boundaries(vec!["sandbox".to_string()]);
    let config = create_boundary_config_with_entry(root.clone(), boundaries, "src/ui/App.ts");
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let from_paths = collect_violation_from_paths(&results, &root);

    let expected: std::collections::BTreeSet<String> = [
        "src/ui/type_only.ts",
        "src/ui/inline_type.ts",
        "src/ui/namespace_type.ts",
        "src/ui/value.ts",
        "src/ui/mixed.ts",
        "src/ui/side_effect.ts",
        "src/ui/type_reexport.ts",
        "src/ui/sibling_imports.ts",
    ]
    .iter()
    .map(|s| (*s).to_string())
    .collect();

    assert_eq!(
        from_paths, expected,
        "allowTypeOnly listing a different zone must NOT admit type-only imports to db"
    );
}

#[test]
fn allow_type_only_admits_type_only_re_exports() {
    let root = fixture_path("boundary-type-only");
    let boundaries = type_only_boundaries(vec!["db".to_string()]);
    let config = create_boundary_config_with_entry(root.clone(), boundaries, "src/ui/App.ts");
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let from_paths = collect_violation_from_paths(&results, &root);
    assert!(
        !from_paths.contains("src/ui/type_reexport.ts"),
        "type-only re-export must not fire under allowTypeOnly=[db]; got: {from_paths:?}"
    );
}
