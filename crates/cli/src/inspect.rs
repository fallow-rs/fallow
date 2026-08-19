use std::path::{Component, Path, PathBuf};
use std::process::{Command, ExitCode};
use std::sync::atomic::{AtomicU8, AtomicUsize, Ordering};

use fallow_config::OutputFormat;
use fallow_types::envelope::Meta;
use serde_json::{Value, json};

use crate::error::emit_error;
use crate::report;
use crate::report::sink::outln;
use fallow_output::{
    InspectEvidence, InspectEvidenceScope, InspectEvidenceSection, InspectFileIdentity,
    InspectIdentity, InspectOutput, InspectSectionStatus, InspectSymbolIdentity,
    InspectTargetDescriptor,
};

#[derive(Clone)]
pub enum InspectTarget {
    File { file: String },
    Symbol { file: String, export_name: String },
}

pub struct InspectOptions<'a> {
    pub root: &'a Path,
    pub config_path: Option<&'a PathBuf>,
    pub output: OutputFormat,
    pub json_style: crate::json_style::JsonStyle,
    pub no_cache: bool,
    pub no_production: bool,
    pub max_file_size: Option<u32>,
    pub threads: usize,
    pub quiet: bool,
    pub production: bool,
    pub workspace: Option<&'a Vec<String>>,
    pub target: InspectTarget,
    /// OPT-IN cache location for in-process target churn evidence. `None`
    /// keeps git-history analysis entirely off the default path.
    pub churn_cache_dir: Option<&'a Path>,
    /// OPT-IN: also run the best-effort symbol-level call chain
    /// (`fallow trace`) and attach it as the `symbol_chain` evidence section.
    /// Only meaningful for a SYMBOL target. Default off (best-effort, off the
    /// ranked path).
    pub symbol_chain: bool,
    /// Include project-wide TypeScript semantic evidence for symbol targets.
    pub type_aware: Option<bool>,
    /// Explicit TypeScript project configs for semantic analysis.
    pub type_aware_projects: &'a [PathBuf],
    /// Whether incomplete semantic evidence is advisory or gating.
    pub type_aware_require: Option<fallow_config::TypeAwareRequire>,
}

#[derive(Debug)]
struct NormalizedTarget {
    file: String,
    export_name: Option<String>,
}

impl NormalizedTarget {
    fn new(root: &Path, target: &InspectTarget) -> Result<Self, String> {
        match target {
            InspectTarget::File { file } => {
                require_non_empty("file", file)?;
                let file = normalize_target_file(root, file)?;
                Ok(Self {
                    file,
                    export_name: None,
                })
            }
            InspectTarget::Symbol { file, export_name } => {
                require_non_empty("symbol file", file)?;
                require_non_empty("symbol export", export_name)?;
                let file = normalize_target_file(root, file)?;
                Ok(Self {
                    file,
                    export_name: Some(export_name.clone()),
                })
            }
        }
    }

    fn target_descriptor(&self) -> InspectTargetDescriptor {
        match self.export_name.as_deref() {
            Some(export_name) => InspectTargetDescriptor::Symbol {
                file: self.file.clone(),
                export_name: export_name.to_string(),
            },
            None => InspectTargetDescriptor::File {
                file: self.file.clone(),
            },
        }
    }
}

pub fn run_inspect(opts: &InspectOptions<'_>) -> ExitCode {
    if !matches!(opts.output, OutputFormat::Json | OutputFormat::Human) {
        return emit_error("inspect supports --format json or human", 2, opts.output);
    }
    let transport = ProcessChildJsonTransport;
    let bundle = match build_inspect_bundle(opts, &transport) {
        Ok(bundle) => bundle,
        Err(message) => return emit_error(&message, 2, opts.output),
    };

    let completeness_failed = inspect_type_aware_completeness_failed(&bundle);
    let emitted = emit_inspect_bundle(bundle, opts);
    if emitted == ExitCode::SUCCESS && completeness_failed {
        ExitCode::from(1)
    } else {
        emitted
    }
}

fn build_inspect_bundle(
    opts: &InspectOptions<'_>,
    transport: &dyn ChildJsonTransport,
) -> Result<InspectOutput, String> {
    let target = NormalizedTarget::new(opts.root, &opts.target)?;

    let target_file = target.file.as_str();
    let trace_file = run_required_json(opts, trace_file_args(target_file), transport)?;
    let trace_export = collect_trace_export(opts, &target, transport)?;

    let mut warnings = Vec::new();
    if target.export_name.is_some() {
        warnings.push(
            "dead_code, duplication, complexity, and security evidence is file-scoped in v1; file:line symbol narrowing is a follow-up"
                .to_string(),
        );
    }

    let (evidence, type_aware, semantic_warnings) =
        build_inspect_evidence(opts, &target, &trace_file, trace_export.clone(), transport);
    warnings.extend(semantic_warnings);
    push_inspect_warnings(&mut warnings, &evidence);

    let identity = build_inspect_identity(&target, &trace_file, trace_export.as_ref());

    Ok(InspectOutput {
        target: target.target_descriptor(),
        identity,
        evidence,
        warnings,
        meta: type_aware.map(|type_aware| Meta {
            type_aware: Some(type_aware),
            ..Meta::default()
        }),
    })
}

fn inspect_type_aware_completeness_failed(bundle: &InspectOutput) -> bool {
    let type_aware = bundle
        .meta
        .as_ref()
        .and_then(|meta| meta.type_aware.as_ref());
    inspect_requires_complete(type_aware)
        && (inspect_semantic_incomplete(&bundle.evidence)
            || type_aware.is_some_and(inspect_type_aware_meta_incomplete))
}

fn inspect_requires_complete(meta: Option<&fallow_types::envelope::TypeAwareMeta>) -> bool {
    meta.and_then(|meta| meta.required_completeness)
        == Some(fallow_types::semantic::SemanticCompletenessRequirement::Complete)
}

fn inspect_type_aware_meta_incomplete(meta: &fallow_types::envelope::TypeAwareMeta) -> bool {
    meta.identity.as_ref().is_some_and(|identity| {
        identity.completeness != fallow_types::semantic::SemanticCompleteness::Complete
    }) || meta
        .queries
        .iter()
        .any(|query| query.status != fallow_types::semantic::SemanticCompleteness::Complete)
}

fn inspect_semantic_incomplete(evidence: &InspectEvidence) -> bool {
    [
        evidence.semantic_trace.as_ref(),
        evidence.api_surface.as_ref(),
        evidence.symbol_impact.as_ref(),
    ]
    .into_iter()
    .flatten()
    .any(|section| {
        section.status != InspectSectionStatus::Ok
            || section
                .data
                .as_ref()
                .and_then(|value| value.get("status"))
                .and_then(Value::as_str)
                .is_some_and(|status| status != "complete")
    })
}

