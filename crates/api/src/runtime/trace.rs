use fallow_engine::session::AnalysisSession;
use fallow_types::duplicates::DuplicationReport;
use rustc_hash::FxHashSet;

use crate::{
    ProgrammaticAnalysisContext, ProgrammaticError, TraceCloneOptions,
    TraceCloneProgrammaticOutput, TraceCloneTarget, TraceDependencyOptions,
    TraceDependencyProgrammaticOutput, TraceExportOptions, TraceExportProgrammaticOutput,
    TraceExportTargetOutput, TraceFileOptions, TraceFileProgrammaticOutput,
};

use super::{ProgrammaticResult, duplication, resolve_programmatic_analysis_context};

struct TraceArtifacts {
    graph: fallow_engine::module_graph::RetainedModuleGraph,
    script_used_packages: FxHashSet<String>,
}

/// Trace why an export is considered used or unused.
///
/// # Errors
///
/// Returns a structured programmatic error for invalid options, config load
/// failures, graph construction failures, or missing trace targets.
pub fn run_trace_export(
    options: &TraceExportOptions,
) -> ProgrammaticResult<TraceExportProgrammaticOutput> {
    validate_non_empty("file", &options.file)?;
    validate_non_empty("export_name", &options.export_name)?;
    let resolved = resolve_programmatic_analysis_context(&options.analysis)?;
    resolved.install(|| {
        let session = load_trace_session(&resolved)?;
        let artifacts = trace_artifacts(&session)?;
        // Resolve a top-level export first; on a miss fall back to a class /
        // enum / store member trace so the MCP tool and Code Mode match the
        // CLI's `--trace FILE:MEMBER` behavior instead of a hard not-found
        // (issue #1744).
        let output = if let Some(export) = fallow_engine::trace::trace_export(
            &artifacts.graph,
            session.root(),
            &options.file,
            &options.export_name,
        ) {
            TraceExportTargetOutput::Export(export)
        } else if let Some(member) = fallow_engine::trace::trace_class_member(
            &artifacts.graph,
            session.root(),
            &options.file,
            &options.export_name,
        ) {
            TraceExportTargetOutput::Member(member)
        } else {
            return Err(ProgrammaticError::new(
                format!(
                    "export or member '{}' not found in '{}'",
                    options.export_name, options.file
                ),
                2,
            )
            .with_code("FALLOW_TRACE_TARGET_NOT_FOUND")
            .with_help(
                "The name is neither a top-level export nor a class / enum / store member of this \
                 file. Run trace_file on the file to list its exports, or project_info for the \
                 project symbol set; confirm the file path is project-relative.",
            )
            .with_context("trace_export"));
        };
        Ok(TraceExportProgrammaticOutput { output })
    })
}

/// Trace all graph edges for a file.
///
/// # Errors
///
/// Returns a structured programmatic error for invalid options, config load
/// failures, graph construction failures, or missing trace targets.
pub fn run_trace_file(
    options: &TraceFileOptions,
) -> ProgrammaticResult<TraceFileProgrammaticOutput> {
    validate_non_empty("file", &options.file)?;
    let resolved = resolve_programmatic_analysis_context(&options.analysis)?;
    resolved.install(|| {
        let session = load_trace_session(&resolved)?;
        let artifacts = trace_artifacts(&session)?;
        let output =
            fallow_engine::trace::trace_file(&artifacts.graph, session.root(), &options.file)
                .ok_or_else(|| {
                    ProgrammaticError::new(
                        format!("file '{}' not found in module graph", options.file),
                        2,
                    )
                    .with_code("FALLOW_TRACE_TARGET_NOT_FOUND")
                    .with_help(
                        "The file is not in the analyzed module graph. Run project_info to list \
                         discovered files; the path must be project-relative and not excluded by \
                         ignore patterns or outside the analyzed roots.",
                    )
                    .with_context("trace_file")
                })?;
        Ok(TraceFileProgrammaticOutput { output })
    })
}

