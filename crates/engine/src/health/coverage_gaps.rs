use crate::{discover::FileId, source::ModuleInfo, suppress};
use fallow_graph::graph::{
    EffectiveExportResolution, ExportNamespace, ExportSymbol, ModuleGraph, ModuleNode,
    is_declaration_file_path,
};
use fallow_output::{CoverageGapSummary, CoverageGaps, UntestedExport, UntestedFile};
use fallow_types::extract::{ExportName, ImportedName, SemanticFact};

use crate::module_graph::StaticTestCoverage;

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

/// Whether a module declares no runtime code, so no test can run it.
///
/// A declaration file qualifies. Other modules qualify when they export at
/// least one name, no export is a value export (see [`is_value_export`]), every
/// star re-export is type-only, and they have no runtime statement or
/// declaration, function, CommonJS export, side-effect import, dynamic import,
/// or `require()` call. A module with no exports can run side effects, so it
/// does not qualify. Each named re-export also has an entry in `node.exports`.
fn declares_only_types(graph: &ModuleGraph, node: &ModuleNode, module: &ModuleInfo) -> bool {
    if is_declaration_file_path(&node.path) {
        return true;
    }
    let exports_something = !node.exports.is_empty() || !node.re_exports.is_empty();
    exports_something
        && node
            .exports
            .iter()
            .all(|export| !is_value_export(graph, node.file_id, export))
        && node
            .re_exports
            .iter()
            .all(|re_export| re_export.is_type_only || re_export.exported_name != "*")
        && !module.has_cjs_exports
        && !module
            .semantic_facts
            .contains(&SemanticFact::TopLevelRuntimeStatement)
        && module.complexity.is_empty()
        && module.dynamic_imports.is_empty()
        && module.dynamic_import_patterns.is_empty()
        && module.require_calls.is_empty()
        && module
            .imports
            .iter()
            .all(|import| !matches!(import.imported_name, ImportedName::SideEffect))
}

/// Whether an export can carry runtime code.
///
/// A re-export without the `type` keyword is not type-only in the graph, but
/// TypeScript erases it when its name resolves only to a type alias or an
/// interface (issue #5361). Such an export has a type binding and no value
/// binding. An export with no binding in either namespace, for example a
/// re-export from an unresolved package, stays a value export.
fn is_value_export(graph: &ModuleGraph, file_id: FileId, export: &ExportSymbol) -> bool {
    if export.is_type_only {
        return false;
    }
    let name = match &export.name {
        ExportName::Named(name) => name.as_str(),
        ExportName::Default => "default",
    };
    let resolves_in = |namespace| {
        !matches!(
            graph.resolve_export(file_id, name, namespace),
            EffectiveExportResolution::Missing
        )
    };
    resolves_in(ExportNamespace::Value) || !resolves_in(ExportNamespace::Type)
}

/// Untested value exports of one module: those with no test-reachable
/// reference and not already flagged unused. The flag marks an export that
/// only tests which mock its module reference.
fn untested_exports(
    test_coverage: StaticTestCoverage<'_>,
    file_id: FileId,
    value_exports: &[&ExportSymbol],
    module: &ModuleInfo,
    path: &std::path::Path,
    unused_exports: &rustc_hash::FxHashSet<(&std::path::Path, String)>,
) -> Vec<(UntestedExport, bool)> {
    let mut exports = Vec::new();
    for &export in value_exports {
        if unused_exports.contains(&(path, export.name.to_string())) {
            continue;
        }

        let has_test_dependency = test_coverage.covers_any_reference(file_id, export);
        if has_test_dependency {
            continue;
        }

        let (line, col) =
            fallow_types::extract::byte_offset_to_line_col(&module.line_offsets, export.span.start);
        let mocked_only = test_coverage.is_mocked_only(file_id, export);
        exports.push((
            UntestedExport {
                path: path.to_path_buf(),
                export_name: export.name.to_string(),
                line,
                col,
            },
            mocked_only,
        ));
    }
    exports
}

/// Accumulated coverage-gap scan results before sorting and wrapping.
struct CoverageGapScan {
    runtime_files: usize,
    covered_files: usize,
    runtime_paths: Vec<std::path::PathBuf>,
    files: Vec<UntestedFile>,
    exports: Vec<(UntestedExport, bool)>,
}

/// Walk runtime-reachable modules, collecting untested files and exports.
fn scan_runtime_files(
    graph: &ModuleGraph,
    test_coverage: StaticTestCoverage<'_>,
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

    for node in graph.modules() {
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
        if module.is_some_and(|module| declares_only_types(graph, node, module)) {
            continue;
        }

        scan.runtime_paths.push((*path).clone());
        let value_exports: Vec<&ExportSymbol> = node
            .exports
            .iter()
            .filter(|export| is_value_export(graph, node.file_id, export))
            .collect();

        scan.runtime_files += 1;
        if test_coverage.covers_file(node.file_id) {
            scan.covered_files += 1;
        } else {
            scan.files.push(UntestedFile {
                path: (*path).clone(),
                value_export_count: value_exports.len(),
            });
        }

        let Some(module) = module else {
            continue;
        };

        scan.exports.extend(untested_exports(
            test_coverage,
            node.file_id,
            &value_exports,
            module,
            path,
            unused_exports,
        ));
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
    exports.sort_by(|(a, _), (b, _)| {
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
        .map(|(export, mocked_only)| {
            fallow_output::UntestedExportFinding::with_reach_actions(export, root, mocked_only)
        })
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
    test_coverage: StaticTestCoverage<'_>,
    file_paths: &rustc_hash::FxHashMap<FileId, &std::path::PathBuf>,
    module_by_id: &rustc_hash::FxHashMap<FileId, &ModuleInfo>,
    unused_exports: &rustc_hash::FxHashSet<(&std::path::Path, String)>,
    root: &std::path::Path,
) -> CoverageGapData {
    let scan = scan_runtime_files(
        graph,
        test_coverage,
        file_paths,
        module_by_id,
        unused_exports,
    );
    build_coverage_gap_data(scan, root)
}
