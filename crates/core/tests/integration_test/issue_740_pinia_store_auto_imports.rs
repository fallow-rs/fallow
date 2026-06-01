//! Issue #740: Pinia store auto-import resolution for Nuxt.

use std::path::Path;

use super::common::{create_config, fixture_path};
use fallow_types::results::AnalysisResults;

fn normalize_path(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

fn unused_file_paths(results: &AnalysisResults, root: &Path) -> Vec<String> {
    results
        .unused_files
        .iter()
        .map(|finding| normalize_path(root, &finding.file.path))
        .collect()
}

fn unused_exports(results: &AnalysisResults, root: &Path) -> Vec<(String, String)> {
    results
        .unused_exports
        .iter()
        .map(|finding| {
            (
                normalize_path(root, &finding.export.path),
                finding.export.export_name.clone(),
            )
        })
        .collect()
}

#[test]
fn pinia_store_auto_imports_keep_direct_store_files_reachable() {
    let root = fixture_path("nuxt-pinia-store-auto-imports");
    let config = create_config(root.clone());

    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let unused = unused_file_paths(&results, &root);

    for reachable in ["stores/user.ts", "app/stores/settings.ts"] {
        assert!(
            !unused.contains(&reachable.to_string()),
            "{reachable} should be reachable via Pinia store auto-imports: {unused:?}"
        );
    }

    assert!(
        unused.contains(&"stores/admin/user.ts".to_string()),
        "nested stores should stay outside default Pinia storesDirs: {unused:?}"
    );
}

#[test]
fn include_entry_exports_credits_pinia_store_exports() {
    let root = fixture_path("nuxt-pinia-store-auto-imports");
    let mut config = create_config(root.clone());
    config.include_entry_exports = true;

    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let unused = unused_exports(&results, &root);

    for used in [
        ("stores/user.ts", "useUserStore"),
        ("app/stores/settings.ts", "useSettingsStore"),
    ] {
        assert!(
            !unused.contains(&(used.0.to_string(), used.1.to_string())),
            "{used:?} should be credited by synthesized Pinia import edges: {unused:?}"
        );
    }

    assert!(
        unused.contains(&(
            "stores/user.ts".to_string(),
            "unusedStoreHelper".to_string()
        )),
        "unreferenced sibling exports should still report: {unused:?}"
    );
}

#[test]
fn package_without_pinia_nuxt_does_not_credit_store_files() {
    let root = fixture_path("nuxt-pinia-store-auto-imports-disabled");
    let config = create_config(root.clone());

    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let unused = unused_file_paths(&results, &root);

    assert!(
        unused.contains(&"stores/user.ts".to_string()),
        "Pinia store auto-import rules should activate only on @pinia/nuxt: {unused:?}"
    );
}
