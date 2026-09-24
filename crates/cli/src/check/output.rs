use std::io::{BufWriter, Write};
use std::process::ExitCode;

use fallow_config::{OutputFormat, ResolvedConfig};
use fallow_engine::module_graph::RetainedModuleGraph;
use fallow_types::discover::DiscoveredFile;
use rustc_hash::FxHashSet;

use super::TraceOptions;
use crate::{error::emit_error, report};

/// What the plugin run knows beyond the graph, which a file or dependency trace
/// adds to its output.
pub(super) struct TraceFacts<'a> {
    /// Packages invoked from package.json scripts and CI configs.
    pub script_used_packages: &'a FxHashSet<String>,
    /// Which configs name which files and dependency names.
    pub provenance: &'a fallow_engine::trace::TraceProvenance,
}

/// Handle `--trace`, `--trace-file`, and `--trace-dependency` early returns.
pub(super) fn handle_trace_output(
    graph: &RetainedModuleGraph,
    trace_opts: &TraceOptions,
    root: &std::path::Path,
    output: OutputFormat,
    json_style: crate::json_style::JsonStyle,
    facts: &TraceFacts<'_>,
) -> Option<ExitCode> {
    let request = TraceRequest {
        graph,
        trace_opts,
        root,
        output,
        json_style,
    };
    handle_trace_export(graph, trace_opts, root, output, json_style)
        .or_else(|| handle_trace_file(&request, facts.provenance))
        .or_else(|| handle_trace_dependency(&request, facts))
        .or_else(|| handle_impact_closure_trace(graph, trace_opts, root, output, json_style))
}

/// Handle semantic trace and exact-symbol impact before the syntactic trace
/// fallback consumes the focused command.
#[expect(
    clippy::too_many_lines,
    reason = "the focused trace renderer preserves one shared output contract across formats"
)]
pub(super) fn handle_type_aware_trace_output(
    graph: &RetainedModuleGraph,
    trace_opts: &TraceOptions,
    config: &ResolvedConfig,
    explain: bool,
    json_style: crate::json_style::JsonStyle,
) -> Option<ExitCode> {
    if !config.type_aware.enabled {
        return None;
    }
    let projects = config
        .type_aware
        .projects
        .iter()
        .map(std::path::PathBuf::from)
        .collect::<Vec<_>>();
    if let Some(trace_spec) = trace_opts.trace_export.as_ref() {
        let Some((file_path, export_name)) = parse_trace_spec(trace_spec) else {
            return Some(emit_error(
                "--trace requires FILE:EXPORT_NAME format (e.g., src/utils.ts:foo)",
                2,
                config.output,
            ));
        };
        if let Some(mut trace) =
            fallow_engine::trace::trace_export(graph, &config.root, file_path, export_name)
        {
            let Some(symbol) = fallow_engine::trace::semantic_symbol_for_export(
                graph,
                &config.root,
                file_path,
                export_name,
            ) else {
                return Some(emit_error(
                    &format!("could not resolve semantic identity for '{trace_spec}'"),
                    2,
                    config.output,
                ));
            };
            match fallow_api::trace_type_aware_symbol(&config.root, &projects, symbol) {
                Ok(mut semantic) => {
                    fallow_engine::trace::reconcile_semantic_trace_reachability(
                        graph,
                        &config.root,
                        trace.file_reachable,
                        &mut semantic,
                    );
                    let exit =
                        semantic_completeness_exit(config.type_aware.require, semantic.status);
                    trace.semantic = Some(semantic);
                    report::print_semantic_export_trace(&trace, config.output, explain, json_style);
                    return Some(exit);
                }
                Err(error) => {
                    return Some(emit_error(
                        &format!("Type-aware trace failed: {error}"),
                        2,
                        config.output,
                    ));
                }
            }
        }
        if let Some(mut trace) =
            fallow_engine::trace::trace_class_member(graph, &config.root, file_path, export_name)
        {
            let Some(symbol) = fallow_engine::trace::semantic_symbol_for_class_member(
                graph,
                &config.root,
                file_path,
                export_name,
            ) else {
                return Some(emit_error(
                    &format!("could not resolve semantic identity for '{trace_spec}'"),
                    2,
                    config.output,
                ));
            };
            match fallow_api::trace_type_aware_symbol(&config.root, &projects, symbol) {
                Ok(mut semantic) => {
                    fallow_engine::trace::reconcile_semantic_trace_reachability(
                        graph,
                        &config.root,
                        trace.owner_file_reachable,
                        &mut semantic,
                    );
                    let exit =
                        semantic_completeness_exit(config.type_aware.require, semantic.status);
                    trace.semantic = Some(semantic);
                    report::print_semantic_class_member_trace(
                        &trace,
                        config.output,
                        explain,
                        json_style,
                    );
                    return Some(exit);
                }
                Err(error) => {
                    return Some(emit_error(
                        &format!("Type-aware trace failed: {error}"),
                        2,
                        config.output,
                    ));
                }
            }
        }
        return Some(emit_error(
            &format!("export or member '{export_name}' not found in '{file_path}'"),
            2,
            config.output,
        ));
    }
    let impact_spec = trace_opts.symbol_impact.as_ref()?;
    let Some((file_path, target_name)) = parse_trace_spec(impact_spec) else {
        return Some(emit_error(
            "--symbol-impact requires FILE:EXPORT_NAME or FILE:CLASS.METHOD format",
            2,
            config.output,
        ));
    };
    let symbol = if let Some(symbol) = fallow_engine::trace::semantic_symbol_for_export(
        graph,
        &config.root,
        file_path,
        target_name,
    ) {
        symbol
    } else if let Some((owner_name, member_name)) = parse_class_method_target(target_name) {
        match fallow_engine::trace::semantic_symbol_for_exact_class_method(
            graph,
            &config.root,
            file_path,
            owner_name,
            member_name,
        ) {
            Ok(symbol) => symbol,
            Err(reason) => {
                return Some(emit_error(
                    &format!(
                        "class method '{target_name}' is unavailable for exact impact analysis: {reason}"
                    ),
                    2,
                    config.output,
                ));
            }
        }
    } else {
        return Some(emit_error(
            &format!("export or class method '{target_name}' not found in '{file_path}'"),
            2,
            config.output,
        ));
    };
    match fallow_api::type_aware_symbol_impact(&config.root, &projects, symbol) {
        Ok(impact) => {
            let exit = semantic_completeness_exit(config.type_aware.require, impact.status);
            report::print_symbol_impact(&impact, config.output, explain, json_style);
            Some(exit)
        }
        Err(error) => Some(emit_error(
            &format!("Type-aware symbol impact failed: {error}"),
            2,
            config.output,
        )),
    }
}

