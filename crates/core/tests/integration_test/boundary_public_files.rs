//! Boundary zones with public entry files (`publicFiles`, issue #4760).
//!
//! In the `boundary-public-files` fixture, `src/billing/public.ts` is the public
//! entry file of `billing`, and `src/payments/index.ts` is the public entry
//! file of `payments`. The `orders` zone may import `billing` and `shared`.
//! The `shipping` zone may import `shared`, plus type-only imports of
//! `billing`. The `shared` zone has no rule.

use std::collections::BTreeSet;

use fallow_types::results::BoundaryViolationReason;

use super::*;

const FIXTURE: &str = "boundary-public-files";

fn explicit_zones(public_files: bool) -> BoundaryConfig {
    let globs = |glob: &str| {
        if public_files {
            vec![glob.to_string()]
        } else {
            vec![]
        }
    };
    BoundaryConfig {
        zones: vec![
            BoundaryZone {
                public_files: globs("src/billing/public.ts"),
                ..zone("billing", &["src/billing/**"])
            },
            BoundaryZone {
                public_files: globs("src/payments/index.ts"),
                ..zone("payments", &["src/payments/**"])
            },
            zone("orders", &["src/orders/**"]),
            zone("shipping", &["src/shipping/**"]),
            zone("shared", &["src/shared/**"]),
        ],
        rules: vec![
            BoundaryRule {
                from: "orders".to_string(),
                allow: vec!["billing".to_string(), "shared".to_string()],
                allow_type_only: vec![],
            },
            BoundaryRule {
                from: "shipping".to_string(),
                allow: vec!["shared".to_string()],
                allow_type_only: vec!["billing".to_string()],
            },
        ],
        ..BoundaryConfig::default()
    }
}

/// One finding as `(from_path, to_path, to_zone, via_path, reason,
/// public_files)`, with paths relative to the fixture root.
type Finding = (
    String,
    String,
    String,
    Option<String>,
    BoundaryViolationReason,
    Vec<String>,
);

fn findings(boundaries: BoundaryConfig) -> BTreeSet<Finding> {
    let root = fixture_path(FIXTURE);
    let config = create_boundary_config_with_entry(root.clone(), boundaries, "src/index.ts");
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    results
        .boundary_violations
        .iter()
        .map(|finding| {
            let v = &finding.violation;
            (
                relative_to(&v.from_path, &root),
                relative_to(&v.to_path, &root),
                v.to_zone.clone(),
                v.via_path.as_deref().map(|via| relative_to(via, &root)),
                v.reason,
                v.public_files
                    .iter()
                    .map(|path| relative_to(path, &root))
                    .collect(),
            )
        })
        .collect()
}

fn disallowed(from: &str, to: &str, to_zone: &str, via: Option<&str>) -> Finding {
    (
        from.to_string(),
        to.to_string(),
        to_zone.to_string(),
        via.map(str::to_string),
        BoundaryViolationReason::DisallowedZone,
        vec![],
    )
}

fn internal(from: &str, to: &str, to_zone: &str, public_files: &str) -> Finding {
    (
        from.to_string(),
        to.to_string(),
        to_zone.to_string(),
        None,
        BoundaryViolationReason::InternalFile,
        vec![public_files.to_string()],
    )
}

#[test]
fn other_zones_import_a_zone_only_through_its_public_entry_files() {
    let found = findings(explicit_zones(true));

    let expected: BTreeSet<Finding> = [
        // A direct import of an internal file of an allowed zone.
        internal(
            "src/orders/direct.ts",
            "src/billing/invoice.ts",
            "billing",
            "src/billing/public.ts",
        ),
        // A barrel inside the zone that is not a public entry file.
        internal(
            "src/orders/via-internal-barrel.ts",
            "src/billing/internal-barrel.ts",
            "billing",
            "src/billing/public.ts",
        ),
        // A zone without a rule must also use the entry file. The finding is
        // on the re-export, so `orders/via-leak.ts` gets no second finding.
        internal(
            "src/shared/leak.ts",
            "src/billing/invoice.ts",
            "billing",
            "src/billing/public.ts",
        ),
        // `allowTypeOnly` allows the zone, not its internal files.
        internal(
            "src/shipping/type-deep.ts",
            "src/billing/types.ts",
            "billing",
            "src/billing/public.ts",
        ),
        // A deep import of a forbidden zone gets only the zone finding.
        disallowed(
            "src/orders/forbidden.ts",
            "src/payments/charge.ts",
            "payments",
            None,
        ),
        // The chain walk stops at the public entry file, and `shipping` may
        // not import a value from `billing`.
        disallowed(
            "src/shipping/via-shared.ts",
            "src/billing/public.ts",
            "billing",
            Some("src/shared/index.ts"),
        ),
    ]
    .into_iter()
    .collect();

    assert_eq!(found, expected);
}

#[test]
fn auto_discover_scopes_public_entry_files_to_each_module() {
    let boundaries = BoundaryConfig {
        zones: vec![BoundaryZone {
            name: "modules".to_string(),
            auto_discover: vec!["src".to_string()],
            public_files: vec!["public.ts".to_string(), "index.ts".to_string()],
            ..BoundaryZone::default()
        }],
        rules: vec![BoundaryRule {
            from: "modules/orders".to_string(),
            allow: vec!["modules/billing".to_string(), "modules/shared".to_string()],
            allow_type_only: vec![],
        }],
        ..BoundaryConfig::default()
    };

    let found = findings(boundaries);

    assert!(
        found.contains(&internal(
            "src/orders/direct.ts",
            "src/billing/invoice.ts",
            "modules/billing",
            "src/billing/public.ts",
        )),
        "the public globs of a discovered module name only its own files, got {found:?}"
    );
    assert!(
        found.contains(&disallowed(
            "src/orders/forbidden.ts",
            "src/payments/charge.ts",
            "modules/payments",
            None,
        )),
        "the rule allows one discovered module and not another, got {found:?}"
    );
    assert!(
        found.contains(&internal(
            "src/orders/via-star.ts",
            "src/shared/star.ts",
            "modules/shared",
            "src/shared/index.ts",
        )),
        "`index.ts` is the public entry file of `shared`, so `star.ts` is internal, got {found:?}"
    );
    assert!(
        found
            .iter()
            .all(|f| f.0 != "src/orders/via-public.ts" && f.0 != "src/orders/via-shared.ts"),
        "imports through public entry files are allowed, got {found:?}"
    );
}

#[test]
fn without_public_files_a_zone_rule_allows_every_file_of_the_zone() {
    let found = findings(explicit_zones(false));

    assert!(
        found.iter().all(
            |f| f.4 == BoundaryViolationReason::DisallowedZone && f.0 != "src/orders/direct.ts"
        ),
        "without publicFiles, `orders` may import any file of `billing`, got {found:?}"
    );
    assert!(
        found.contains(&disallowed(
            "src/shipping/via-shared.ts",
            "src/billing/invoice.ts",
            "billing",
            Some("src/shared/index.ts"),
        )),
        "without publicFiles, the chain reaches the declaring file, got {found:?}"
    );
}