/// Trace where a dependency is used.
///
/// # Errors
///
/// Returns a structured programmatic error for invalid options, config load, or
/// graph construction failures.
pub fn run_trace_dependency(
    options: &TraceDependencyOptions,
) -> ProgrammaticResult<TraceDependencyProgrammaticOutput> {
    validate_non_empty("package_name", &options.package_name)?;
    let resolved = resolve_programmatic_analysis_context(&options.analysis)?;
    resolved.install(|| {
        let session = load_trace_session(&resolved)?;
        let artifacts = trace_artifacts(&session)?;
        let output = fallow_engine::trace::trace_dependency(
            &artifacts.graph,
            session.root(),
            &options.package_name,
            &artifacts.script_used_packages,
        );
        Ok(TraceDependencyProgrammaticOutput { output })
    })
}

/// Trace duplicate-code groups by location or stable fingerprint.
///
/// # Errors
///
/// Returns a structured programmatic error for invalid options, config load
/// failures, duplicate detection failures, or missing trace targets.
pub fn run_trace_clone(
    options: &TraceCloneOptions,
) -> ProgrammaticResult<TraceCloneProgrammaticOutput> {
    validate_trace_clone_target(&options.target)?;
    let resolved = resolve_programmatic_analysis_context(&options.duplication.analysis)?;
    resolved.install(|| {
        let session = duplication::load_duplication_session(&options.duplication, &resolved)?;
        let dupes_config =
            duplication::build_dupes_config(&options.duplication, &session.config().duplicates);
        let cache_dir = (!resolved.no_cache).then_some(session.config().cache_dir.as_path());
        let report = session
            .find_duplicates_with_defaults(&dupes_config, cache_dir)
            .report;
        let (trace, not_found) = match &options.target {
            TraceCloneTarget::Location { file, line } => (
                fallow_engine::trace::trace_clone(&report, session.root(), file, *line),
                format!("no clone found at {file}:{line}"),
            ),
            TraceCloneTarget::Fingerprint(fingerprint) => (
                fallow_engine::trace::trace_clone_by_fingerprint(
                    &report,
                    session.root(),
                    fingerprint,
                ),
                format!("no clone group with fingerprint {fingerprint}"),
            ),
        };
        if trace.matched_instance.is_none() {
            return Err(ProgrammaticError::new(not_found, 2)
                .with_code("FALLOW_TRACE_TARGET_NOT_FOUND")
                .with_help(
                    "No clone matched. Run find_dupes to list clone groups and their fingerprints; \
                     a location must fall inside a reported clone instance, and a fingerprint must \
                     be a find_dupes clone_groups[].fingerprint (a dup:<id> value).",
                )
                .with_context("trace_clone"));
        }
        Ok(TraceCloneProgrammaticOutput { output: trace })
    })
}

/// Exercise the retained-graph trace family and compact JSON boundary without
/// repeating project discovery, parsing, or graph construction.
///
/// # Errors
///
/// Returns a structured error when a fixture target is missing or compact JSON
/// serialization fails.
#[doc(hidden)]
#[allow(
    clippy::implicit_hasher,
    reason = "the engine trace boundary intentionally accepts the workspace-standard FxHashSet"
)]
pub fn benchmark_trace_graph_family_compact_json(
    graph: &fallow_engine::module_graph::RetainedModuleGraph,
    root: &std::path::Path,
    script_used_packages: &FxHashSet<String>,
) -> ProgrammaticResult<(usize, usize, usize, usize, usize)> {
    let export =
        fallow_engine::trace::trace_export(graph, root, "src/000-shared.ts", "sharedValue")
            .ok_or_else(|| benchmark_trace_target_missing("src/000-shared.ts:sharedValue"))?;
    let export_reference_count = export.direct_references.len();
    let export_json =
        crate::serialize_trace_export_programmatic_json(TraceExportProgrammaticOutput {
            output: TraceExportTargetOutput::Export(export),
        })?;

    let file = fallow_engine::trace::trace_file(graph, root, "src/000-shared.ts")
        .ok_or_else(|| benchmark_trace_target_missing("src/000-shared.ts"))?;
    let file_export_count = file.exports.len();
    let file_imported_by_count = file.imported_by.len();
    let file_json = crate::serialize_trace_file_programmatic_json(TraceFileProgrammaticOutput {
        output: file,
    })?;

    let dependency =
        fallow_engine::trace::trace_dependency(graph, root, "trace-package", script_used_packages);
    let dependency_import_count = dependency.import_count;
    let dependency_json =
        crate::serialize_trace_dependency_programmatic_json(TraceDependencyProgrammaticOutput {
            output: dependency,
        })?;

    let rendered_bytes = compact_json_len(&[export_json, file_json, dependency_json])?;
    Ok((
        export_reference_count,
        file_export_count,
        file_imported_by_count,
        dependency_import_count,
        rendered_bytes,
    ))
}

