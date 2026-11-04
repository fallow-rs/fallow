//! The config quality signal at the end of the human check report.

use super::*;

/// Emit a config-quality advisory to stderr when unused files are dominated by one directory.
///
/// Called from `print_human` (not `build_human_lines`) so it respects the `quiet` flag
/// and doesn't fire as a side effect during line-building.
pub(super) fn emit_config_quality_signal(results: &AnalysisResults, root: &Path) {
    if results.unused_files.len() <= 50 {
        return;
    }
    let mut dir_counts: rustc_hash::FxHashMap<String, usize> = rustc_hash::FxHashMap::default();
    for f in &results.unused_files {
        let rel = relative_path(&f.file.path, root);
        if let Some(first) = rel.components().next() {
            *dir_counts
                .entry(first.as_os_str().to_string_lossy().to_string())
                .or_insert(0) += 1;
        }
    }
    let total = results.unused_files.len();
    if let Some((dominant_dir, count)) = dir_counts.iter().max_by_key(|(_, c)| **c) {
        let pct = (*count as f64 / total as f64) * 100.0;
        if pct > 80.0 {
            let is_source_dir =
                matches!(dominant_dir.as_str(), "packages" | "src" | "lib" | "apps");
            let summary = format!("Note: {pct:.0}% of unused files are under {dominant_dir}/.");
            let advice: &[&str] = if is_source_dir {
                &["Run `fallow list --entry-points` to verify entry-point detection."]
            } else {
                &[
                    "Add it to ignorePatterns, or use --production to analyze only",
                    "production entry points.",
                ]
            };
            eprintln!("  {}", summary.yellow());
            for line in advice {
                eprintln!("  {}", line.yellow());
            }
            eprintln!(
                "  {}",
                "https://fallow.tools/docs/explanations/dead-code/#unused-files".yellow()
            );
        }
    }
}
