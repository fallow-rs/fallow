use std::path::Path;

use fallow_core::results::AnalysisResults;

pub fn normalize_path(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

pub fn collect_unused_files(root: &Path, results: &AnalysisResults) -> Vec<String> {
    results
        .unused_files
        .iter()
        .map(|file| normalize_path(root, &file.file.path))
        .collect()
}

pub fn collect_unused_exports(root: &Path, results: &AnalysisResults) -> Vec<(String, String)> {
    results
        .unused_exports
        .iter()
        .map(|export| {
            (
                normalize_path(root, &export.export.path),
                export.export.export_name.clone(),
            )
        })
        .collect()
}

pub fn has_unused_export(unused_exports: &[(String, String)], path: &str, export: &str) -> bool {
    unused_exports
        .iter()
        .any(|(unused_path, unused_export)| unused_path == path && unused_export == export)
}