/// Stable facts returned by the clone trace benchmark boundary.
#[doc(hidden)]
#[derive(Debug, PartialEq, Eq)]
pub struct TraceCloneBenchmarkResult {
    /// Location identity returned by the location trace.
    pub location_file: std::path::PathBuf,
    /// Location line returned by the location trace.
    pub location_line: usize,
    /// Group fingerprint returned by the location trace.
    pub location_fingerprint: String,
    /// Group fingerprint returned by the fingerprint trace.
    pub fingerprint_fingerprint: String,
    /// Groups returned by the location trace.
    pub location_group_count: usize,
    /// Groups returned by the fingerprint trace.
    pub fingerprint_group_count: usize,
    /// Instances returned by the location trace.
    pub location_instance_count: usize,
    /// Instances returned by the fingerprint trace.
    pub fingerprint_instance_count: usize,
    /// Total compact JSON bytes for both trace responses.
    pub rendered_bytes: usize,
}

/// Exercise both supported clone trace identities from one retained report.
///
/// # Errors
///
/// Returns a structured error when either identity misses or compact JSON
/// serialization fails.
#[doc(hidden)]
pub fn benchmark_trace_clone_compact_json(
    report: &DuplicationReport,
    root: &std::path::Path,
    file: &str,
    line: usize,
    fingerprint: &str,
) -> ProgrammaticResult<TraceCloneBenchmarkResult> {
    let location_trace = fallow_engine::trace::trace_clone(report, root, file, line);
    let matched_location = location_trace
        .matched_instance
        .as_ref()
        .ok_or_else(|| benchmark_trace_target_missing(&format!("{file}:{line}")))?;
    let location_file = matched_location.file.clone();
    let location_line = matched_location.start_line;
    let location_fingerprint = location_trace
        .clone_groups
        .first()
        .map(|group| group.fingerprint.clone())
        .ok_or_else(|| benchmark_trace_target_missing(&format!("{file}:{line}")))?;
    let location_group_count = location_trace.clone_groups.len();
    let location_instance_count = location_trace
        .clone_groups
        .iter()
        .map(|group| group.instances.len())
        .sum();
    let location_json =
        crate::serialize_trace_clone_programmatic_json(TraceCloneProgrammaticOutput {
            output: location_trace,
        })?;

    let fingerprint_trace =
        fallow_engine::trace::trace_clone_by_fingerprint(report, root, fingerprint);
    if fingerprint_trace.matched_instance.is_none() {
        return Err(benchmark_trace_target_missing(fingerprint));
    }
    let fingerprint_fingerprint = fingerprint_trace
        .clone_groups
        .first()
        .map(|group| group.fingerprint.clone())
        .ok_or_else(|| benchmark_trace_target_missing(fingerprint))?;
    let fingerprint_group_count = fingerprint_trace.clone_groups.len();
    let fingerprint_instance_count = fingerprint_trace
        .clone_groups
        .iter()
        .map(|group| group.instances.len())
        .sum();
    let fingerprint_json =
        crate::serialize_trace_clone_programmatic_json(TraceCloneProgrammaticOutput {
            output: fingerprint_trace,
        })?;

    let rendered_bytes = compact_json_len(&[location_json, fingerprint_json])?;
    Ok(TraceCloneBenchmarkResult {
        location_file,
        location_line,
        location_fingerprint,
        fingerprint_fingerprint,
        location_group_count,
        fingerprint_group_count,
        location_instance_count,
        fingerprint_instance_count,
        rendered_bytes,
    })
}