/// Run the `trace_export` child when the target is a symbol, else `Ok(None)`.
fn collect_trace_export(
    opts: &InspectOptions<'_>,
    target: &NormalizedTarget,
    transport: &dyn ChildJsonTransport,
) -> Result<Option<Value>, String> {
    let Some(export_name) = target.export_name.as_deref() else {
        return Ok(None);
    };
    run_required_json(
        opts,
        trace_export_args(&target.file, export_name),
        transport,
    )
    .map(Some)
}

/// Compose the evidence sections (trace, dead-code, duplication, complexity,
/// security, impact-closure, plus the OPT-IN symbol chain) for the inspect
/// bundle.
fn build_inspect_evidence(
    opts: &InspectOptions<'_>,
    target: &NormalizedTarget,
    trace_file: &Value,
    trace_export: Option<Value>,
    transport: &dyn ChildJsonTransport,
) -> (
    InspectEvidence,
    Option<fallow_types::envelope::TypeAwareMeta>,
    Vec<String>,
) {
    let optional_threads = parallel_child_threads(opts.threads);
    let child_evidence = collect_inspect_child_evidence(opts, target, optional_threads, transport);

    let semantic = collect_semantic_evidence(opts, target);
    let evidence = InspectEvidence {
        trace_file: InspectEvidenceSection::ok(InspectEvidenceScope::File, trace_file.clone()),
        trace_export: trace_export
            .map(|value| InspectEvidenceSection::ok(InspectEvidenceScope::Symbol, value)),
        dead_code: child_evidence.dead_code,
        duplication: child_evidence.duplication,
        complexity: child_evidence.complexity,
        security: child_evidence.security,
        impact_closure: child_evidence.impact_closure,
        churn: child_evidence.churn,
        symbol_chain: build_symbol_chain_section(opts, target, optional_threads, transport),
        semantic_trace: semantic.semantic_trace,
        api_surface: semantic.api_surface,
        symbol_impact: semantic.symbol_impact,
        targeted_tests: semantic.targeted_tests,
    };
    (evidence, semantic.type_aware, semantic.warnings)
}

struct InspectSemanticEvidence {
    semantic_trace: Option<InspectEvidenceSection>,
    api_surface: Option<InspectEvidenceSection>,
    symbol_impact: Option<InspectEvidenceSection>,
    targeted_tests: Option<InspectEvidenceSection>,
    type_aware: Option<fallow_types::envelope::TypeAwareMeta>,
    warnings: Vec<String>,
}

#[expect(
    clippy::too_many_lines,
    reason = "semantic inspect resolves all related evidence in one shared Program request"
)]
fn collect_semantic_evidence(
    opts: &InspectOptions<'_>,
    target: &NormalizedTarget,
) -> InspectSemanticEvidence {
    let Some(export_name) = target.export_name.as_deref() else {
        return InspectSemanticEvidence {
            semantic_trace: None,
            api_surface: None,
            symbol_impact: None,
            targeted_tests: None,
            type_aware: None,
            warnings: Vec::new(),
        };
    };
    let config_path = opts.config_path.cloned();
    let Ok(mut config) = crate::runtime_support::load_config_for_analysis(
        opts.root,
        &config_path,
        crate::runtime_support::ConfigLoadOptions {
            output: opts.output,
            no_cache: opts.no_cache,
            threads: opts.threads,
            production_override: if opts.no_production {
                Some(false)
            } else {
                opts.production.then_some(true)
            },
            quiet: opts.quiet,
            allow_remote_extends: false,
        },
        fallow_config::ProductionAnalysis::DeadCode,
    ) else {
        return semantic_error_sections(
            "could not load type-aware inspect config",
            opts.type_aware_require.map(Into::into),
        );
    };
    if crate::check::apply_type_aware_overrides_from(
        opts.output,
        opts.type_aware,
        None,
        opts.type_aware_projects,
        opts.type_aware_require,
        &mut config,
    )
    .is_err()
    {
        return semantic_error_sections(
            "invalid type-aware inspect options",
            Some(config.type_aware.require.into()),
        );
    }
    if !config.type_aware.enabled {
        return InspectSemanticEvidence {
            semantic_trace: None,
            api_surface: None,
            symbol_impact: None,
            targeted_tests: None,
            type_aware: None,
            warnings: Vec::new(),
        };
    }
    let session =
        match fallow_engine::session::AnalysisSession::from_resolved_config(config.clone()) {
            Ok(session) => session,
            Err(error) => {
                return semantic_error_sections(
                    &format!("analysis failed: {error}"),
                    Some(config.type_aware.require.into()),
                );
            }
        };
    let analysis = match session.analyze_dead_code_with_artifacts(false, true) {
        Ok(analysis) => analysis,
        Err(error) => {
            return semantic_error_sections(
                &format!("analysis failed: {error}"),
                Some(config.type_aware.require.into()),
            );
        }
    };
    let Some(graph) = analysis.graph.as_ref() else {
        return semantic_error_sections(
            "semantic inspect graph was unavailable",
            Some(config.type_aware.require.into()),
        );
    };
    let Some(symbol) = fallow_engine::trace::semantic_symbol_for_export(
        graph,
        &config.root,
        &target.file,
        export_name,
    ) else {
        return semantic_error_sections(
            "could not resolve the exact exported symbol",
            Some(config.type_aware.require.into()),
        );
    };
    let entry_points = fallow_engine::project_analysis::public_api_entry_paths_for_graph(
        graph,
        &config,
        session.workspaces(),
    );
    let projects = config
        .type_aware
        .projects
        .iter()
        .map(PathBuf::from)
        .collect::<Vec<_>>();
    let outcome =
        match fallow_api::inspect_type_aware_symbol(&config.root, &projects, symbol, &entry_points)
        {
            Ok(outcome) => outcome,
            Err(error) => {
                return semantic_error_sections(
                    &error.to_string(),
                    Some(config.type_aware.require.into()),
                );
            }
        };
    let trace = InspectEvidenceSection::semantic(
        InspectEvidenceScope::Symbol,
        outcome.trace.status,
        serde_json::to_value(&outcome.trace).unwrap_or(Value::Null),
    );
    let api_surface = InspectEvidenceSection::semantic(
        InspectEvidenceScope::Symbol,
        outcome.api_surface.status,
        serde_json::to_value(&outcome.api_surface).unwrap_or(Value::Null),
    );
    let impact_value = serde_json::to_value(&outcome.impact).unwrap_or(Value::Null);
    let impact = InspectEvidenceSection::semantic(
        InspectEvidenceScope::Symbol,
        outcome.impact.status,
        impact_value.clone(),
    );
    let targeted_tests = InspectEvidenceSection::semantic(
        InspectEvidenceScope::Symbol,
        outcome.impact.status,
        json!({
            "tests": impact_value.get("targeted_tests").cloned().unwrap_or_else(|| Value::Array(Vec::new())),
            "total_test_count": impact_value.get("total_targeted_test_count").cloned().unwrap_or_else(|| Value::from(0)),
            "confidence": impact_value.get("confidence").cloned(),
            "status": impact_value.get("status").cloned(),
            "identity": impact_value.get("identity").cloned(),
        }),
    );

    let mut type_aware_meta = outcome.type_aware.meta;
    type_aware_meta.required_completeness = Some(config.type_aware.require.into());
    InspectSemanticEvidence {
        semantic_trace: Some(trace),
        api_surface: Some(api_surface),
        symbol_impact: Some(impact),
        targeted_tests: Some(targeted_tests),
        type_aware: Some(type_aware_meta),
        warnings: outcome.type_aware.warnings,
    }
}