fn semantic_completeness_exit(
    require: fallow_config::TypeAwareRequire,
    status: fallow_types::semantic::SemanticCompleteness,
) -> ExitCode {
    if require == fallow_config::TypeAwareRequire::Complete
        && status != fallow_types::semantic::SemanticCompleteness::Complete
    {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}

fn handle_trace_export(
    graph: &RetainedModuleGraph,
    trace_opts: &TraceOptions,
    root: &std::path::Path,
    output: OutputFormat,
    json_style: crate::json_style::JsonStyle,
) -> Option<ExitCode> {
    let trace_spec = trace_opts.trace_export.as_ref()?;
    let Some((file_path, export_name)) = parse_trace_spec(trace_spec) else {
        return Some(emit_error(
            "--trace requires FILE:EXPORT_NAME format (e.g., src/utils.ts:foo)",
            2,
            output,
        ));
    };
    if let Some(trace) = fallow_engine::trace::trace_export(graph, root, file_path, export_name) {
        report::print_export_trace(&trace, output, json_style);
        return Some(ExitCode::SUCCESS);
    }
    // #1744: the name is not a top-level export. It may be a class / enum / store
    // MEMBER declared on one; fall back to a member trace so a class-member
    // finding can be debugged instead of erroring "export not found".
    if let Some(trace) =
        fallow_engine::trace::trace_class_member(graph, root, file_path, export_name)
    {
        report::print_class_member_trace(&trace, output, json_style);
        return Some(ExitCode::SUCCESS);
    }
    Some(emit_error(
        &format!("export or member '{export_name}' not found in '{file_path}'"),
        2,
        output,
    ))
}

/// The inputs every focused trace handler shares.
struct TraceRequest<'a> {
    graph: &'a RetainedModuleGraph,
    trace_opts: &'a TraceOptions,
    root: &'a std::path::Path,
    output: OutputFormat,
    json_style: crate::json_style::JsonStyle,
}

