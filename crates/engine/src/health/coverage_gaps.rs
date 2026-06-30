use crate::{discover::FileId, source::ModuleInfo, suppress};
use fallow_graph::graph::{ModuleGraph, ModuleNode};
use fallow_output::{CoverageGapSummary, CoverageGaps, UntestedExport, UntestedFile};

pub struct CoverageGapData {
    pub report: CoverageGaps,
    pub runtime_paths: Vec<std::path::PathBuf>,
}

pub(super) fn build_coverage_summary(
    runtime_files: usize,
    covered_files: usize,
    untested_files: usize,
    untested_exports: usize,
) -> CoverageGapSummary {
    let file_coverage_pct = if runtime_files == 0 {
        100.0
    } else {
        ((covered_files as f64 / runtime_files as f64) * 1000.0).round() / 10.0
    };

    CoverageGapSummary {
        runtime_files,
        covered_files,
        file_coverage_pct,
        untested_files,
        untested_exports,
    }
}

/// Whether a path is a stylesheet excluded from runtime coverage gaps.
fn is_excluded_coverage_extension(path: &std::path::Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|ext| matches!(ext, "css" | "scss" | "less" | "sass"))
}

/// Whether the module opted out of coverage-gap reporting via a file suppression.
fn module_is_coverage_suppressed(module: Option<&ModuleInfo>) -> bool {
    module.is_some_and(|m| {
        suppress::is_file_suppressed(
            &m.suppressions,
            fallow_types::suppress::IssueKind::CoverageGaps,
        )
    })
}

/// Append untested value exports of one node (those with no test-reachable
/// reference and not already flagged unused) to `exports`.
fn collect_untested_exports(
    exports: &mut Vec<UntestedExport>,
    graph: &ModuleGraph,
    node: &ModuleNode,
    module: &ModuleInfo,
    path: &std::path::Path,
    unused_exports: &rustc_hash::FxHashSet<(&std::path::Path, String)>,
) {
    for export in &node.exports {
        if export.is_type_only {
            continue;
        }
        if unused_exports.contains(&(path, export.name.to_string())) {
            continue;
        }

        let has_test_dependency = export.references.iter().any(|reference| {
            graph
                .modules
                .get(reference.from_file.0 as usize)
                .is_some_and(|module| module.is_test_reachable())
        });
        if has_test_dependency {
            continue;
        }

        let (line, col) =
            fallow_types::extract::byte_offset_to_line_col(&module.line_offsets, export.span.start);
        exports.push(UntestedExport {
            path: path.to_path_buf(),
            export_name: export.name.to_string(),
            line,
            col,
        });
    }
}

/// Accumulated coverage-gap scan results before sorting and wrapping.
struct CoverageGapScan {
    runtime_files: usize,
    covered_files: usize,
    runtime_paths: Vec<std::path::PathBuf>,
    files: Vec<UntestedFile>,
    exports: Vec<UntestedExport>,
}

/// Walk runtime-reachable modules, collecting untested files and exports.
fn scan_runtime_files(
    graph: &ModuleGraph,
    file_paths: &rustc_hash::FxHashMap<FileId, &std::path::PathBuf>,
    module_by_id: &rustc_hash::FxHashMap<FileId, &ModuleInfo>,
    unused_exports: &rustc_hash::FxHashSet<(&std::path::Path, String)>,
) -> CoverageGapScan {
    let mut scan = CoverageGapScan {
        runtime_files: 0,
        covered_files: 0,
        runtime_paths: Vec::new(),
        files: Vec::new(),
        exports: Vec::new(),
    };

    for node in &graph.modules {
        if !node.is_runtime_reachable() {
            continue;
        }

        let Some(path) = file_paths.get(&node.file_id) else {
            continue;
        };

        if is_excluded_coverage_extension(path) {
            continue;
        }

        let module = module_by_id.get(&node.file_id).copied();
        if module_is_coverage_suppressed(module) {
            continue;
        }

        scan.runtime_paths.push((*path).clone());

        scan.runtime_files += 1;
        if node.is_test_reachable() {
            scan.covered_files += 1;
        } else {
            scan.files.push(UntestedFile {
                path: (*path).clone(),
                value_export_count: node.exports.iter().filter(|e| !e.is_type_only).count(),
            });
        }

        let Some(module) = module else {
            continue;
        };

        collect_untested_exports(&mut scan.exports, graph, node, module, path, unused_exports);
    }

    scan
}

/// Sort, wrap, and summarize the scan results into the final report data.
fn build_coverage_gap_data(scan: CoverageGapScan, root: &std::path::Path) -> CoverageGapData {
    let CoverageGapScan {
        runtime_files,
        covered_files,
        runtime_paths,
        mut files,
        mut exports,
    } = scan;

    files.sort_by(|a, b| a.path.cmp(&b.path));
    exports.sort_by(|a, b| {
        a.path
            .cmp(&b.path)
            .then_with(|| a.export_name.cmp(&b.export_name))
            .then_with(|| a.line.cmp(&b.line))
    });

    let untested_file_count = files.len();
    let untested_export_count = exports.len();
    let wrapped_files: Vec<fallow_output::UntestedFileFinding> = files
        .into_iter()
        .map(|file| fallow_output::UntestedFileFinding::with_actions(file, root))
        .collect();
    let wrapped_exports: Vec<fallow_output::UntestedExportFinding> = exports
        .into_iter()
        .map(|export| fallow_output::UntestedExportFinding::with_actions(export, root))
        .collect();

    CoverageGapData {
        report: CoverageGaps {
            summary: build_coverage_summary(
                runtime_files,
                covered_files,
                untested_file_count,
                untested_export_count,
            ),
            files: wrapped_files,
            exports: wrapped_exports,
        },
        runtime_paths,
    }
}

pub(super) fn compute_coverage_gaps(
    graph: &ModuleGraph,
    file_paths: &rustc_hash::FxHashMap<FileId, &std::path::PathBuf>,
    module_by_id: &rustc_hash::FxHashMap<FileId, &ModuleInfo>,
    unused_exports: &rustc_hash::FxHashSet<(&std::path::Path, String)>,
    root: &std::path::Path,
) -> CoverageGapData {
    let scan = scan_runtime_files(graph, file_paths, module_by_id, unused_exports);
    build_coverage_gap_data(scan, root)
}
