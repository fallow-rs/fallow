use fallow_config::{
    BoundaryConfig, BoundaryPreset, BoundaryRule, BoundaryZone, DuplicatesConfig, FallowConfig,
    FlagsConfig, HealthConfig, OutputFormat, ResolveConfig, RulesConfig, Severity,
};

use super::common::fixture_path;

fn create_boundary_config(
    root: std::path::PathBuf,
    boundaries: BoundaryConfig,
) -> fallow_config::ResolvedConfig {
    FallowConfig {
        schema: None,
        extends: vec![],
        entry: vec!["src/ui/App.ts".to_string()],
        ignore_patterns: vec![],
        framework: vec![],
        workspaces: None,
        ignore_dependencies: vec![],
        ignore_exports: vec![],
        ignore_catalog_references: vec![],
        ignore_dependency_overrides: vec![],
        ignore_exports_used_in_file: fallow_config::IgnoreExportsUsedInFileConfig::default(),
        used_class_members: vec![],
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
        resolve: ResolveConfig::default(),
        sealed: false,
        include_entry_exports: false,
    }
    .resolve(root, OutputFormat::Human, 4, true, true)
}

#[test]
fn detects_boundary_violation() {
    let root = fixture_path("boundary-violations");
    let boundaries = BoundaryConfig {
        preset: None,
        zones: vec![
            BoundaryZone {
                name: "ui".to_string(),
                patterns: vec!["src/ui/**".to_string()],
                root: None,
            },
            BoundaryZone {
                name: "db".to_string(),
                patterns: vec!["src/db/**".to_string()],
                root: None,
            },
            BoundaryZone {
                name: "shared".to_string(),
                patterns: vec!["src/shared/**".to_string()],
                root: None,
            },
        ],
        rules: vec![
            BoundaryRule {
                from: "ui".to_string(),
                allow: vec!["shared".to_string()],
            },
            BoundaryRule {
                from: "db".to_string(),
                allow: vec!["shared".to_string()],
            },
        ],
    };
    let config = create_boundary_config(root, boundaries);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    // Should find exactly 1 boundary violation: ui/App.ts -> db/query.ts
    assert_eq!(
        results.boundary_violations.len(),
        1,
        "expected 1 boundary violation, got: {:?}",
        results
            .boundary_violations
            .iter()
            .map(|v| format!("{} -> {}", v.from_zone, v.to_zone))
            .collect::<Vec<_>>()
    );

    let v = &results.boundary_violations[0];
    assert_eq!(v.from_zone, "ui");
    assert_eq!(v.to_zone, "db");
    assert!(
        v.from_path
            .to_string_lossy()
            .replace('\\', "/")
            .ends_with("src/ui/App.ts"),
        "from_path should end with src/ui/App.ts, got: {}",
        v.from_path.display()
    );
    assert!(
        v.to_path
            .to_string_lossy()
            .replace('\\', "/")
            .ends_with("src/db/query.ts"),
        "to_path should end with src/db/query.ts, got: {}",
        v.to_path.display()
    );
}

#[test]
fn no_violations_when_boundaries_disabled() {
    let root = fixture_path("boundary-violations");
    let config = super::common::create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    // Default config has no boundaries configured, so no violations
    assert!(
        results.boundary_violations.is_empty(),
        "no boundary violations expected with default config"
    );
}