fn semantic_error_sections(
    message: &str,
    required_completeness: Option<fallow_types::semantic::SemanticCompletenessRequirement>,
) -> InspectSemanticEvidence {
    let section =
        || InspectEvidenceSection::error(InspectEvidenceScope::Symbol, message.to_string());
    InspectSemanticEvidence {
        semantic_trace: Some(section()),
        api_surface: Some(section()),
        symbol_impact: Some(section()),
        targeted_tests: Some(section()),
        type_aware: Some(fallow_types::envelope::TypeAwareMeta {
            required_completeness,
            ..Default::default()
        }),
        warnings: Vec::new(),
    }
}

struct InspectChildEvidence {
    dead_code: InspectEvidenceSection,
    duplication: InspectEvidenceSection,
    complexity: InspectEvidenceSection,
    security: InspectEvidenceSection,
    impact_closure: InspectEvidenceSection,
    churn: Option<InspectEvidenceSection>,
}

fn collect_inspect_child_evidence(
    opts: &InspectOptions<'_>,
    target: &NormalizedTarget,
    optional_threads: usize,
    transport: &dyn ChildJsonTransport,
) -> InspectChildEvidence {
    let target_file = target.file.as_str();

    std::thread::scope(|scope| {
        let dead_code = scope.spawn(|| {
            optional_section(
                opts,
                dead_code_args(target_file),
                InspectEvidenceScope::File,
                optional_threads,
                transport,
                |value| value,
            )
        });
        let duplication = scope.spawn(|| {
            optional_section(
                opts,
                dupes_args(),
                InspectEvidenceScope::ProjectFilteredToFile,
                optional_threads,
                transport,
                |value| filter_path_array(&value, target_file, "clone_groups"),
            )
        });
        let complexity = scope.spawn(|| {
            optional_section(
                opts,
                health_args(),
                InspectEvidenceScope::ProjectFilteredToFile,
                optional_threads,
                transport,
                |value| filter_path_array(&value, target_file, "findings"),
            )
        });
        let security = scope.spawn(|| {
            optional_section(
                opts,
                security_args(target_file),
                InspectEvidenceScope::File,
                optional_threads,
                transport,
                |value| value,
            )
        });
        let impact_closure = scope.spawn(|| {
            optional_section(
                opts,
                impact_closure_args(target_file),
                InspectEvidenceScope::ProjectFilteredToFile,
                optional_threads,
                transport,
                |value| value,
            )
        });
        let churn = opts.churn_cache_dir.map(|cache_dir| {
            scope.spawn(|| collect_target_churn_section(opts, target_file, cache_dir))
        });

        join_inspect_child_evidence(
            dead_code,
            duplication,
            complexity,
            security,
            impact_closure,
            churn,
        )
    })
}

const fn parallel_child_threads(parent_threads: usize) -> usize {
    let threads = parent_threads / 4;
    if threads == 0 { 1 } else { threads }
}

fn join_inspect_child_evidence(
    dead_code: std::thread::ScopedJoinHandle<'_, InspectEvidenceSection>,
    duplication: std::thread::ScopedJoinHandle<'_, InspectEvidenceSection>,
    complexity: std::thread::ScopedJoinHandle<'_, InspectEvidenceSection>,
    security: std::thread::ScopedJoinHandle<'_, InspectEvidenceSection>,
    impact_closure: std::thread::ScopedJoinHandle<'_, InspectEvidenceSection>,
    churn: Option<std::thread::ScopedJoinHandle<'_, InspectEvidenceSection>>,
) -> InspectChildEvidence {
    InspectChildEvidence {
        dead_code: join_inspect_section(dead_code, InspectEvidenceScope::File),
        duplication: join_inspect_section(duplication, InspectEvidenceScope::ProjectFilteredToFile),
        complexity: join_inspect_section(complexity, InspectEvidenceScope::ProjectFilteredToFile),
        security: join_inspect_section(security, InspectEvidenceScope::File),
        impact_closure: join_inspect_section(
            impact_closure,
            InspectEvidenceScope::ProjectFilteredToFile,
        ),
        churn: join_optional_inspect_section(churn, InspectEvidenceScope::ProjectFilteredToFile),
    }
}

fn join_optional_inspect_section(
    handle: Option<std::thread::ScopedJoinHandle<'_, InspectEvidenceSection>>,
    scope: InspectEvidenceScope,
) -> Option<InspectEvidenceSection> {
    handle.map(|handle| join_inspect_section(handle, scope))
}

fn join_inspect_section(
    handle: std::thread::ScopedJoinHandle<'_, InspectEvidenceSection>,
    scope: InspectEvidenceScope,
) -> InspectEvidenceSection {
    match handle.join() {
        Ok(section) => section,
        Err(_) => {
            InspectEvidenceSection::error(scope, "inspect evidence worker panicked".to_string())
        }
    }
}

fn collect_target_churn_section(
    opts: &InspectOptions<'_>,
    target_file: &str,
    cache_dir: &Path,
) -> InspectEvidenceSection {
    let options = fallow_engine::health::TargetChurnOptions {
        root: opts.root,
        target: Path::new(target_file),
        cache_dir: cache_dir.to_path_buf(),
        no_cache: opts.no_cache,
        since: None,
        min_commits: None,
    };
    churn_section_from_result(
        target_file,
        fallow_engine::health::analyze_target_churn(&options),
    )
}

