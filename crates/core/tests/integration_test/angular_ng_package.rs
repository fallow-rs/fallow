use super::common::{create_config, fixture_path};

/// Issue #606: ng-packagr library packages declare their public API via
/// `ng-package.json` `lib.entryFile` (default `src/public_api.ts`). The Angular
/// plugin treats that file as an entry point so the library surface and
/// everything re-exported from it stays reachable instead of being flagged as
/// unused files / unused exports. Nested secondary-entry-point configs
/// (`client/ng-package.json`) are scanned too. A control file outside any
/// entry-file graph must remain flagged, proving the credit is scoped, not
/// project-wide.
#[test]
fn ng_package_entry_file_keeps_public_api_reachable() {
    let root = fixture_path("ng-package-entrypoint");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    // Full relative path suffixes (separator-normalized) so the primary
    // `src/public-api.ts` and the secondary `client/src/public-api.ts` (same
    // file name) can be distinguished.
    let unused_file_paths: Vec<String> = results
        .unused_files
        .iter()
        .map(|f| f.file.path.to_string_lossy().replace('\\', "/"))
        .collect();
    let is_unused = |suffix: &str| unused_file_paths.iter().any(|p| p.ends_with(suffix));

    // The primary entry file and the files reachable through its re-export
    // chain must not be reported as unused.
    for path in [
        "ng-package-entrypoint/src/public-api.ts",
        "ng-package-entrypoint/src/composables.ts",
        "ng-package-entrypoint/src/context.ts",
        // Secondary entry point: client/ng-package.json is empty, so the entry
        // file resolves to ng-packagr's default src/public_api.ts (underscore),
        // and the file reached through its re-export chain stays reachable.
        "ng-package-entrypoint/client/src/public_api.ts",
        "ng-package-entrypoint/client/src/client.ts",
    ] {
        assert!(
            !is_unused(path),
            "{path} should be reachable as ng-package public API: {unused_file_paths:?}"
        );
    }

    // The exports re-exported from the entry files are public API, not unused.
    let unused_export_names: Vec<&str> = results
        .unused_exports
        .iter()
        .map(|e| e.export.export_name.as_str())
        .collect();
    for name in [
        "useHead",
        "useHeadSafe",
        "headSymbol",
        "UnheadInjectionToken",
        "useClientHead",
    ] {
        assert!(
            !unused_export_names.contains(&name),
            "{name} is part of the public API surface, must not be unused: {unused_export_names:?}"
        );
    }

    // Control: a file outside any entry-file reachability graph stays flagged,
    // proving the ng-package credit is scoped to the public API, not the whole
    // project.
    assert!(
        is_unused("ng-package-entrypoint/src/internal-unused.ts"),
        "internal-unused.ts is unreachable and must stay flagged: {unused_file_paths:?}"
    );
}