fn handle_trace_file(
    request: &TraceRequest<'_>,
    trace_provenance: &fallow_engine::trace::TraceProvenance,
) -> Option<ExitCode> {
    let file_path = request.trace_opts.trace_file.as_ref()?;
    match fallow_engine::trace::trace_file(request.graph, request.root, file_path) {
        Some(mut trace) => {
            trace.sources = trace_provenance.file_sources(&trace.file);
            report::print_file_trace(&trace, request.output, request.json_style);
            Some(ExitCode::SUCCESS)
        }
        None => Some(emit_error(
            &format!("file '{file_path}' not found in module graph"),
            2,
            request.output,
        )),
    }
}

fn handle_trace_dependency(request: &TraceRequest<'_>, facts: &TraceFacts<'_>) -> Option<ExitCode> {
    let pkg_name = request.trace_opts.trace_dependency.as_ref()?;
    let mut trace = fallow_engine::trace::trace_dependency(
        request.graph,
        request.root,
        pkg_name,
        facts.script_used_packages,
    );
    trace.sources = facts.provenance.dependency_sources(pkg_name);
    report::print_dependency_trace(&trace, request.output, request.json_style);
    Some(ExitCode::SUCCESS)
}

fn handle_impact_closure_trace(
    graph: &RetainedModuleGraph,
    trace_opts: &TraceOptions,
    root: &std::path::Path,
    output: OutputFormat,
    json_style: crate::json_style::JsonStyle,
) -> Option<ExitCode> {
    let file_path = trace_opts.impact_closure.as_ref()?;
    match fallow_engine::trace::trace_impact_closure(graph, root, file_path) {
        Some(trace) => {
            report::print_impact_closure_trace(&trace, output, json_style);
            Some(ExitCode::SUCCESS)
        }
        None => Some(emit_error(
            &format!("file '{file_path}' not found in module graph"),
            2,
            output,
        )),
    }
}

/// Write SARIF output to a file if `--sarif-file` was specified, and record
/// what became of that request either way.
///
/// The secondary artefact's fate reaches no other surface: the document on
/// stdout is complete whether or not the file was written, and the exit code
/// stays the one the findings produced. A consumer that uploads the file to
/// code scanning would otherwise see a green step and no alerts (issue #2690).
///
/// The asymmetry is deliberate and preserved: a failure is printed whether or
/// not `--quiet` was passed, the success line only without it.
pub fn write_sarif_file(
    results: &fallow_types::results::AnalysisResults,
    config: &ResolvedConfig,
    sarif_path: &std::path::Path,
    quiet: bool,
    type_aware: Option<&fallow_types::envelope::TypeAwareMeta>,
) {
    match write_sarif_document(results, config, sarif_path, type_aware) {
        Ok(()) => {
            if !quiet {
                eprintln!("SARIF output written to {}", sarif_path.display());
            }
            crate::requests::record_sarif_file_applied(sarif_path);
        }
        Err(failure) => {
            let message = failure.message(sarif_path);
            eprintln!("Warning: {message}");
            crate::requests::record_sarif_file_failure(sarif_path, failure.reason(), message);
        }
    }
}

/// Why a `--sarif-file` write did not happen, with the prose each case needs.
///
/// One source for the stderr line and the envelope message, so a log a human
/// read and a report a script read cannot state different remedies.
enum SarifWriteFailure {
    /// The target file could not be created, with the directory-creation error
    /// when that is what stopped it.
    Create {
        error: String,
        directory_failed: bool,
    },
    /// The document could not be serialized into the created file.
    Serialize { error: String },
    /// The created file could not be flushed to disk.
    Flush { error: String },
}

impl SarifWriteFailure {
    fn reason(&self) -> &'static str {
        match self {
            Self::Create {
                directory_failed: true,
                ..
            } => "directory-create-failed",
            Self::Create { .. } | Self::Flush { .. } => "write-failed",
            Self::Serialize { .. } => "serialize-failed",
        }
    }

    fn message(&self, sarif_path: &std::path::Path) -> String {
        let path = sarif_path.display();
        match self {
            Self::Create {
                error,
                directory_failed: true,
            } => format!(
                "failed to create the directory for SARIF file '{path}' ({error}), so nothing \
                 was written there. Anything reading that path, including code scanning, \
                 receives no findings from this run. Create the directory first, or point \
                 --sarif-file at a writable location."
            ),
            Self::Create {
                error,
                directory_failed: false,
            } => format!(
                "failed to write SARIF file '{path}': {error}. Anything reading that path, \
                 including code scanning, receives no findings from this run. Check the \
                 directory exists and is writable."
            ),
            Self::Serialize { error } => format!(
                "failed to serialize SARIF output for '{path}': {error}. The file is incomplete \
                 or absent, so anything reading that path receives no findings from this run. \
                 Rerun, and report this if it persists."
            ),
            Self::Flush { error } => format!(
                "failed to write SARIF file '{path}': {error}. The file is incomplete, so \
                 anything reading that path, including code scanning, receives no findings from \
                 this run. Check the available disk space and the directory permissions."
            ),
        }
    }
}