#[test]
fn no_violations_when_rule_is_off() {
    let root = fixture_path("boundary-violations");
    let boundaries = BoundaryConfig {
        preset: None,
        zones: vec![
            BoundaryZone {
                name: "ui".to_string(),
                patterns: vec!["src/ui/**".to_string()],
                root: None,
            },
            BoundaryZone {
                name: "db".to_string(),
                patterns: vec!["src/db/**".to_string()],
                root: None,
            },
        ],
        rules: vec![BoundaryRule {
            from: "ui".to_string(),
            allow: vec![],
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
        ignore_exports: vec![],
        ignore_catalog_references: vec![],
        ignore_dependency_overrides: vec![],
        ignore_exports_used_in_file: fallow_config::IgnoreExportsUsedInFileConfig::default(),
        used_class_members: vec![],
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
        resolve: ResolveConfig::default(),
        sealed: false,
        include_entry_exports: false,
    }
    .resolve(root, OutputFormat::Human, 4, true, true);

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
        preset: Some(BoundaryPreset::Hexagonal),
        zones: vec![],
        rules: vec![],
    };
    // Use explicit entry point matching the preset fixture (not the shared helper
    // which hardcodes src/ui/App.ts for the boundary-violations fixture).
    let config = FallowConfig {
        schema: None,
        extends: vec![],
        entry: vec!["src/adapters/http.ts".to_string()],
        ignore_patterns: vec![],
        framework: vec![],
        workspaces: None,
        ignore_dependencies: vec![],
        ignore_exports: vec![],
        ignore_catalog_references: vec![],
        ignore_dependency_overrides: vec![],
        ignore_exports_used_in_file: fallow_config::IgnoreExportsUsedInFileConfig::default(),
        used_class_members: vec![],
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
        resolve: ResolveConfig::default(),
        sealed: false,
        include_entry_exports: false,
    }
    .resolve(root, OutputFormat::Human, 4, true, true);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    // adapters/http.ts imports domain/user.ts directly — that's a violation
    // (adapters may only import from ports)
    assert_eq!(
        results.boundary_violations.len(),
        1,
        "expected 1 boundary violation, got: {:?}",
        results
            .boundary_violations
            .iter()
            .map(|v| format!(
                "{} ({}) -> {} ({})",
                v.from_zone,
                v.from_path.display(),
                v.to_zone,
                v.to_path.display()
            ))
            .collect::<Vec<_>>()
    );

    let v = &results.boundary_violations[0];
    assert_eq!(v.from_zone, "adapters");
    assert_eq!(v.to_zone, "domain");
}

#[test]
fn root_field_classifies_per_subtree() {
    let root = fixture_path("boundary-root");
    let boundaries = BoundaryConfig {
        preset: None,
        zones: vec![
            BoundaryZone {
                name: "ui".to_string(),
                patterns: vec!["src/**".to_string()],
                root: Some("packages/app/".to_string()),
            },
            BoundaryZone {
                name: "domain".to_string(),
                patterns: vec!["src/**".to_string()],
                root: Some("packages/core/".to_string()),
            },
        ],
        rules: vec![
            BoundaryRule {
                from: "ui".to_string(),
                allow: vec![],
            },
            BoundaryRule {
                from: "domain".to_string(),
                allow: vec![],
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
        ignore_exports: vec![],
        ignore_catalog_references: vec![],
        ignore_dependency_overrides: vec![],
        ignore_exports_used_in_file: fallow_config::IgnoreExportsUsedInFileConfig::default(),
        used_class_members: vec![],
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
        resolve: ResolveConfig::default(),
        sealed: false,
        include_entry_exports: false,
    }
    .resolve(root, OutputFormat::Human, 4, true, true);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    // Same flat pattern `src/**` is disambiguated by `root`: ui matches
    // packages/app/src/login.tsx, domain matches packages/core/src/order.ts.
    // The deny rule on "ui" fires on the cross-zone import.
    assert_eq!(
        results.boundary_violations.len(),
        1,
        "expected 1 boundary violation, got: {:?}",
        results
            .boundary_violations
            .iter()
            .map(|v| format!(
                "{} ({}) -> {} ({})",
                v.from_zone,
                v.from_path.display(),
                v.to_zone,
                v.to_path.display()
            ))
            .collect::<Vec<_>>()
    );

    let v = &results.boundary_violations[0];
    assert_eq!(v.from_zone, "ui");
    assert_eq!(v.to_zone, "domain");
    assert!(
        v.from_path
            .to_string_lossy()
            .replace('\\', "/")
            .ends_with("packages/app/src/login.tsx"),
        "from_path should end with packages/app/src/login.tsx, got: {}",
        v.from_path.display()
    );
    assert!(
        v.to_path
            .to_string_lossy()
            .replace('\\', "/")
            .ends_with("packages/core/src/order.ts"),
        "to_path should end with packages/core/src/order.ts, got: {}",
        v.to_path.display()
    );
}

#[test]
fn root_field_genuinely_disambiguates_flat_patterns() {
    // Without `root`, the flat patterns `src/**` would match BOTH files
    // and the first zone (`ui`) would steal both. With `root` the two
    // zones partition the subtrees correctly.
    let root = fixture_path("boundary-root");

    // Without root: flat `packages/*/src/**` collapses to a single zone
    // because "ui" matches first for both files. No violation possible
    // since the importer and target end up in the same zone.
    let flat_boundaries = BoundaryConfig {
        preset: None,
        zones: vec![BoundaryZone {
            name: "ui".to_string(),
            patterns: vec!["packages/*/src/**".to_string()],
            root: None,
        }],
        rules: vec![BoundaryRule {
            from: "ui".to_string(),
            allow: vec![],
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
        ignore_exports: vec![],
        ignore_catalog_references: vec![],
        ignore_dependency_overrides: vec![],
        ignore_exports_used_in_file: fallow_config::IgnoreExportsUsedInFileConfig::default(),
        used_class_members: vec![],
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
        resolve: ResolveConfig::default(),
        sealed: false,
        include_entry_exports: false,
    }
    .resolve(root.clone(), OutputFormat::Human, 4, true, true);
    let flat_results = fallow_core::analyze(&flat_config).expect("analysis should succeed");
    assert!(
        flat_results.boundary_violations.is_empty(),
        "without root, both files share the same zone so self-imports are allowed"
    );

    // With root: same internal pattern `src/**` partitions cleanly.
    let scoped_boundaries = BoundaryConfig {
        preset: None,
        zones: vec![
            BoundaryZone {
                name: "ui".to_string(),
                patterns: vec!["src/**".to_string()],
                root: Some("packages/app/".to_string()),
            },
            BoundaryZone {
                name: "domain".to_string(),
                patterns: vec!["src/**".to_string()],
                root: Some("packages/core/".to_string()),
            },
        ],
        rules: vec![BoundaryRule {
            from: "ui".to_string(),
            allow: vec![],
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
        ignore_exports: vec![],
        ignore_catalog_references: vec![],
        ignore_dependency_overrides: vec![],
        ignore_exports_used_in_file: fallow_config::IgnoreExportsUsedInFileConfig::default(),
        used_class_members: vec![],
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
        resolve: ResolveConfig::default(),
        sealed: false,
        include_entry_exports: false,
    }
    .resolve(root, OutputFormat::Human, 4, true, true);
    let scoped_results = fallow_core::analyze(&scoped_config).expect("analysis should succeed");
    assert_eq!(
        scoped_results.boundary_violations.len(),
        1,
        "with root, the cross-package import is now a cross-zone violation"
    );
    let v = &scoped_results.boundary_violations[0];
    assert_eq!(v.from_zone, "ui");
    assert_eq!(v.to_zone, "domain");
}

#[test]
fn bulletproof_preset_detects_violation() {
    let root = fixture_path("boundary-bulletproof");
    let boundaries = BoundaryConfig {
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
        ignore_exports: vec![],
        ignore_catalog_references: vec![],
        ignore_dependency_overrides: vec![],
        ignore_exports_used_in_file: fallow_config::IgnoreExportsUsedInFileConfig::default(),
        used_class_members: vec![],
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
        resolve: ResolveConfig::default(),
        sealed: false,
        include_entry_exports: false,
    }
    .resolve(root, OutputFormat::Human, 4, true, true);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    // features/auth/login.ts imports from app/page.ts — features zone cannot
    // import from app zone (only shared and server are allowed).
    assert_eq!(
        results.boundary_violations.len(),
        1,
        "expected 1 boundary violation, got: {:?}",
        results
            .boundary_violations
            .iter()
            .map(|v| format!(
                "{} ({}) -> {} ({})",
                v.from_zone,
                v.from_path.display(),
                v.to_zone,
                v.to_path.display()
            ))
            .collect::<Vec<_>>()
    );

    let v = &results.boundary_violations[0];
    assert_eq!(v.from_zone, "features");
    assert_eq!(v.to_zone, "app");
    assert!(
        v.from_path
            .to_string_lossy()
            .replace('\\', "/")
            .ends_with("src/features/auth/login.ts"),
        "from_path should end with src/features/auth/login.ts, got: {}",
        v.from_path.display()
    );
    assert!(
        v.to_path
            .to_string_lossy()
            .replace('\\', "/")
            .ends_with("src/app/page.ts"),
        "to_path should end with src/app/page.ts, got: {}",
        v.to_path.display()
    );
}
