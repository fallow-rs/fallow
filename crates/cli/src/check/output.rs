use std::io::{BufWriter, Write};
use std::process::ExitCode;

use fallow_config::{OutputFormat, ResolvedConfig};
use fallow_engine::module_graph::RetainedModuleGraph;
use fallow_types::discover::DiscoveredFile;
use rustc_hash::FxHashSet;

use super::TraceOptions;
use crate::{error::emit_error, report};

/// Handle `--trace`, `--trace-file`, and `--trace-dependency` early returns.
pub(super) fn handle_trace_output(
    graph: &RetainedModuleGraph,
    trace_opts: &TraceOptions,
    root: &std::path::Path,
    output: OutputFormat,
    script_used_packages: &FxHashSet<String>,
) -> Option<ExitCode> {
    handle_trace_export(graph, trace_opts, root, output)
        .or_else(|| handle_trace_file(graph, trace_opts, root, output))
        .or_else(|| handle_trace_dependency(graph, trace_opts, root, output, script_used_packages))
        .or_else(|| handle_impact_closure_trace(graph, trace_opts, root, output))
}

fn handle_trace_export(
    graph: &RetainedModuleGraph,
    trace_opts: &TraceOptions,
    root: &std::path::Path,
    output: OutputFormat,
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
        report::print_export_trace(&trace, output);
        return Some(ExitCode::SUCCESS);
    }
    // #1744: the name is not a top-level export. It may be a class / enum / store
    // MEMBER declared on one; fall back to a member trace so a class-member
    // finding can be debugged instead of erroring "export not found".
    if let Some(trace) =
        fallow_engine::trace::trace_class_member(graph, root, file_path, export_name)
    {
        report::print_class_member_trace(&trace, output);
        return Some(ExitCode::SUCCESS);
    }
    Some(emit_error(
        &format!("export or member '{export_name}' not found in '{file_path}'"),
        2,
        output,
    ))
}

fn handle_trace_file(
    graph: &RetainedModuleGraph,
    trace_opts: &TraceOptions,
    root: &std::path::Path,
    output: OutputFormat,
) -> Option<ExitCode> {
    let file_path = trace_opts.trace_file.as_ref()?;
    match fallow_engine::trace::trace_file(graph, root, file_path) {
        Some(trace) => {
            report::print_file_trace(&trace, output);
            Some(ExitCode::SUCCESS)
        }
        None => Some(emit_error(
            &format!("file '{file_path}' not found in module graph"),
            2,
            output,
        )),
    }
}

fn handle_trace_dependency(
    graph: &RetainedModuleGraph,
    trace_opts: &TraceOptions,
    root: &std::path::Path,
    output: OutputFormat,
    script_used_packages: &FxHashSet<String>,
) -> Option<ExitCode> {
    let pkg_name = trace_opts.trace_dependency.as_ref()?;
    let trace = fallow_engine::trace::trace_dependency(graph, root, pkg_name, script_used_packages);
    report::print_dependency_trace(&trace, output);
    Some(ExitCode::SUCCESS)
}

fn handle_impact_closure_trace(
    graph: &RetainedModuleGraph,
    trace_opts: &TraceOptions,
    root: &std::path::Path,
    output: OutputFormat,
) -> Option<ExitCode> {
    let file_path = trace_opts.impact_closure.as_ref()?;
    match fallow_engine::trace::trace_impact_closure(graph, root, file_path) {
        Some(trace) => {
            report::print_impact_closure_trace(&trace, output);
            Some(ExitCode::SUCCESS)
        }
        None => Some(emit_error(
            &format!("file '{file_path}' not found in module graph"),
            2,
            output,
        )),
    }
}

/// Write SARIF output to a file if `--sarif-file` was specified.
pub fn write_sarif_file(
    results: &fallow_types::results::AnalysisResults,
    config: &ResolvedConfig,
    sarif_path: &std::path::Path,
    quiet: bool,
) {
    let sarif = report::api_sarif_document(results, &config.root, &config.rules);
    if let Some(parent) = sarif_path.parent()
        && !parent.as_os_str().is_empty()
        && let Err(e) = std::fs::create_dir_all(parent)
    {
        eprintln!(
            "Warning: failed to create directory for SARIF file '{}': {e}",
            sarif_path.display()
        );
    }
    let file = match std::fs::File::create(sarif_path) {
        Ok(file) => file,
        Err(e) => {
            eprintln!(
                "Warning: failed to write SARIF file '{}': {e}",
                sarif_path.display()
            );
            return;
        }
    };
    let mut writer = BufWriter::new(file);
    if let Err(e) = serde_json::to_writer_pretty(&mut writer, &sarif) {
        eprintln!("Warning: failed to serialize SARIF output: {e}");
    } else if let Err(e) = writer.flush() {
        eprintln!(
            "Warning: failed to write SARIF file '{}': {e}",
            sarif_path.display()
        );
    } else if !quiet {
        eprintln!("SARIF output written to {}", sarif_path.display());
    }
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

/// Parse a `--trace` spec into `(file_path, export_name)`.
///
/// The format is `FILE:EXPORT_NAME`. Uses `rsplit_once` so that colons
/// in Windows drive letters (e.g., `C:\src\utils.ts:foo`) are handled
/// correctly, only the last colon is used as the separator.
pub(super) fn parse_trace_spec(spec: &str) -> Option<(&str, &str)> {
    spec.rsplit_once(':')
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

    #[test]
    fn parse_trace_spec_colon_only() {
        let result = parse_trace_spec(":");
        assert_eq!(result, Some(("", "")));
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

    #[test]
    fn handle_trace_output_returns_none_when_no_trace_active() {
        let trace_opts = TraceOptions {
            trace_export: None,
            trace_file: None,
            trace_dependency: None,
            impact_closure: None,
            performance: false,
        };
        assert!(!trace_opts.any_active());
    }

    fn make_resolved_config() -> fallow_config::ResolvedConfig {
        fallow_config::ResolvedConfig {
            root: std::path::PathBuf::from("/project"),
            entry_patterns: vec![],
            ignore_patterns: globset::GlobSet::empty(),
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
            health: fallow_config::HealthConfig::default(),
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
            cache_max_size_mb: None,
            cache_config_hash: 0,
            max_file_size_bytes: None,
        }
    }

    #[test]
    fn write_sarif_file_creates_output() {
        let results = fallow_types::results::AnalysisResults::default();
        let config = make_resolved_config();

        let dir = tempfile::tempdir().expect("create temp dir");
        let sarif_path = dir.path().join("output.sarif");

        write_sarif_file(&results, &config, &sarif_path, true);

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

        write_sarif_file(&results, &config, &sarif_path, true);

        assert!(sarif_path.exists());
    }
}