fn write_sarif_document(
    results: &fallow_types::results::AnalysisResults,
    config: &ResolvedConfig,
    sarif_path: &std::path::Path,
    type_aware: Option<&fallow_types::envelope::TypeAwareMeta>,
) -> Result<(), SarifWriteFailure> {
    let mut sarif = report::api_sarif_document(results, &config.root, &config.rules);
    crate::report::sarif::annotate_type_aware_sarif(&mut sarif, type_aware);
    let file = fallow_engine::write_guard::create_file(
        sarif_path,
        fallow_engine::write_guard::WriteTarget::Path,
    )
    .map_err(|e| SarifWriteFailure::Create {
        directory_failed: e.is_directory(),
        error: e.to_string(),
    })?;
    let mut writer = BufWriter::new(file);
    serde_json::to_writer_pretty(&mut writer, &sarif).map_err(|e| {
        SarifWriteFailure::Serialize {
            error: e.to_string(),
        }
    })?;
    writer.flush().map_err(|e| SarifWriteFailure::Flush {
        error: e.to_string(),
    })?;
    Ok(())
}

/// Run duplication cross-reference and print combined findings.
pub fn run_cross_reference(
    config: &ResolvedConfig,
    unfiltered_results: &fallow_types::results::AnalysisResults,
    files: &[DiscoveredFile],
    quiet: bool,
) {
    let dupe_report =
        fallow_engine::duplicates::find_duplicates(&config.root, files, &config.duplicates);
    let cross_ref =
        fallow_engine::cross_reference::cross_reference(&dupe_report, unfiltered_results);

    if cross_ref.has_findings() {
        report::print_cross_reference_findings(&cross_ref, &config.root, quiet, config.output);
    }
}

/// Parse a `--trace` or `--symbol-impact` spec into `(file_path, export_name)`,
/// on the shared selector contract in [`crate::selector`].
fn parse_trace_spec(spec: &str) -> Option<(&str, &str)> {
    crate::selector::parse_file_symbol_selector(spec)
}