fn compact_json_len(values: &[serde_json::Value]) -> ProgrammaticResult<usize> {
    serde_json::to_vec(values)
        .map(|json| json.len())
        .map_err(|err| {
            ProgrammaticError::new(
                format!("failed to serialize benchmark trace JSON: {err}"),
                2,
            )
            .with_code("FALLOW_SERIALIZE_BENCHMARK_TRACE")
            .with_context("benchmark_trace")
        })
}

fn benchmark_trace_target_missing(target: &str) -> ProgrammaticError {
    ProgrammaticError::new(format!("benchmark trace target not found: {target}"), 2)
        .with_code("FALLOW_BENCHMARK_TRACE_TARGET_NOT_FOUND")
        .with_context("benchmark_trace")
}

fn validate_non_empty(field: &str, value: &str) -> ProgrammaticResult<()> {
    if value.trim().is_empty() {
        return Err(
            ProgrammaticError::new(format!("{field} must not be empty"), 2)
                .with_code("FALLOW_INVALID_TRACE_OPTIONS")
                .with_context(field.to_string()),
        );
    }
    Ok(())
}

fn validate_trace_clone_target(target: &TraceCloneTarget) -> ProgrammaticResult<()> {
    match target {
        TraceCloneTarget::Location { file, line } => {
            validate_non_empty("file", file)?;
            if *line == 0 {
                return Err(ProgrammaticError::new("line must be greater than 0", 2)
                    .with_code("FALLOW_INVALID_TRACE_OPTIONS")
                    .with_context("trace_clone.line"));
            }
        }
        TraceCloneTarget::Fingerprint(fingerprint) => {
            validate_non_empty("fingerprint", fingerprint)?;
        }
    }
    Ok(())
}

fn load_trace_session(
    resolved: &ProgrammaticAnalysisContext,
) -> ProgrammaticResult<AnalysisSession> {
    super::dead_code::load_dead_code_session(
        &super::dead_code::default_dead_code_options_for_context(resolved),
        resolved,
    )
}

fn trace_artifacts(session: &AnalysisSession) -> ProgrammaticResult<TraceArtifacts> {
    let artifacts = session
        .analyze_dead_code_with_session_artifacts(false, true, None)
        .map_err(|err| {
            ProgrammaticError::new(format!("trace analysis failed: {err}"), 2)
                .with_code("FALLOW_TRACE_FAILED")
                .with_context("trace")
        })?;
    let graph = artifacts.analysis.graph.ok_or_else(|| {
        ProgrammaticError::new("trace requires a retained module graph", 2)
            .with_code("FALLOW_TRACE_GRAPH_UNAVAILABLE")
            .with_context("trace.graph")
    })?;
    Ok(TraceArtifacts {
        graph,
        script_used_packages: artifacts.analysis.script_used_packages,
    })
}

#[cfg(test)]
mod benchmark_tests {
    use std::fmt::Write as _;
    use std::fs;

    use fallow_engine::duplicates::CloneFingerprintSet;

    use super::*;

    const FIXTURE_SIZE: usize = 4;

    fn write_file(root: &std::path::Path, path: &str, source: impl AsRef<str>) {
        let path = root.join(path);
        fs::create_dir_all(path.parent().expect("fixture file has parent"))
            .expect("fixture directory is created");
        fs::write(path, source.as_ref()).expect("fixture file is written");
    }