fn churn_section_from_result(
    target_file: &str,
    result: Result<fallow_engine::health::TargetChurnOutcome, String>,
) -> InspectEvidenceSection {
    let scope = InspectEvidenceScope::ProjectFilteredToFile;
    match result {
        Ok(fallow_engine::health::TargetChurnOutcome::Found(evidence)) => {
            InspectEvidenceSection::ok(
                scope,
                json!({
                    "file": target_file,
                    "matched_count": 1,
                    "commits": evidence.file.commits,
                    "weighted_commits": evidence.file.weighted_commits,
                    "lines_added": evidence.file.lines_added,
                    "lines_deleted": evidence.file.lines_deleted,
                    "trend": evidence.file.trend,
                    "window": evidence.since.display,
                    "minimum_commits": evidence.min_commits,
                    "shallow_clone": evidence.shallow_clone,
                }),
            )
        }
        Ok(fallow_engine::health::TargetChurnOutcome::NoQualifyingChurn {
            observed_commits,
            since,
            min_commits,
            shallow_clone,
        }) => InspectEvidenceSection::ok(
            scope,
            json!({
                "file": target_file,
                "matched_count": 0,
                "observed_commits": observed_commits,
                "window": since.display,
                "minimum_commits": min_commits,
                "shallow_clone": shallow_clone,
            }),
        ),
        Ok(fallow_engine::health::TargetChurnOutcome::Unavailable { message }) => {
            InspectEvidenceSection::unavailable(scope, message)
        }
        Err(message) => InspectEvidenceSection::error(scope, message),
    }
}

/// Build the OPT-IN symbol-level call-chain section. Returns `None` (the
/// section is omitted) unless `--symbol-chain` was requested AND the target is a
/// SYMBOL. Best-effort, syntactic, OFF the ranked path: it is attached as
/// separate evidence, never folded into the trusted sections.
fn build_symbol_chain_section(
    opts: &InspectOptions<'_>,
    target: &NormalizedTarget,
    threads: usize,
    transport: &dyn ChildJsonTransport,
) -> Option<InspectEvidenceSection> {
    if !opts.symbol_chain {
        return None;
    }
    let export_name = target.export_name.as_deref()?;
    Some(optional_section(
        opts,
        symbol_chain_args(&target.file, export_name),
        InspectEvidenceScope::Symbol,
        threads,
        transport,
        |value| value,
    ))
}

/// Derive the identity summary from the trace evidence (symbol when an export
/// trace is present, file otherwise).
fn build_inspect_identity(
    target: &NormalizedTarget,
    trace_file: &Value,
    trace_export: Option<&Value>,
) -> InspectIdentity {
    match trace_export {
        Some(export) => InspectIdentity::Symbol(InspectSymbolIdentity {
            file: target.file.clone(),
            export_name: target.export_name.clone().unwrap_or_default(),
            file_reachable: export.get("file_reachable").cloned(),
            is_entry_point: export.get("is_entry_point").cloned(),
            is_used: export.get("is_used").cloned(),
            reason: export.get("reason").cloned(),
        }),
        None => InspectIdentity::File(InspectFileIdentity {
            file: target.file.clone(),
            is_reachable: trace_file.get("is_reachable").cloned(),
            is_entry_point: trace_file.get("is_entry_point").cloned(),
            export_count: trace_file
                .get("exports")
                .and_then(Value::as_array)
                .map(Vec::len),
            import_count: trace_file
                .get("imports_from")
                .and_then(Value::as_array)
                .map(Vec::len),
            imported_by_count: trace_file
                .get("imported_by")
                .and_then(Value::as_array)
                .map(Vec::len),
        }),
    }
}

/// Serialize and emit the inspect bundle in the requested output format.
fn emit_inspect_bundle(bundle: InspectOutput, opts: &InspectOptions<'_>) -> ExitCode {
    match opts.output {
        OutputFormat::Json => {
            let value = match fallow_output::serialize_inspect_json_output(
                bundle,
                crate::output_runtime::current_root_envelope_mode(),
                crate::output_runtime::telemetry_analysis_run_id().as_deref(),
            ) {
                Ok(value) => value,
                Err(err) => {
                    return emit_error(
                        &format!("failed to serialize inspect output: {err}"),
                        2,
                        opts.output,
                    );
                }
            };
            report::emit_report_json(&value, "inspect", opts.json_style)
        }
        OutputFormat::Human => {
            print_human(&bundle, opts.quiet);
            ExitCode::SUCCESS
        }
        _ => emit_error("inspect supports --format json or human", 2, opts.output),
    }
}

fn print_human(bundle: &InspectOutput, quiet: bool) {
    outln!("Inspect target");
    outln!();
    outln!("  target: {}", json_display(&bundle.target));
    outln!("  identity: {}", json_display(&bundle.identity));
    outln!();
    outln!("Evidence");
    print_evidence_summary("trace_file", &bundle.evidence.trace_file);
    if let Some(section) = bundle.evidence.trace_export.as_ref() {
        print_evidence_summary("trace_export", section);
    }
    print_evidence_summary("dead_code", &bundle.evidence.dead_code);
    print_evidence_summary("duplication", &bundle.evidence.duplication);
    print_evidence_summary("complexity", &bundle.evidence.complexity);
    print_evidence_summary("security", &bundle.evidence.security);
    print_evidence_summary("impact_closure", &bundle.evidence.impact_closure);
    if let Some(section) = bundle.evidence.churn.as_ref() {
        print_evidence_summary("churn", section);
    }
    if let Some(section) = bundle.evidence.symbol_chain.as_ref() {
        print_evidence_summary("symbol_chain", section);
    }
    if let Some(section) = bundle.evidence.semantic_trace.as_ref() {
        print_evidence_summary("semantic_trace", section);
    }
    if let Some(section) = bundle.evidence.api_surface.as_ref() {
        print_evidence_summary("api_surface", section);
    }
    if let Some(section) = bundle.evidence.symbol_impact.as_ref() {
        print_evidence_summary("symbol_impact", section);
    }
    if let Some(section) = bundle.evidence.targeted_tests.as_ref() {
        print_evidence_summary("targeted_tests", section);
    }
    if !bundle.warnings.is_empty() && !quiet {
        outln!();
        for warning in &bundle.warnings {
            outln!("  warning: {warning}");
        }
    }
}

fn json_display(value: &impl serde::Serialize) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "<unprintable>".to_string())
}

fn print_evidence_summary(name: &str, section: &InspectEvidenceSection) {
    let status = match section.status {
        InspectSectionStatus::Ok => "ok",
        InspectSectionStatus::Partial => "partial",
        InspectSectionStatus::Unavailable => "unavailable",
        InspectSectionStatus::Error => "error",
    };
    let detail = evidence_detail(section)
        .map(|detail| format!(" ({detail})"))
        .unwrap_or_default();
    outln!(
        "  {name}: {status} [{}]{detail}",
        evidence_scope_label(section.scope)
    );
}

fn evidence_scope_label(scope: InspectEvidenceScope) -> &'static str {
    match scope {
        InspectEvidenceScope::Symbol => "symbol",
        InspectEvidenceScope::File => "file",
        InspectEvidenceScope::ProjectFilteredToFile => "project filtered to file",
    }
}

fn evidence_detail(section: &InspectEvidenceSection) -> Option<String> {
    if let Some(message) = section.message.as_deref() {
        return Some(message.to_string());
    }
    let data = section.data.as_ref()?;
    if let Some(count) = data.get("matched_count").and_then(Value::as_u64) {
        return Some(format!("matches: {count}"));
    }
    if let Some(exports) = data.get("exports").and_then(Value::as_array) {
        return Some(format!("exports: {}", exports.len()));
    }
    None
}