fn parse_class_method_target(target: &str) -> Option<(&str, &str)> {
    let (owner, member) = target.split_once('.')?;
    if owner.is_empty() || member.is_empty() || member.contains('.') {
        return None;
    }
    Some((owner, member))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_trace_spec_simple() {
        let result = parse_trace_spec("src/utils.ts:foo");
        assert_eq!(result, Some(("src/utils.ts", "foo")));
    }

    #[test]
    fn parse_trace_spec_default_export() {
        let result = parse_trace_spec("src/component.tsx:default");
        assert_eq!(result, Some(("src/component.tsx", "default")));
    }

    #[test]
    fn parse_trace_spec_no_colon() {
        let result = parse_trace_spec("src/utils.ts");
        assert_eq!(result, None);
    }

    #[test]
    fn parse_trace_spec_empty_string() {
        let result = parse_trace_spec("");
        assert_eq!(result, None);
    }

    /// A bare colon used to parse into two empty halves and fail later with
    /// "export or member '' not found in ''". It is now rejected up front by
    /// the shared selector, with the same exit code and a diagnosis that names
    /// the expected format.
    #[test]
    fn parse_trace_spec_colon_only() {
        let result = parse_trace_spec(":");
        assert_eq!(result, None);
    }

    #[test]
    fn parse_trace_spec_multiple_colons_uses_last() {
        let result = parse_trace_spec("C:\\src\\utils.ts:foo");
        assert_eq!(result, Some(("C:\\src\\utils.ts", "foo")));
    }

    #[test]
    fn parse_trace_spec_nested_path_with_colons() {
        let result = parse_trace_spec("packages/core:src/index.ts:myExport");
        assert_eq!(result, Some(("packages/core:src/index.ts", "myExport")));
    }

    /// Whitespace-only halves are rejected like empty ones, and surviving halves
    /// are returned verbatim: the trim is a guard, never a normalisation.
    #[test]
    fn parse_trace_spec_rejects_whitespace_only_halves() {
        assert_eq!(parse_trace_spec(" : "), None);
        assert_eq!(parse_trace_spec("src/utils.ts:\t"), None);
        assert_eq!(
            parse_trace_spec(" src/utils.ts : foo "),
            Some((" src/utils.ts ", " foo "))
        );
    }

    #[test]
    fn parse_class_method_target_requires_exact_owner_and_member() {
        assert_eq!(
            parse_class_method_target("UserRepository.save"),
            Some(("UserRepository", "save"))
        );
        assert_eq!(parse_class_method_target("save"), None);
        assert_eq!(parse_class_method_target("A.B.save"), None);
        assert_eq!(parse_class_method_target(".save"), None);
        assert_eq!(parse_class_method_target("Repository."), None);
    }

    #[test]
    fn handle_trace_output_returns_none_when_no_trace_active() {
        let trace_opts = TraceOptions {
            trace_export: None,
            trace_file: None,
            trace_dependency: None,
            impact_closure: None,
            symbol_impact: None,
            performance: false,
        };
        assert!(!trace_opts.any_active());
    }

    fn make_resolved_config() -> fallow_config::ResolvedConfig {
        fallow_config::ResolvedConfig {
            root: std::path::PathBuf::from("/project"),
            entry_patterns: vec![],
            ignore_patterns: globset::GlobSet::empty(),
            user_ignore_pattern_count: 0,
            ignore_findings: fallow_config::FindingIgnoreMatcher::default(),
            output: OutputFormat::Json,
            cache_dir: std::path::PathBuf::from("/tmp/cache"),
            threads: 1,
            no_cache: true,
            ignore_dependencies: vec![],
            ignore_unresolved_imports: vec![],
            ignore_export_rules: vec![],
            compiled_ignore_exports: vec![],
            compiled_ignore_catalog_references: vec![],
            compiled_ignore_dependency_overrides: vec![],
            ignore_exports_used_in_file: fallow_config::IgnoreExportsUsedInFileConfig::default(),
            used_class_members: vec![],
            ignore_decorators: vec![],
            unused_component_props_ignore: None,
            duplicates: fallow_config::DuplicatesConfig::default(),
            similar_code: fallow_config::SimilarCodeConfig::default(),
            health: fallow_config::HealthConfig::default(),
            type_aware: fallow_config::TypeAwareConfig::default(),
            rules: fallow_config::RulesConfig::default(),
            boundaries: fallow_config::ResolvedBoundaryConfig::default(),
            production: false,
            quiet: true,
            external_plugins: vec![],
            rule_packs: vec![],
            rule_pack_sources: vec![],
            dynamically_loaded: vec![],
            overrides: vec![],
            regression: None,
            audit: fallow_config::AuditConfig::default(),
            codeowners: None,
            public_packages: vec![],
            flags: fallow_config::FlagsConfig::default(),
            security: fallow_config::SecurityConfig::default(),
            fix: fallow_config::FixConfig::default(),
            resolve: fallow_config::ResolveConfig::default(),
            include_entry_exports: false,
            auto_imports: false,
            fail_on_parse_error: false,
            cache_max_size_mb: None,
            cache_config_hash: 0,
            max_file_size_bytes: None,
            analysis_snapshot: fallow_config::AnalysisSnapshot::Current,
        }
    }

    #[test]
    fn write_sarif_file_creates_output() {
        let results = fallow_types::results::AnalysisResults::default();
        let config = make_resolved_config();

        let dir = tempfile::tempdir().expect("create temp dir");
        let sarif_path = dir.path().join("output.sarif");

        write_sarif_file(&results, &config, &sarif_path, true, None);

        assert!(sarif_path.exists());
        let content = std::fs::read_to_string(&sarif_path).expect("read sarif");
        let parsed: serde_json::Value =
            serde_json::from_str(&content).expect("parse sarif as json");
        assert!(parsed.get("$schema").is_some() || parsed.get("version").is_some());
    }

    #[test]
    fn write_sarif_file_creates_parent_directories() {
        let results = fallow_types::results::AnalysisResults::default();
        let config = make_resolved_config();

        let dir = tempfile::tempdir().expect("create temp dir");
        let sarif_path = dir.path().join("nested").join("dir").join("output.sarif");

        write_sarif_file(&results, &config, &sarif_path, true, None);

        assert!(sarif_path.exists());
    }
}
