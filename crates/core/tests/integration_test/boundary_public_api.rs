//! Boundary zones with `publicApi: true` (issue #4760).
//!
//! The `boundary-public-api` fixture has a `billing` module with one public
//! entry file, `src/billing/public.ts`. The `orders` zone may import the
//! entry file and `shared`. The `shipping` zone may import only `shared`,
//! plus type-only imports of the entry file.

use std::collections::BTreeSet;

use super::*;

const FIXTURE: &str = "boundary-public-api";

fn public_api_boundaries(public_api: bool) -> BoundaryConfig {
    BoundaryConfig {
        zones: vec![
            BoundaryZone {
                public_api,
                ..zone("billing-public", &["src/billing/public.ts"])
            },
            zone("billing", &["src/billing/**"]),
            zone("orders", &["src/orders/**"]),
            zone("shipping", &["src/shipping/**"]),
            zone("shared", &["src/shared/**"]),
        ],
        rules: vec![
            BoundaryRule {
                from: "orders".to_string(),
                allow: vec!["billing-public".to_string(), "shared".to_string()],
                allow_type_only: vec![],
            },
            BoundaryRule {
                from: "shipping".to_string(),
                allow: vec!["shared".to_string()],
                allow_type_only: vec!["billing-public".to_string()],
            },
        ],
        ..BoundaryConfig::default()
    }
}

/// One finding as `(from_path, to_path, to_zone, via_path)`, relative to the
/// fixture root.
type Finding = (String, String, String, Option<String>);

fn findings(public_api: bool) -> BTreeSet<Finding> {
    let root = fixture_path(FIXTURE);
    let config = create_boundary_config_with_entry(
        root.clone(),
        public_api_boundaries(public_api),
        "src/index.ts",
    );
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    results
        .boundary_violations
        .iter()
        .map(|v| {
            (
                relative_to(&v.violation.from_path, &root),
                relative_to(&v.violation.to_path, &root),
                v.violation.to_zone.clone(),
                v.violation
                    .via_path
                    .as_deref()
                    .map(|via| relative_to(via, &root)),
            )
        })
        .collect()
}

fn finding(from: &str, to: &str, to_zone: &str, via: Option<&str>) -> Finding {
    (
        from.to_string(),
        to.to_string(),
        to_zone.to_string(),
        via.map(str::to_string),
    )
}

#[test]
fn imports_through_a_public_entry_file_are_judged_by_the_entry_file() {
    let found = findings(true);

    let expected: BTreeSet<Finding> = [
        // A direct import of an internal file stays a violation.
        finding(
            "src/orders/direct.ts",
            "src/billing/invoice.ts",
            "billing",
            None,
        ),
        // A barrel inside the module, outside the public zone, is internal.
        finding(
            "src/orders/via-internal-barrel.ts",
            "src/billing/internal-barrel.ts",
            "billing",
            None,
        ),
        // A barrel that is not public does not hide its origin.
        finding(
            "src/orders/via-leak.ts",
            "src/billing/invoice.ts",
            "billing",
            Some("src/shared/leak.ts"),
        ),
        // The chain walk stops at the public entry file, and `shipping` may
        // not import a value from it.
        finding(
            "src/shipping/via-shared.ts",
            "src/billing/public.ts",
            "billing-public",
            Some("src/shared/index.ts"),
        ),
    ]
    .into_iter()
    .collect();

    assert_eq!(found, expected);
}

#[test]
fn without_public_api_the_origin_zone_judges_the_import() {
    let found = findings(false);

    assert!(
        found.contains(&finding(
            "src/orders/via-public.ts",
            "src/billing/invoice.ts",
            "billing",
            Some("src/billing/public.ts"),
        )),
        "a zone without publicApi keeps the origin-module verdict, got {found:?}"
    );
    assert!(
        found.contains(&finding(
            "src/orders/via-star.ts",
            "src/billing/invoice.ts",
            "billing",
            Some("src/shared/star.ts"),
        )),
        "an `export *` chain reaches the origin, got {found:?}"
    );
    assert!(
        found.contains(&finding(
            "src/orders/via-shared.ts",
            "src/billing/invoice.ts",
            "billing",
            Some("src/shared/index.ts"),
        )),
        "a chain through a plain barrel reaches the origin, got {found:?}"
    );
}