fn run_required_json(
    opts: &InspectOptions<'_>,
    args: Vec<String>,
    transport: &dyn ChildJsonTransport,
) -> Result<Value, String> {
    run_child_json(opts, args, opts.threads, transport).and_then(|output| output.value)
}

fn optional_section<F>(
    opts: &InspectOptions<'_>,
    args: Vec<String>,
    scope: InspectEvidenceScope,
    threads: usize,
    transport: &dyn ChildJsonTransport,
    filter: F,
) -> InspectEvidenceSection
where
    F: FnOnce(Value) -> Value,
{
    match run_child_json(opts, args, threads, transport) {
        Ok(output) => match output.value {
            Ok(value) => InspectEvidenceSection::ok(scope, filter(value)),
            Err(message) => InspectEvidenceSection::error(scope, message),
        },
        Err(message) => InspectEvidenceSection::error(scope, message),
    }
}

struct ChildJson {
    value: Result<Value, String>,
}

struct ChildProcessOutput {
    code: i32,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}

trait ChildJsonTransport: Sync {
    fn run(&self, args: &[String]) -> Result<ChildProcessOutput, String>;
}

struct ProcessChildJsonTransport;

impl ChildJsonTransport for ProcessChildJsonTransport {
    fn run(&self, args: &[String]) -> Result<ChildProcessOutput, String> {
        let binary = std::env::current_exe()
            .map_err(|err| format!("failed to locate current fallow binary: {err}"))?;
        let output = Command::new(binary)
            .args(args)
            .output()
            .map_err(|err| format!("failed to run child analysis: {err}"))?;
        Ok(ChildProcessOutput {
            code: output.status.code().unwrap_or(2),
            stdout: output.stdout,
            stderr: output.stderr,
        })
    }
}

fn run_child_json(
    opts: &InspectOptions<'_>,
    args: Vec<String>,
    threads: usize,
    transport: &dyn ChildJsonTransport,
) -> Result<ChildJson, String> {
    let args = build_child_args(opts, args, threads);
    let output = transport.run(&args)?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    if output.code > 1 {
        let message = child_error_message(output.code, &stdout, &stderr);
        return Err(message);
    }
    if stdout.trim().is_empty() {
        return Ok(ChildJson {
            value: Err("child analysis returned no JSON".to_string()),
        });
    }
    Ok(ChildJson {
        value: serde_json::from_str(&stdout)
            .map_err(|err| format!("child analysis returned invalid JSON: {err}")),
    })
}

fn build_child_args(
    opts: &InspectOptions<'_>,
    command_args: Vec<String>,
    threads: usize,
) -> Vec<String> {
    let command_name = command_args.first().map(String::as_str);
    let mut args = vec![
        "--root".to_string(),
        opts.root.to_string_lossy().to_string(),
        "--format".to_string(),
        "json".to_string(),
        "--quiet".to_string(),
    ];
    if let Some(config) = opts.config_path {
        args.extend(["--config".to_string(), config.to_string_lossy().to_string()]);
    }
    if opts.no_cache {
        args.push("--no-cache".to_string());
    }
    if opts.no_production && command_name != Some("security") {
        args.push("--no-production".to_string());
    }
    if let Some(max_file_size) = opts.max_file_size {
        args.extend(["--max-file-size".to_string(), max_file_size.to_string()]);
    }
    args.extend(["--threads".to_string(), threads.to_string()]);
    if opts.production && command_name != Some("security") {
        args.push("--production".to_string());
    }
    if let Some(workspace) = opts.workspace {
        args.extend(["--workspace".to_string(), workspace.join(",")]);
    }
    args.extend(command_args);
    args
}

fn trace_file_args(file: &str) -> Vec<String> {
    vec![
        "dead-code".to_string(),
        "--trace-file".to_string(),
        file.to_string(),
    ]
}

fn trace_export_args(file: &str, export_name: &str) -> Vec<String> {
    vec![
        "dead-code".to_string(),
        "--trace".to_string(),
        format!("{file}:{export_name}"),
    ]
}

fn dead_code_args(file: &str) -> Vec<String> {
    vec![
        "dead-code".to_string(),
        "--file".to_string(),
        file.to_string(),
    ]
}

fn dupes_args() -> Vec<String> {
    vec!["dupes".to_string()]
}

fn health_args() -> Vec<String> {
    vec!["health".to_string(), "--complexity".to_string()]
}

fn security_args(file: &str) -> Vec<String> {
    vec![
        "security".to_string(),
        "--file".to_string(),
        file.to_string(),
    ]
}

fn impact_closure_args(file: &str) -> Vec<String> {
    vec![
        "dead-code".to_string(),
        "--impact-closure".to_string(),
        file.to_string(),
    ]
}

fn symbol_chain_args(file: &str, export_name: &str) -> Vec<String> {
    vec![
        "trace".to_string(),
        format!("{file}:{export_name}"),
        "--callers".to_string(),
        "--callees".to_string(),
    ]
}

fn filter_path_array(value: &Value, file: &str, key: &str) -> Value {
    let matched = value
        .get(key)
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter(|item| value_mentions_file(item, file))
                .cloned()
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let matched_count = matched.len();

    json!({
        key: matched,
        "matched_count": matched_count,
        "summary": value.get("summary").cloned(),
        "stats": value.get("stats").cloned(),
    })
}

fn value_mentions_file(value: &Value, file: &str) -> bool {
    match value {
        Value::String(s) => path_eq(s, file),
        Value::Array(items) => items.iter().any(|item| value_mentions_file(item, file)),
        Value::Object(map) => map.values().any(|item| value_mentions_file(item, file)),
        _ => false,
    }
}

fn path_eq(left: &str, right: &str) -> bool {
    left.replace('\\', "/") == right.replace('\\', "/")
}

fn normalize_target_file(root: &Path, file: &str) -> Result<String, String> {
    let raw = file.trim();
    let normalized_raw = raw.replace('\\', "/");
    let path = Path::new(&normalized_raw);
    let relative = if path.is_absolute() {
        let absolute = dunce::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        absolute
            .strip_prefix(root)
            .map_err(|_| {
                format!(
                    "inspect target must be inside the project root: {}",
                    absolute.display()
                )
            })?
            .to_path_buf()
    } else {
        path.to_path_buf()
    };
    let mut parts = Vec::new();
    for component in relative.components() {
        match component {
            Component::CurDir => {}
            Component::Normal(part) => parts.push(part.to_string_lossy().to_string()),
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(format!(
                    "inspect target must be a root-relative path inside the project: {raw}"
                ));
            }
        }
    }
    if parts.is_empty() {
        return Err("inspect target file must not be empty".to_string());
    }
    Ok(parts.join("/"))
}