    #[test]
    fn graph_family_benchmark_boundary_uses_only_retained_artifacts() {
        let temp_dir = tempfile::TempDir::new().expect("temporary project is created");
        let root = temp_dir.path().to_path_buf();
        write_file(
            &root,
            "package.json",
            r#"{"name":"trace-test","type":"module","main":"src/index.ts"}"#,
        );
        write_file(
            &root,
            "src/000-shared.ts",
            "export const sharedValue = 42;\n",
        );

        let mut index_source = String::new();
        for index in 0..FIXTURE_SIZE {
            write_file(
                &root,
                &format!("src/consumer{index}.ts"),
                format!(
                    "import {{ sharedValue }} from './000-shared';\nimport {{ traceHelper }} from 'trace-package';\nexport const value{index} = traceHelper(sharedValue + {index});\n"
                ),
            );
            writeln!(
                index_source,
                "import {{ value{index} }} from './consumer{index}';\nconsole.log(value{index});"
            )
            .expect("index source is built");
        }
        write_file(&root, "src/index.ts", index_source);

        let session = AnalysisSession::load(&root, None).expect("trace session loads");
        let target = session
            .files()
            .iter()
            .find(|file| file.path.ends_with("src/000-shared.ts"))
            .expect("trace target is discovered");
        assert_eq!(
            target.id.0, 0,
            "the retained trace target must precede every non-matching importer"
        );
        let trace_root = session.root().to_path_buf();
        let artifacts = session
            .analyze_dead_code_with_artifacts(false, true)
            .expect("trace graph analysis succeeds");
        drop(session);
        temp_dir.close().expect("temporary project is removed");

        let result = benchmark_trace_graph_family_compact_json(
            artifacts.graph.as_ref().expect("trace graph is retained"),
            &trace_root,
            &artifacts.script_used_packages,
        )
        .expect("retained trace graph serializes without project IO");
        assert_eq!(result.0, FIXTURE_SIZE);
        assert_eq!(result.1, 1);
        assert_eq!(result.2, FIXTURE_SIZE);
        assert_eq!(result.3, FIXTURE_SIZE);
        assert!(result.4 > 0);
    }

    #[test]
    fn clone_benchmark_boundary_uses_only_the_retained_report() {
        let temp_dir = tempfile::TempDir::new().expect("temporary project is created");
        let root = temp_dir.path().to_path_buf();
        write_file(
            &root,
            "package.json",
            r#"{"name":"trace-clone-test","type":"module"}"#,
        );
        for index in 0..FIXTURE_SIZE {
            write_file(
                &root,
                &format!("src/clone{index}.ts"),
                format!(
                    "export function normalizeRecords(records: Array<{{ active: boolean; value: number }}>) {{\n  const active = records.filter((record) => record.active);\n  const values = active.map((record) => record.value);\n  const total = values.reduce((sum, value) => sum + value, 0);\n  const average = values.length === 0 ? 0 : total / values.length;\n  const maximum = values.reduce((current, value) => Math.max(current, value), 0);\n  return {{ total, average, maximum, count: values.length }};\n}}\n\nexport const cloneId = {index};\n"
                ),
            );
        }

        let session = AnalysisSession::load(&root, None).expect("clone session loads");
        let trace_root = session.root().to_path_buf();
        let mut config = session.config().duplicates.clone();
        config.min_tokens = 35;
        config.min_lines = 5;
        config.min_occurrences = FIXTURE_SIZE;
        let report = session.find_duplicates_with_defaults(&config, None).report;
        let group = report
            .clone_groups
            .iter()
            .max_by_key(|group| group.instances.len())
            .expect("clone group exists");
        let target = group.instances.last().expect("clone instance exists");
        let target_file = target
            .file
            .strip_prefix(&trace_root)
            .expect("clone path is project-relative")
            .to_string_lossy()
            .replace('\\', "/");
        let target_line = target.start_line;
        let expected_fingerprint =
            CloneFingerprintSet::from_groups(&report.clone_groups).fingerprint_for_group(group);
        drop(session);
        temp_dir.close().expect("temporary project is removed");

        let result = benchmark_trace_clone_compact_json(
            &report,
            &trace_root,
            &target_file,
            target_line,
            &expected_fingerprint,
        )
        .expect("retained clone report serializes without project IO");
        assert_eq!(result.location_file, std::path::PathBuf::from(&target_file));
        assert_eq!(result.location_line, target_line);
        assert_eq!(result.location_fingerprint, expected_fingerprint);
        assert_eq!(result.fingerprint_fingerprint, expected_fingerprint);
        assert_eq!(result.location_group_count, 1);
        assert_eq!(result.fingerprint_group_count, 1);
        assert_eq!(result.location_instance_count, FIXTURE_SIZE);
        assert_eq!(result.fingerprint_instance_count, FIXTURE_SIZE);
        assert!(result.rendered_bytes > 0);
    }
}
