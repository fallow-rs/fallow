use fallow_config::{FallowConfig, OutputFormat};

use crate::common::fixture_path;

/// Capability E: fallow ALREADY reports route-internal unused exports today.
/// On a `next`-enabled App Router page, a stray non-allowlisted export and a
/// typo'd route-export name surface as `unused_exports` while the valid
/// `metadata` and `default` are credited by the nextjs plugin's precise
/// used-exports allowlist. `--include-entry-exports` subjects the entry file's
/// exports to unused-export detection (still honoring plugin allowlists).
///
/// This proves the knip-can't-but-fallow-does gap: a typo like `meatdata`
/// (instead of `metadata`) and a stray `helper` are caught, while the real
/// framework exports are not falsely reported.
#[test]
fn typo_and_stray_route_exports_surface_while_valid_ones_are_credited() {
    let config = FallowConfig {
        include_entry_exports: true,
        ..Default::default()
    }
    .resolve(
        fixture_path("capability-e-route-exports"),
        OutputFormat::Human,
        4,
        true,
        true,
        None,
    );

    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused: Vec<&str> = results
        .unused_exports
        .iter()
        .filter(|e| {
            e.export
                .path
                .to_string_lossy()
                .replace('\\', "/")
                .ends_with("app/page.tsx")
        })
        .map(|e| e.export.export_name.as_str())
        .collect();

    assert!(
        unused.contains(&"meatdata"),
        "typo'd route export `meatdata` should be reported as unused: {unused:?}"
    );
    assert!(
        unused.contains(&"helper"),
        "stray non-route export `helper` should be reported as unused: {unused:?}"
    );
    assert!(
        !unused.contains(&"metadata"),
        "valid route export `metadata` must NOT be reported: {unused:?}"
    );
    assert!(
        !unused.contains(&"default"),
        "the default export must NOT be reported: {unused:?}"
    );
}
