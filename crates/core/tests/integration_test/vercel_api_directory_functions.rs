//! Vercel deploys each file under `api/` as a serverless function. A project
//! with only a `vercel.json` (no `vercel` dependency) must credit those files
//! and their handler exports, but files and directories that start with `_`
//! are not deployed, so they stay ordinary modules.

use std::path::Path;

use super::common::{create_config, fixture_path};
use fallow_types::results::AnalysisResults;

#[test]
fn vercel_api_directory_files_are_function_entry_points() {
    let root = fixture_path("vercel-api-directory-functions");
    let mut config = create_config(root.clone());
    config.include_entry_exports = true;
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let unused_files = unused_file_paths(&results, &root);
    let unused_exports = unused_export_names(&results, &root);

    for function in ["api/hello.ts", "api/users/list.ts", "api/_lib/util.ts"] {
        assert!(
            !unused_files.contains(&function.to_string()),
            "{function} is deployed or imported and must not be unused, got {unused_files:?}"
        );
    }
    for private in ["api/_draft.ts", "api/_lib/orphan.ts"] {
        assert!(
            unused_files.contains(&private.to_string()),
            "{private} is not a Vercel function and nothing imports it, got {unused_files:?}"
        );
    }

    for credited in [
        "api/hello.ts:default",
        "api/hello.ts:config",
        "api/users/list.ts:GET",
        "api/users/list.ts:POST",
    ] {
        assert!(
            !unused_exports.contains(&credited.to_string()),
            "{credited} is a Vercel handler export and must be credited, got {unused_exports:?}"
        );
    }
    assert!(
        unused_exports.contains(&"api/users/list.ts:pageSize".to_string()),
        "a non-handler export of a function file must still be reported, got {unused_exports:?}"
    );
}

fn relative_path(path: &Path, root: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

fn unused_file_paths(results: &AnalysisResults, root: &Path) -> Vec<String> {
    results
        .unused_files
        .iter()
        .map(|finding| relative_path(&finding.file.path, root))
        .collect()
}

fn unused_export_names(results: &AnalysisResults, root: &Path) -> Vec<String> {
    results
        .unused_exports
        .iter()
        .map(|finding| {
            format!(
                "{}:{}",
                relative_path(&finding.export.path, root),
                finding.export.export_name
            )
        })
        .collect()
}