fn child_error_message(code: i32, stdout: &str, stderr: &str) -> String {
    structured_child_message(stdout)
        .or_else(|| {
            let trimmed = strip_ansi(stderr.trim());
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed)
            }
        })
        .unwrap_or_else(|| format!("child analysis exited with code {code}"))
}

fn structured_child_message(stdout: &str) -> Option<String> {
    let value = serde_json::from_str::<Value>(stdout.trim()).ok()?;
    value
        .get("message")
        .or_else(|| value.get("error_message"))
        .and_then(Value::as_str)
        .map(strip_ansi)
        .filter(|message| !message.is_empty())
}

fn strip_ansi(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\u{1b}' && chars.peek() == Some(&'[') {
            chars.next();
            for next in chars.by_ref() {
                if ('@'..='~').contains(&next) {
                    break;
                }
            }
            continue;
        }
        if ch.is_control() && ch != '\n' && ch != '\t' {
            continue;
        }
        output.push(ch);
    }
    output.trim().to_string()
}

fn push_inspect_warnings(warnings: &mut Vec<String>, evidence: &InspectEvidence) {
    push_warning(warnings, "dead_code", &evidence.dead_code);
    push_warning(warnings, "duplication", &evidence.duplication);
    push_warning(warnings, "complexity", &evidence.complexity);
    push_warning(warnings, "security", &evidence.security);
    push_warning(warnings, "impact_closure", &evidence.impact_closure);
    if let Some(churn) = evidence.churn.as_ref() {
        push_warning(warnings, "churn", churn);
    }
}

fn push_warning(warnings: &mut Vec<String>, section: &str, evidence: &InspectEvidenceSection) {
    if !matches!(evidence.status, InspectSectionStatus::Ok)
        && let Some(message) = evidence.message.as_ref()
    {
        warnings.push(format!("{section} evidence unavailable: {message}"));
    }
}

fn require_non_empty(field: &str, value: &str) -> Result<(), String> {
    if value.trim().is_empty() {
        return Err(format!("{field} must not be empty"));
    }
    Ok(())
}

const INSPECT_BENCH_TARGET: &str = "src/target.ts";
const INSPECT_BENCH_CALL_COUNT: usize = 6;
const INSPECT_BENCH_EXPORT_COUNT: usize = 8;
const INSPECT_BENCH_IMPORT_COUNT: usize = 4;
const INSPECT_BENCH_IMPORTED_BY_COUNT: usize = 6;
const INSPECT_BENCH_FILTERED_COUNT: usize = 16;

struct InspectBenchmarkResponse {
    args: Vec<String>,
    slot: u8,
    stdout: Vec<u8>,
}

struct InspectBenchmarkRawResponse {
    command_args: Vec<String>,
    required: bool,
    stdout: Vec<u8>,
}

/// Prebuilt child-response corpus for the inspect benchmark. This is not a
/// supported API.
#[doc(hidden)]
pub struct InspectBenchmarkCorpus {
    responses: Vec<InspectBenchmarkResponse>,
}

struct InspectBenchmarkTransport<'a> {
    corpus: &'a InspectBenchmarkCorpus,
    calls: AtomicUsize,
    seen: AtomicU8,
}

impl ChildJsonTransport for InspectBenchmarkTransport<'_> {
    fn run(&self, args: &[String]) -> Result<ChildProcessOutput, String> {
        let response = self
            .corpus
            .responses
            .iter()
            .find(|response| args == response.args)
            .ok_or_else(|| format!("unexpected inspect benchmark child args: {args:?}"))?;
        let bit = 1 << response.slot;
        if self.seen.fetch_or(bit, Ordering::Relaxed) & bit != 0 {
            return Err(format!("duplicate inspect benchmark child args: {args:?}"));
        }
        self.calls.fetch_add(1, Ordering::Relaxed);
        Ok(ChildProcessOutput {
            code: 0,
            stdout: response.stdout.clone(),
            stderr: Vec::new(),
        })
    }
}

fn benchmark_json_response(
    command_args: Vec<String>,
    required: bool,
    value: Value,
) -> InspectBenchmarkRawResponse {
    let stdout = value.to_string().into_bytes();
    drop(value);
    InspectBenchmarkRawResponse {
        command_args,
        required,
        stdout,
    }
}

/// Build the response corpus outside the timed inspect benchmark. This is not
/// a supported API.
#[doc(hidden)]
pub fn create_inspect_benchmark_corpus(root: &Path, threads: usize) -> InspectBenchmarkCorpus {
    let exports = (0..INSPECT_BENCH_EXPORT_COUNT)
        .map(|index| json!({"name": format!("export_{index}")}))
        .collect::<Vec<_>>();
    let imports = (0..INSPECT_BENCH_IMPORT_COUNT)
        .map(|index| format!("src/dependency_{index}.ts"))
        .collect::<Vec<_>>();
    let imported_by = (0..INSPECT_BENCH_IMPORTED_BY_COUNT)
        .map(|index| format!("src/consumer_{index}.ts"))
        .collect::<Vec<_>>();
    let target_and_other = |key: &str| {
        (0..INSPECT_BENCH_FILTERED_COUNT * 2)
            .map(|index| {
                let file = if index % 2 == 0 {
                    INSPECT_BENCH_TARGET
                } else {
                    "src/unrelated.ts"
                };
                json!({key: file, "index": index})
            })
            .collect::<Vec<_>>()
    };
    let file_items = |kind: &str| {
        (0..INSPECT_BENCH_FILTERED_COUNT)
            .map(|index| json!({"file": INSPECT_BENCH_TARGET, "kind": kind, "index": index}))
            .collect::<Vec<_>>()
    };

    let responses = vec![
        benchmark_json_response(
            trace_file_args(INSPECT_BENCH_TARGET),
            true,
            json!({
                "is_reachable": true,
                "is_entry_point": false,
                "exports": exports,
                "imports_from": imports,
                "imported_by": imported_by,
            }),
        ),
        benchmark_json_response(
            dead_code_args(INSPECT_BENCH_TARGET),
            false,
            json!({"issues": file_items("unused_export"), "summary": {"total": INSPECT_BENCH_FILTERED_COUNT}}),
        ),
        benchmark_json_response(
            dupes_args(),
            false,
            json!({"clone_groups": target_and_other("path"), "summary": {"groups": INSPECT_BENCH_FILTERED_COUNT * 2}, "stats": {"files": 2}}),
        ),
        benchmark_json_response(
            health_args(),
            false,
            json!({"findings": target_and_other("file"), "summary": {"findings": INSPECT_BENCH_FILTERED_COUNT * 2}, "stats": {"files": 2}}),
        ),
        benchmark_json_response(
            security_args(INSPECT_BENCH_TARGET),
            false,
            json!({"findings": file_items("security"), "summary": {"total": INSPECT_BENCH_FILTERED_COUNT}}),
        ),
        benchmark_json_response(
            impact_closure_args(INSPECT_BENCH_TARGET),
            false,
            json!({"in_diff": [INSPECT_BENCH_TARGET], "affected_not_shown": imported_by}),
        ),
    ];
    let opts = inspect_benchmark_options(root, threads);
    let optional_threads = parallel_child_threads(threads);
    InspectBenchmarkCorpus {
        responses: responses
            .into_iter()
            .enumerate()
            .map(|(slot, response)| InspectBenchmarkResponse {
                args: build_child_args(
                    &opts,
                    response.command_args,
                    if response.required {
                        threads
                    } else {
                        optional_threads
                    },
                ),
                slot: u8::try_from(slot).unwrap_or(u8::MAX),
                stdout: response.stdout,
            })
            .collect(),
    }
}

fn inspect_benchmark_options(root: &Path, threads: usize) -> InspectOptions<'_> {
    InspectOptions {
        root,
        config_path: None,
        output: OutputFormat::Json,
        json_style: crate::json_style::JsonStyle::Compact,
        no_cache: true,
        no_production: true,
        max_file_size: None,
        threads,
        quiet: true,
        production: false,
        workspace: None,
        target: InspectTarget::File {
            file: INSPECT_BENCH_TARGET.to_string(),
        },
        churn_cache_dir: None,
        symbol_chain: false,
        type_aware: None,
        type_aware_projects: &[],
        type_aware_require: None,
    }
}

fn validate_inspect_benchmark_bundle(bundle: &InspectOutput) -> Result<(), String> {
    let InspectIdentity::File(identity) = &bundle.identity else {
        return Err("inspect benchmark returned a symbol identity".to_string());
    };
    if identity.export_count != Some(INSPECT_BENCH_EXPORT_COUNT)
        || identity.import_count != Some(INSPECT_BENCH_IMPORT_COUNT)
        || identity.imported_by_count != Some(INSPECT_BENCH_IMPORTED_BY_COUNT)
    {
        return Err("inspect benchmark identity counts changed".to_string());
    }
    for (name, section) in [
        ("trace_file", &bundle.evidence.trace_file),
        ("dead_code", &bundle.evidence.dead_code),
        ("duplication", &bundle.evidence.duplication),
        ("complexity", &bundle.evidence.complexity),
        ("security", &bundle.evidence.security),
        ("impact_closure", &bundle.evidence.impact_closure),
    ] {
        if section.status != InspectSectionStatus::Ok {
            return Err(format!("inspect benchmark {name} evidence was not ok"));
        }
    }
    for (name, section) in [
        ("duplication", &bundle.evidence.duplication),
        ("complexity", &bundle.evidence.complexity),
    ] {
        let count = section
            .data
            .as_ref()
            .and_then(|data| data.get("matched_count"))
            .and_then(Value::as_u64);
        if count != Some(INSPECT_BENCH_FILTERED_COUNT as u64) {
            return Err(format!("inspect benchmark {name} filtering changed"));
        }
    }
    if !bundle.warnings.is_empty() {
        return Err("inspect benchmark unexpectedly emitted warnings".to_string());
    }
    Ok(())
}

/// Run file inspect orchestration with prebuilt child responses, including
/// compact tagged JSON rendering. This is not a supported API.
#[doc(hidden)]
pub fn benchmark_inspect_file_evidence_bundle_json(
    root: &Path,
    threads: usize,
    corpus: &InspectBenchmarkCorpus,
) -> Result<(usize, usize), String> {
    let opts = inspect_benchmark_options(root, threads);
    let transport = InspectBenchmarkTransport {
        corpus,
        calls: AtomicUsize::new(0),
        seen: AtomicU8::new(0),
    };
    let bundle = build_inspect_bundle(&opts, &transport)?;
    validate_inspect_benchmark_bundle(&bundle)?;
    let value = fallow_output::serialize_inspect_json_output(
        bundle,
        crate::output_runtime::current_root_envelope_mode(),
        crate::output_runtime::telemetry_analysis_run_id().as_deref(),
    )
    .map_err(|error| format!("failed to serialize inspect benchmark output: {error}"))?;
    if value.get("kind").and_then(Value::as_str) != Some("inspect_target") {
        return Err("inspect benchmark output kind changed".to_string());
    }
    let rendered = serde_json::to_vec(&value)
        .map_err(|error| format!("failed to render inspect benchmark JSON: {error}"))?;
    let calls = transport.calls.load(Ordering::Relaxed);
    let expected_seen = (1 << INSPECT_BENCH_CALL_COUNT) - 1;
    if calls != INSPECT_BENCH_CALL_COUNT || transport.seen.load(Ordering::Relaxed) != expected_seen
    {
        return Err("inspect benchmark child calls changed".to_string());
    }
    Ok((calls, rendered.len()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    struct StubTransport {
        calls: Mutex<Vec<Vec<String>>>,
        fail_required: bool,
        fail_optional: bool,
    }

    impl ChildJsonTransport for StubTransport {
        fn run(&self, args: &[String]) -> Result<ChildProcessOutput, String> {
            self.calls.lock().unwrap().push(args.to_vec());
            let is_required = args.ends_with(&trace_file_args("src/api.ts"));
            if (is_required && self.fail_required) || (!is_required && self.fail_optional) {
                return Err("stub transport failed".to_string());
            }
            Ok(ChildProcessOutput {
                code: 0,
                stdout: if is_required {
                    br#"{"is_reachable":true,"is_entry_point":false,"exports":[],"imports_from":[],"imported_by":[]}"#.to_vec()
                } else {
                    b"{}".to_vec()
                },
                stderr: Vec::new(),
            })
        }
    }

    fn inspect_options<'a>(
        root: &'a Path,
        config_path: Option<&'a PathBuf>,
        target: InspectTarget,
    ) -> InspectOptions<'a> {
        InspectOptions {
            root,
            config_path,
            output: OutputFormat::Json,
            json_style: crate::json_style::JsonStyle::Compact,
            no_cache: true,
            no_production: true,
            max_file_size: Some(2),
            threads: 3,
            quiet: true,
            production: false,
            workspace: None,
            target,
            churn_cache_dir: None,
            symbol_chain: false,
            type_aware: None,
            type_aware_projects: &[],
            type_aware_require: None,
        }
    }

    #[test]
    fn normalized_target_uses_root_relative_posix_path() {
        let root = std::env::current_dir().unwrap();
        let file = root
            .join("src")
            .join("api.ts")
            .to_string_lossy()
            .to_string();

        let target = NormalizedTarget::new(&root, &InspectTarget::File { file }).unwrap();

        assert_eq!(target.file, "src/api.ts");
    }

    #[test]
    fn normalized_target_rejects_parent_paths() {
        let root = PathBuf::from("/repo");
        let file = "../other.ts".to_string();

        let err = NormalizedTarget::new(&root, &InspectTarget::File { file }).unwrap_err();

        assert!(err.contains("inside the project"));
    }

    #[test]
    fn completeness_gate_uses_effective_metadata_requirement() {
        let mut meta = fallow_types::envelope::TypeAwareMeta {
            required_completeness: Some(
                fallow_types::semantic::SemanticCompletenessRequirement::Complete,
            ),
            ..Default::default()
        };

        assert!(inspect_requires_complete(Some(&meta)));
        meta.queries
            .push(fallow_types::semantic::SemanticQuerySummary {
                query_id: 0,
                capability: fallow_types::semantic::SemanticCapability::SymbolTrace,
                assertion: "exact symbol trace".to_string(),
                status: fallow_types::semantic::SemanticCompleteness::Partial,
                reason_code: None,
                total_evidence_count: 0,
                truncated: false,
                omissions: Vec::new(),
                actions: Vec::new(),
            });
        assert!(inspect_type_aware_meta_incomplete(&meta));
    }

    #[test]
    fn semantic_error_preserves_effective_completeness_policy() {
        let evidence = semantic_error_sections(
            "sidecar unavailable",
            Some(fallow_types::semantic::SemanticCompletenessRequirement::Complete),
        );

        assert!(inspect_requires_complete(evidence.type_aware.as_ref()));
        assert_eq!(
            evidence
                .semantic_trace
                .as_ref()
                .map(|section| section.status),
            Some(InspectSectionStatus::Error)
        );
    }

    #[test]
    fn child_args_forward_global_inspect_overrides() {
        let root = PathBuf::from("/repo");
        let config_path = Some(PathBuf::from("/repo/.fallowrc.json"));
        let opts = inspect_options(
            &root,
            config_path.as_ref(),
            InspectTarget::File {
                file: "src/api.ts".to_string(),
            },
        );

        let args = build_child_args(&opts, dead_code_args("src/api.ts"), opts.threads);

        assert!(
            args.windows(2)
                .any(|pair| pair == ["--config", "/repo/.fallowrc.json"])
        );
        assert!(args.contains(&"--no-cache".to_string()));
        assert!(args.contains(&"--no-production".to_string()));
        assert!(args.windows(2).any(|pair| pair == ["--max-file-size", "2"]));
        assert!(args.windows(2).any(|pair| pair == ["--threads", "3"]));
    }

    #[test]
    fn child_args_do_not_forward_production_overrides_to_security() {
        let root = PathBuf::from("/repo");
        let config_path = None;
        let opts = inspect_options(
            &root,
            config_path.as_ref(),
            InspectTarget::File {
                file: "src/api.ts".to_string(),
            },
        );

        let args = build_child_args(&opts, security_args("src/api.ts"), opts.threads);

        assert!(!args.contains(&"--no-production".to_string()));
        assert!(!args.contains(&"--production".to_string()));
    }

    #[test]
    fn child_error_prefers_structured_stdout_message() {
        let stdout = r#"{"message":"\u001b[31mconfig failed\u001b[0m","exit_code":2}"#;
        let stderr = "warning before JSON\n";

        assert_eq!(child_error_message(2, stdout, stderr), "config failed");
    }

    #[test]
    fn parallel_child_threads_caps_optional_evidence_workers() {
        assert_eq!(parallel_child_threads(1), 1);
        assert_eq!(parallel_child_threads(4), 1);
        assert_eq!(parallel_child_threads(8), 2);
    }

    #[test]
    fn churn_section_distinguishes_no_history_unavailable_and_failure() {
        let since = fallow_engine::churn::parse_since("6m").unwrap();
        let no_history = churn_section_from_result(
            "src/api.ts",
            Ok(
                fallow_engine::health::TargetChurnOutcome::NoQualifyingChurn {
                    observed_commits: Some(2),
                    since,
                    min_commits: 3,
                    shallow_clone: false,
                },
            ),
        );
        let unavailable = churn_section_from_result(
            "src/api.ts",
            Ok(fallow_engine::health::TargetChurnOutcome::Unavailable {
                message: "git repository unavailable".to_string(),
            }),
        );
        let failed =
            churn_section_from_result("src/api.ts", Err("git churn analysis failed".to_string()));

        assert_eq!(no_history.status, InspectSectionStatus::Ok);
        assert_eq!(no_history.data.unwrap()["matched_count"], 0);
        assert_eq!(unavailable.status, InspectSectionStatus::Unavailable);
        assert_eq!(failed.status, InspectSectionStatus::Error);
    }

    #[test]
    fn churn_worker_failure_remains_a_partial_evidence_section() {
        std::thread::scope(|scope| {
            let churn = scope
                .spawn(|| -> InspectEvidenceSection { panic!("simulated churn worker failure") });
            let section = join_optional_inspect_section(
                Some(churn),
                InspectEvidenceScope::ProjectFilteredToFile,
            )
            .expect("requested churn must retain a section");

            assert_eq!(section.status, InspectSectionStatus::Error);
            assert_eq!(
                section.message.as_deref(),
                Some("inspect evidence worker panicked")
            );
        });
    }

    #[test]
    fn required_transport_failure_stops_before_optional_evidence() {
        let root = PathBuf::from("/repo");
        let opts = inspect_options(
            &root,
            None,
            InspectTarget::File {
                file: "src/api.ts".to_string(),
            },
        );
        let transport = StubTransport {
            calls: Mutex::new(Vec::new()),
            fail_required: true,
            fail_optional: false,
        };

        let error = build_inspect_bundle(&opts, &transport).unwrap_err();

        assert_eq!(error, "stub transport failed");
        let calls = transport.calls.lock().unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(
            calls[0],
            build_child_args(&opts, trace_file_args("src/api.ts"), opts.threads)
        );
        drop(calls);
    }

    #[test]
    fn optional_transport_failures_remain_tagged_evidence_errors() {
        let root = PathBuf::from("/repo");
        let opts = inspect_options(
            &root,
            None,
            InspectTarget::File {
                file: "src/api.ts".to_string(),
            },
        );
        let transport = StubTransport {
            calls: Mutex::new(Vec::new()),
            fail_required: false,
            fail_optional: true,
        };

        let bundle = build_inspect_bundle(&opts, &transport).unwrap();

        assert_eq!(bundle.evidence.trace_file.status, InspectSectionStatus::Ok);
        for section in [
            &bundle.evidence.dead_code,
            &bundle.evidence.duplication,
            &bundle.evidence.complexity,
            &bundle.evidence.security,
            &bundle.evidence.impact_closure,
        ] {
            assert_eq!(section.status, InspectSectionStatus::Error);
            assert_eq!(section.message.as_deref(), Some("stub transport failed"));
        }
        assert_eq!(transport.calls.lock().unwrap().len(), 6);
    }
}
