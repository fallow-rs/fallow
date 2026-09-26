//! `fallow coverage analyze` implementation.

use std::fmt;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};
use std::time::Instant;

use fallow_config::OutputFormat;
use fallow_cov_protocol::function_identity_id;
use fallow_engine::changed_files::clear_ambient_git_env;
use fallow_engine::source::inventory::{
    InventoryComplexity, InventoryEntry, walk_source_with_complexity,
};
use fallow_types::cloud::CLOUD_API_KEY_MISSING_MESSAGE;
use rayon::prelude::*;
use rustc_hash::{FxHashMap, FxHashSet};

use crate::coverage::RunContext;
use crate::coverage::cloud_client::{
    CloudError, CloudNeverCalledSource, CloudRequest, CloudRuntimeContext, CloudRuntimeFunction,
    CloudRuntimeProvenance, CloudRuntimeWarning, CloudTrackingState, fetch_runtime_context,
};
use crate::coverage::upload_common::parse_git_remote_to_project_id;
use crate::coverage::upload_inventory::extension_supported;
use crate::error::emit_error;
use crate::health::HealthOptions;
use crate::health::optimization_target::{StaticCost, optimization_target};
use fallow_output::{
    RUNTIME_STALE_AFTER_DAYS, RuntimeCoverageAction, RuntimeCoverageCaptureQuality,
    RuntimeCoverageConfidence, RuntimeCoverageDataSource, RuntimeCoverageEvidence,
    RuntimeCoverageFinding, RuntimeCoverageHotPath, RuntimeCoverageMessage,
    RuntimeCoverageProvenance, RuntimeCoverageReport, RuntimeCoverageReportVerdict,
    RuntimeCoverageRiskBand, RuntimeCoverageSchemaVersion, RuntimeCoverageSummary,
    RuntimeCoverageVerdict,
};

const RUNTIME_COVERAGE_SCHEMA_VERSION: &str = "1";

#[derive(Clone, Default)]
pub struct AnalyzeArgs {
    pub runtime_coverage: Option<PathBuf>,
    pub cloud: bool,
    pub api_key: Option<String>,
    pub api_endpoint: Option<String>,
    pub repo: Option<String>,
    pub project_id: Option<String>,
    pub coverage_period: u16,
    pub environment: Option<String>,
    pub commit_sha: Option<String>,
    pub production: bool,
    pub min_invocations_hot: u64,
    pub min_observation_volume: Option<u32>,
    pub low_traffic_threshold: Option<f64>,
    pub top: Option<usize>,
    pub blast_radius: bool,
    pub importance: bool,
    /// List every cloud runtime function that found no local counterpart on
    /// stderr, instead of only counting them in the `cloud_functions_unmatched`
    /// warning.
    pub debug_unmatched: bool,
}

impl fmt::Debug for AnalyzeArgs {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AnalyzeArgs")
            .field("runtime_coverage", &self.runtime_coverage)
            .field("cloud", &self.cloud)
            .field("api_key", &self.api_key.as_ref().map(|_| "***"))
            .field("api_endpoint", &self.api_endpoint)
            .field("repo", &self.repo)
            .field("project_id", &self.project_id)
            .field("coverage_period", &self.coverage_period)
            .field("environment", &self.environment)
            .field("commit_sha", &self.commit_sha)
            .field("production", &self.production)
            .field("min_invocations_hot", &self.min_invocations_hot)
            .field("min_observation_volume", &self.min_observation_volume)
            .field("low_traffic_threshold", &self.low_traffic_threshold)
            .field("top", &self.top)
            .field("blast_radius", &self.blast_radius)
            .field("importance", &self.importance)
            .field("debug_unmatched", &self.debug_unmatched)
            .finish()
    }
}

pub fn run(args: &AnalyzeArgs, ctx: &RunContext<'_>) -> ExitCode {
    if let Err(message) = validate_output_format(ctx.output) {
        return emit_error(&message, 2, ctx.output);
    }

    let env_cloud = runtime_coverage_source_env_is_cloud();
    let cloud = args.cloud || env_cloud;
    if cloud && args.runtime_coverage.is_some() {
        return emit_error(
            "Choose one runtime coverage source: --cloud or --runtime-coverage <path>.",
            2,
            ctx.output,
        );
    }

    if cloud {
        return run_cloud(args, ctx);
    }

    let Some(path) = args.runtime_coverage.as_deref() else {
        return emit_error(
            "No runtime coverage source selected. Pass --runtime-coverage <path>, --cloud, or set FALLOW_RUNTIME_COVERAGE_SOURCE=cloud.",
            2,
            ctx.output,
        );
    };
    run_local(path, args, ctx)
}

/// `fallow coverage analyze` only emits two output formats: structured JSON
/// (the canonical agent-readable shape, used by every non-`Human` `--format`
/// today) and the terse human renderer. Other formats (`compact`, `markdown`,
/// `sarif`, `codeclimate`, `badge`) require shape conversion that this
/// command does not yet implement; falling through to the JSON serializer
/// would silently mislead consumers expecting SARIF or markdown. Reject them
/// explicitly so the user gets an actionable error instead.
fn validate_output_format(output: OutputFormat) -> Result<(), String> {
    match output {
        OutputFormat::Json | OutputFormat::Human => Ok(()),
        OutputFormat::Compact
        | OutputFormat::Markdown
        | OutputFormat::Sarif
        | OutputFormat::CodeClimate
        | OutputFormat::PrCommentGithub
        | OutputFormat::PrCommentGitlab
        | OutputFormat::ReviewGithub
        | OutputFormat::ReviewGitlab
        | OutputFormat::Badge
        | OutputFormat::GithubAnnotations
        | OutputFormat::GithubSummary => Err(format!(
            "fallow coverage analyze only supports --format json or --format human (got {output:?}). Use `fallow coverage analyze --format json` and pipe to your own converter for {output:?}."
        )),
    }
}

fn run_local(path: &Path, args: &AnalyzeArgs, ctx: &RunContext<'_>) -> ExitCode {
    let runtime_coverage = match crate::health::coverage::prepare_options(
        path,
        args.min_invocations_hot,
        args.min_observation_volume,
        args.low_traffic_threshold,
        ctx.output,
    ) {
        Ok(options) => options,
        Err(code) => return code,
    };
    let options = local_health_options(args, ctx, runtime_coverage);
    let result = match crate::health::execute_health(&options) {
        Ok(result) => result,
        Err(code) => return code,
    };
    let Some(report) = result.report.runtime_coverage else {
        return emit_error("runtime coverage report was not produced", 2, ctx.output);
    };
    print_runtime_report(&report, ctx, result.elapsed, args)
}

pub fn benchmark_local_json(
    root: &Path,
    runtime_coverage_path: &Path,
    response_bytes: &[u8],
    threads: usize,
) -> Result<(usize, usize, usize, String), ExitCode> {
    let args = AnalyzeArgs {
        runtime_coverage: Some(runtime_coverage_path.to_path_buf()),
        min_invocations_hot: 100,
        ..AnalyzeArgs::default()
    };
    let config_path = None;
    let ctx = RunContext {
        root,
        config_path: &config_path,
        output: OutputFormat::Json,
        json_style: crate::json_style::JsonStyle::Compact,
        quiet: true,
        no_cache: true,
        threads,
        explain: false,
        allow_remote_extends: false,
    };
    let runtime_coverage = fallow_engine::health::RuntimeCoverageOptions {
        path: runtime_coverage_path.to_path_buf(),
        min_invocations_hot: args.min_invocations_hot,
        min_observation_volume: args.min_observation_volume,
        low_traffic_threshold: args.low_traffic_threshold,
        license_jwt: String::new(),
        watermark: None,
    };
    let options = local_health_options(&args, &ctx, runtime_coverage);
    let request_len = std::cell::Cell::new(0);
    let result = crate::health::benchmark_execute_health_with_response(
        &options,
        response_bytes,
        &request_len,
    )?;
    let report = result
        .report
        .runtime_coverage
        .ok_or_else(|| ExitCode::from(2))?;
    let output =
        runtime_json_output(&report, result.elapsed, false).map_err(|_| ExitCode::from(2))?;
    let rendered = crate::json_style::JsonStyle::Compact
        .serialize(&output)
        .map_err(|_| ExitCode::from(2))?;
    Ok((
        report.findings.len(),
        report.hot_paths.len(),
        request_len.get(),
        rendered,
    ))
}

/// Build the `HealthOptions` for a local `coverage analyze` run: complexity,
/// hotspot, and gating features are off so the run focuses on the supplied
/// runtime-coverage artifact.
fn local_health_options<'a>(
    args: &AnalyzeArgs,
    ctx: &RunContext<'a>,
    runtime_coverage: fallow_engine::health::RuntimeCoverageOptions,
) -> HealthOptions<'a> {
    HealthOptions {
        root: ctx.root,
        config_path: ctx.config_path,
        output: ctx.output,
        no_cache: ctx.no_cache,
        threads: ctx.threads,
        quiet: ctx.quiet,
        thresholds: fallow_engine::health::HealthThresholdOverrides::default(),
        top: args.top,
        sort: fallow_engine::health::HealthSort::Cyclomatic,
        production: args.production,
        production_override: Some(args.production),
        allow_remote_extends: ctx.allow_remote_extends,
        changed_since: None,
        diff_index: None,
        use_shared_diff_index: true,
        workspace: None,
        changed_workspaces: None,
        baseline: None,
        save_baseline: None,
        baseline_mode: fallow_engine::baseline::HealthBaselineMode::default(),
        baseline_mode_explicit: false,
        complexity: false,
        file_scores: false,
        coverage_gaps: false,
        config_activates_coverage_gaps: false,
        hotspots: false,
        ownership: false,
        ownership_emails: None,
        targets: false,
        css: false,
        css_deep: false,
        force_full: false,
        score_only_output: false,
        enforce_coverage_gap_gate: false,
        effort: None,
        score: false,
        gates: fallow_engine::health::HealthGateOptions::default(),
        since: None,
        min_commits: None,
        explain: ctx.explain,
        summary: false,
        save_snapshot: None,
        trend: false,
        coverage_inputs: fallow_engine::health::HealthCoverageInputs::default(),
        performance: false,
        runtime_coverage: Some(runtime_coverage),
        churn_file: None,
        analysis_identity: fallow_types::semantic::SemanticAnalysisIdentity::default(),
        complexity_breakdown: false,
        group_by: None,
        scope: None,
    }
}

fn run_cloud(args: &AnalyzeArgs, ctx: &RunContext<'_>) -> ExitCode {
    let api_key = match resolve_api_key(args.api_key.as_deref()) {
        Ok(api_key) => api_key,
        Err(err) => return emit_cloud_error(&err, ctx.output),
    };
    let repo = match resolve_repo(args.repo.as_deref(), ctx.root) {
        Ok(repo) => repo,
        Err(err) => return emit_cloud_error(&err, ctx.output),
    };
    let request = CloudRequest {
        api_key,
        api_endpoint: args.api_endpoint.clone(),
        repo,
        project_id: args.project_id.clone(),
        period_days: args.coverage_period,
        environment: args.environment.clone(),
        commit_sha: args.commit_sha.clone(),
    };

    let start = Instant::now();
    let snapshot = match fetch_runtime_context(&request) {
        Ok(snapshot) => snapshot,
        Err(err) => return emit_cloud_error(&err, ctx.output),
    };
    let static_index = match build_static_index(ctx, args.production) {
        Ok(index) => index,
        Err(code) => return code,
    };
    let CloudMergeOutput {
        mut report,
        unmatched,
    } = merge_cloud_snapshot(&snapshot, &static_index, args.min_invocations_hot);
    if args.debug_unmatched {
        print_unmatched_cloud_functions(&unmatched);
    }
    apply_top_limit(&mut report, args.top);
    print_runtime_report(&report, ctx, start.elapsed(), args)
}

/// Print the unmatched cloud runtime functions on stderr, highest traffic
/// first, so the class of misses is visible without a debugger. Kept off
/// stdout so `--format json` stays machine-readable.
fn print_unmatched_cloud_functions(unmatched: &[UnmatchedCloudFunction]) {
    if unmatched.is_empty() {
        eprintln!("unmatched cloud functions: none");
        return;
    }
    let mut sorted: Vec<&UnmatchedCloudFunction> = unmatched.iter().collect();
    sorted.sort_by(|left, right| {
        right
            .invocations
            .cmp(&left.invocations)
            .then_with(|| left.path.cmp(&right.path))
            .then_with(|| left.name.cmp(&right.name))
    });
    eprintln!("unmatched cloud functions: {}", sorted.len());
    for function in sorted {
        let line = function
            .line
            .map_or_else(|| "?".to_owned(), |line| line.to_string());
        eprintln!(
            "  {}:{} {} ({} invocations)",
            function.path, line, function.name, function.invocations
        );
    }
}

fn runtime_coverage_source_env_is_cloud() -> bool {
    std::env::var("FALLOW_RUNTIME_COVERAGE_SOURCE")
        .is_ok_and(|value| value.trim().eq_ignore_ascii_case("cloud"))
}

fn resolve_api_key(explicit: Option<&str>) -> Result<String, CloudError> {
    if let Some(value) = explicit.map(str::trim).filter(|value| !value.is_empty()) {
        return Ok(value.to_owned());
    }
    if let Ok(value) = std::env::var("FALLOW_API_KEY") {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            return Ok(trimmed.to_owned());
        }
    }
    Err(CloudError::Auth(CLOUD_API_KEY_MISSING_MESSAGE.to_owned()))
}

fn resolve_repo(explicit: Option<&str>, root: &Path) -> Result<String, CloudError> {
    if let Some(value) = explicit.map(str::trim).filter(|value| !value.is_empty()) {
        return Ok(value.to_owned());
    }
    if let Ok(value) = std::env::var("FALLOW_REPO") {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            return Ok(trimmed.to_owned());
        }
    }
    if let Some(from_remote) = git_origin_project_id(root) {
        return Ok(from_remote);
    }
    Err(CloudError::Validation(
        "Could not infer repository for cloud runtime coverage.\n\nPass it explicitly:\n\n  fallow coverage analyze --cloud --repo owner/repo\n\nor set:\n\n  FALLOW_REPO=owner/repo".to_owned(),
    ))
}

fn git_origin_project_id(root: &Path) -> Option<String> {
    let mut command = Command::new("git");
    command
        .args(["remote", "get-url", "origin"])
        .current_dir(root);
    clear_ambient_git_env(&mut command);
    let output = command.output().ok()?;
    if !output.status.success() {
        return None;
    }
    parse_git_remote_to_project_id(String::from_utf8_lossy(&output.stdout).trim())
}

fn emit_cloud_error(err: &CloudError, output: OutputFormat) -> ExitCode {
    match err {
        CloudError::Auth(_) | CloudError::TierRequired(_) => {
            crate::telemetry::note_failure_reason(crate::telemetry::FailureReason::Auth);
        }
        CloudError::Network(_) | CloudError::Server(_) => {
            crate::telemetry::note_failure_reason(crate::telemetry::FailureReason::Network);
        }
        CloudError::Validation(_) => {
            crate::telemetry::note_failure_reason(crate::telemetry::FailureReason::Validation);
        }
        CloudError::NotFound(_) => {
            crate::telemetry::note_failure_reason(crate::telemetry::FailureReason::Config);
        }
    }
    emit_error(err.message(), err.exit_code(), output)
}

#[derive(Debug, Clone)]
struct StaticFunctionInfo {
    path: PathBuf,
    name: String,
    start_line: u32,
    end_line: u32,
    static_used: bool,
    /// `Some(true)` when production mode dropped the only files that reference
    /// this function, so `static_used` reads `false` purely because the test,
    /// story, or fixture side of the tree is missing from the graph.
    /// `Some(false)` when both graphs were built and no such reference exists,
    /// `None` when the run was not filtered and there is no second answer.
    test_only_reference: Option<bool>,
    test_covered: bool,
    cyclomatic: u32,
    /// Static cost inputs for the hot-path optimization target. `None` when
    /// the source inventory carried no complexity for the function.
    cost: Option<StaticCost>,
    caller_count: u32,
    owner_count: Option<u32>,
    /// Cross-surface join key (`fallow:fn:<hash>`) computed over the
    /// repo-relative `path`. Agrees with the static-inventory producer's
    /// `stable_id` for the same function, so a cloud function carrying a
    /// `stable_id` joins here directly.
    stable_id: String,
    /// Content digest of the function's full-span source slice
    /// (`FunctionComplexity.source_hash`). Stable across line moves, so a
    /// finding built from this function carries a line-move-immune key for
    /// baseline suppression.
    source_hash: Option<String>,
    /// Whether `name` is the callee this function was passed to
    /// (`arr.map(cb)` -> `map`) rather than a declared or bound name. The
    /// runtime instrumenter names callbacks that way, so the join key reads
    /// like a declaration while the source has no declaration to point at;
    /// the verdict copy says "callback passed to X" instead.
    is_callback: bool,
}

#[derive(Default)]
struct StaticIndex {
    by_key: FxHashMap<(String, String, u32), StaticFunctionInfo>,
    by_path_name: FxHashMap<(String, String), Vec<StaticFunctionInfo>>,
    /// Stable-id join tier: the strongest match, tried before
    /// `(path, name, line)` and the fuzzy line fallback.
    by_stable_id: FxHashMap<String, StaticFunctionInfo>,
    /// Positional join tier, keyed by `(path, start_line)` and deliberately
    /// name-free. Runtime instrumentation names a function from its
    /// surroundings (an anonymous callback takes the callee's name, an
    /// accessor keeps its `get`/`set` prefix), so a name comparison drops
    /// exactly the callback-heavy, high-traffic functions. Position is the
    /// part both sides agree on.
    by_path_line: FxHashMap<(String, u32), Vec<StaticFunctionInfo>>,
    /// File name to repo-relative paths, used to rebase a runtime path that
    /// carries a container or bundle prefix onto the local tree.
    paths_by_file_name: FxHashMap<String, Vec<String>>,
}

/// One dead-code analysis run, kept together with the session so callers can
/// still read the resolved root and config after the artifacts are produced.
struct StaticAnalysisRun {
    session: fallow_engine::session::AnalysisSession,
    artifacts: fallow_engine::dead_code::DeadCodeAnalysisArtifacts,
}

/// Run dead-code analysis over the project, with or without the production
/// file filter.
fn run_static_analysis(
    ctx: &RunContext<'_>,
    production: bool,
) -> Result<StaticAnalysisRun, ExitCode> {
    let config = crate::load_config_for_analysis(
        ctx.root,
        ctx.config_path,
        crate::ConfigLoadOptions {
            output: ctx.output,
            no_cache: ctx.no_cache,
            threads: ctx.threads,
            production_override: Some(production),
            quiet: ctx.quiet,
            allow_remote_extends: ctx.allow_remote_extends,
        },
        fallow_config::ProductionAnalysis::Health,
    )?;
    let session = fallow_engine::session::AnalysisSession::from_resolved_config(config)
        .map_err(|err| emit_error(&format!("analysis failed: {err}"), 2, ctx.output))?;
    let artifacts = session
        .analyze_dead_code_with_artifacts(true, true)
        .map_err(|err| emit_error(&format!("analysis failed: {err}"), 2, ctx.output))?;
    Ok(StaticAnalysisRun { session, artifacts })
}

fn build_static_index(ctx: &RunContext<'_>, production: bool) -> Result<StaticIndex, ExitCode> {
    // Production mode drops test, spec, story, and fixture files from
    // discovery, so an export that only a test references reads as statically
    // unused and, with zero production invocations, as safe to delete. A
    // second unfiltered run answers "is anything left referencing it", which
    // separates a genuinely dead export from a test-only one. It runs only in
    // production mode, where the first answer is the ambiguous one.
    let full_tree = if production {
        Some(UnusedStaticSets::from_analysis(
            &run_static_analysis(ctx, false)?.artifacts,
        ))
    } else {
        None
    };
    let run = run_static_analysis(ctx, production)?;
    let analysis_output = &run.artifacts;
    let Some(modules) = analysis_output.modules.as_deref() else {
        return Err(emit_error(
            "analysis failed: engine did not retain parsed modules",
            2,
            ctx.output,
        ));
    };
    let Some(files) = analysis_output.files.as_deref() else {
        return Err(emit_error(
            "analysis failed: engine did not retain discovered files",
            2,
            ctx.output,
        ));
    };
    let file_paths: FxHashMap<_, _> = files.iter().map(|file| (file.id, &file.path)).collect();
    let codeowners = crate::codeowners::CodeOwners::load(
        run.session.root(),
        run.session.config().codeowners.as_deref(),
    )
    .ok();
    Ok(build_index_from_analysis(
        run.session.root(),
        modules,
        analysis_output,
        &file_paths,
        codeowners.as_ref(),
        full_tree.as_ref(),
    ))
}

fn build_index_from_analysis(
    root: &Path,
    modules: &[fallow_types::extract::ModuleInfo],
    analysis_output: &fallow_engine::dead_code::DeadCodeAnalysisArtifacts,
    file_paths: &FxHashMap<fallow_types::discover::FileId, &PathBuf>,
    codeowners: Option<&crate::codeowners::CodeOwners>,
    full_tree: Option<&UnusedStaticSets>,
) -> StaticIndex {
    let reachability = Reachability {
        analysed: UnusedStaticSets::from_analysis(analysis_output),
        full_tree,
    };
    let mut out = StaticIndex::default();
    let graph = analysis_output.graph.as_ref();
    let mut walked = Vec::with_capacity(modules.len());
    for module in modules {
        let Some(path) = file_paths.get(&module.file_id) else {
            continue;
        };
        let rel = normalize_runtime_path(path.strip_prefix(root).unwrap_or(path));
        let caller_count = graph.map_or(0_usize, |g| g.direct_importer_count(module.file_id));
        let caller_count = u32::try_from(caller_count).unwrap_or(u32::MAX);
        let owner_count = codeowners.map(|co| co.owner_count_of(Path::new(&rel)).unwrap_or(0));
        for function in &module.complexity {
            let info = static_function_info(
                function,
                path.as_path(),
                &rel,
                &reachability,
                caller_count,
                owner_count,
            );
            index_static_function(&mut out, &rel, info);
        }
        walked.push(ModuleIndexContext {
            path: (*path).clone(),
            rel,
            caller_count,
            owner_count,
        });
    }
    index_instrumenter_functions(&mut out, &walked, &reachability);
    out
}

/// The per-file facts the instrumenter-name pass needs after the complexity
/// pass has consumed the parsed module.
struct ModuleIndexContext {
    /// Absolute path of the file on disk, re-read by the inventory walker.
    path: PathBuf,
    /// Repo-relative posix path, the one the identity hash is taken over.
    rel: String,
    caller_count: u32,
    owner_count: Option<u32>,
}

/// Index every function the runtime instrumenter would name, on top of the
/// complexity pass.
///
/// The health/complexity pass enumerates declarations, so a callback passed to
/// a call (`sqliteTable("t", {}, (table) => [...])`, `.references(() => ...)`,
/// `rows.map(...)`), an object-literal method, and an accessor never enter the
/// index. A cloud row for one of those carries the instrumenter's name, so it
/// has nothing to join against and the hottest functions in a service are
/// dropped from `findings` and `hot_paths`. The inventory walker already
/// reproduces `oxc-coverage-instrument`'s naming for exactly this contract, so
/// walking the same files with it closes the gap on the identity the cloud
/// stores. Complexity-pass entries win every collision; only identities the
/// first pass never produced are added.
fn index_instrumenter_functions(
    out: &mut StaticIndex,
    contexts: &[ModuleIndexContext],
    reachability: &Reachability<'_>,
) {
    let per_file: Vec<Vec<StaticFunctionInfo>> = contexts
        .par_iter()
        .map(|context| instrumenter_functions_for_file(context, reachability))
        .collect();
    for (context, functions) in contexts.iter().zip(per_file) {
        for info in functions {
            if out.by_stable_id.contains_key(&info.stable_id) {
                continue;
            }
            index_static_function(out, &context.rel, info);
        }
    }
}

/// Walk one file with the inventory walker and build static info for every
/// function it names. A file the walker cannot parse, or cannot read, yields
/// nothing: the complexity-pass entries for it stay as they are.
fn instrumenter_functions_for_file(
    context: &ModuleIndexContext,
    reachability: &Reachability<'_>,
) -> Vec<StaticFunctionInfo> {
    if !extension_supported(&context.path) {
        return Vec::new();
    }
    let Ok(source) = std::fs::read_to_string(&context.path) else {
        return Vec::new();
    };
    let (entries, complexity) = walk_source_with_complexity(&context.path, &source);
    entries
        .into_iter()
        .map(|entry| {
            // Complexity is paired by `source_hash`, which the walker derives
            // from the same full-span slice it hashes for the entry, so the
            // lookup is exact.
            let metrics = complexity.get(&entry.source_hash).copied();
            instrumenter_function_info(entry, metrics, context, reachability)
        })
        .collect()
}

/// Build a `StaticFunctionInfo` for one inventory entry.
fn instrumenter_function_info(
    entry: InventoryEntry,
    metrics: Option<InventoryComplexity>,
    context: &ModuleIndexContext,
    reachability: &Reachability<'_>,
) -> StaticFunctionInfo {
    let static_used =
        reachability
            .analysed
            .function_is_used(&context.path, &entry.name, entry.line);
    let stable_id = function_identity_id(&context.rel, &entry.name, entry.line);
    let test_only_reference = reachability
        .full_tree
        .map(|full| !static_used && full.function_is_used(&context.path, &entry.name, entry.line));
    StaticFunctionInfo {
        path: PathBuf::from(&context.rel),
        name: entry.name,
        start_line: entry.line,
        end_line: entry.end_line,
        static_used,
        test_only_reference,
        test_covered: false,
        cyclomatic: metrics.map_or(0, |metrics| u32::from(metrics.cyclomatic)),
        cost: metrics.map(|metrics| StaticCost {
            cognitive: metrics.cognitive,
            cyclomatic: metrics.cyclomatic,
            line_count: entry.end_line.saturating_sub(entry.line),
        }),
        caller_count: context.caller_count,
        owner_count: context.owner_count,
        stable_id,
        source_hash: Some(entry.source_hash),
        is_callback: entry.is_callback,
    }
}

/// Per-file sets of statically-unused files and exports, used to flag whether a
/// function is reachable in the static graph.
struct UnusedStaticSets {
    files: FxHashSet<PathBuf>,
    export_names: FxHashMap<PathBuf, FxHashSet<String>>,
    export_lines: FxHashMap<PathBuf, FxHashSet<u32>>,
}

impl UnusedStaticSets {
    fn from_analysis(
        analysis_output: &fallow_engine::dead_code::DeadCodeAnalysisArtifacts,
    ) -> Self {
        let files: FxHashSet<PathBuf> = analysis_output
            .results
            .unused_files
            .iter()
            .map(|file| file.file.path.clone())
            .collect();
        let mut export_names: FxHashMap<PathBuf, FxHashSet<String>> = FxHashMap::default();
        let mut export_lines: FxHashMap<PathBuf, FxHashSet<u32>> = FxHashMap::default();
        for finding in &analysis_output.results.unused_exports {
            let export = &finding.export;
            export_names
                .entry(export.path.clone())
                .or_default()
                .insert(export.export_name.clone());
            export_lines
                .entry(export.path.clone())
                .or_default()
                .insert(export.line);
        }
        Self {
            files,
            export_names,
            export_lines,
        }
    }

    fn function_is_used(&self, path: &Path, name: &str, line: u32) -> bool {
        !self.files.contains(path)
            && !self
                .export_names
                .get(path)
                .is_some_and(|names| names.contains(name))
            && !self
                .export_lines
                .get(path)
                .is_some_and(|lines| lines.contains(&line))
    }
}

/// The reachability answers a function is scored against: the sets the run's
/// own analysis produced, plus the unfiltered sets when that analysis applied
/// the production file filter.
struct Reachability<'a> {
    analysed: UnusedStaticSets,
    full_tree: Option<&'a UnusedStaticSets>,
}

/// Build a `StaticFunctionInfo` for one extracted function.
fn static_function_info(
    function: &fallow_types::extract::FunctionComplexity,
    path: &Path,
    rel: &str,
    reachability: &Reachability<'_>,
    caller_count: u32,
    owner_count: Option<u32>,
) -> StaticFunctionInfo {
    let static_used =
        reachability
            .analysed
            .function_is_used(path, function.name.as_str(), function.line);
    StaticFunctionInfo {
        path: PathBuf::from(rel),
        name: function.name.clone(),
        start_line: function.line,
        end_line: function.line.saturating_add(function.line_count),
        static_used,
        test_only_reference: reachability.full_tree.map(|full| {
            !static_used && full.function_is_used(path, function.name.as_str(), function.line)
        }),
        test_covered: false,
        cyclomatic: u32::from(function.cyclomatic),
        cost: Some(StaticCost {
            cognitive: function.cognitive,
            cyclomatic: function.cyclomatic,
            line_count: function.line_count,
        }),
        caller_count,
        owner_count,
        stable_id: function_identity_id(rel, &function.name, function.line),
        source_hash: function.source_hash.clone(),
        // The complexity pass only enumerates declarations and bindings, so
        // nothing it produces is a callee-named callback.
        is_callback: false,
    }
}

/// Insert a function's static info into every lookup tier of the index.
fn index_static_function(out: &mut StaticIndex, rel: &str, info: StaticFunctionInfo) {
    out.by_key.insert(
        (rel.to_string(), info.name.clone(), info.start_line),
        info.clone(),
    );
    out.by_stable_id
        .insert(info.stable_id.clone(), info.clone());
    out.by_path_line
        .entry((rel.to_string(), info.start_line))
        .or_default()
        .push(info.clone());
    let file_name = path_file_name(rel).to_owned();
    let paths = out.paths_by_file_name.entry(file_name).or_default();
    if !paths.iter().any(|path| path == rel) {
        paths.push(rel.to_string());
    }
    out.by_path_name
        .entry((rel.to_string(), info.name.clone()))
        .or_default()
        .push(info);
}

/// Last `/`-separated segment of an already normalized path.
fn path_file_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// The merged report plus the runtime functions that found no local
/// counterpart, kept alongside the report so `--debug-unmatched` can list them.
struct CloudMergeOutput {
    report: RuntimeCoverageReport,
    unmatched: Vec<UnmatchedCloudFunction>,
}

fn merge_cloud_snapshot(
    snapshot: &CloudRuntimeContext,
    static_index: &StaticIndex,
    min_invocations_hot: u64,
) -> CloudMergeOutput {
    let CloudMergeEntries {
        mut findings,
        mut hot_paths,
        synthesized_blast_radius,
        synthesized_importance,
        unmatched_cloud_functions,
    } = collect_cloud_merge_entries(snapshot, static_index, min_invocations_hot);

    sort_cloud_runtime_entries(&mut findings, &mut hot_paths);
    let blast_radius = cloud_blast_radius_entries(snapshot, synthesized_blast_radius);
    let importance = cloud_importance_entries(snapshot, synthesized_importance);

    let warnings = cloud_warnings(snapshot, unmatched_cloud_functions.len());
    let trust_output = cloud_runtime_trust_output(snapshot);

    let report = RuntimeCoverageReport {
        schema_version: RuntimeCoverageSchemaVersion::V1,
        verdict: cloud_report_verdict(&findings),
        signals: Vec::new(),
        summary: cloud_report_summary(snapshot),
        findings,
        hot_paths,
        blast_radius,
        importance,
        watermark: None,
        warnings,
        actionable: trust_output.actionable,
        actionability_reason: trust_output.actionability_reason,
        actionability_verdict: trust_output.actionability_verdict,
        provenance: trust_output.provenance,
    };
    CloudMergeOutput {
        report,
        unmatched: unmatched_cloud_functions,
    }
}

fn sort_cloud_runtime_entries(
    findings: &mut [RuntimeCoverageFinding],
    hot_paths: &mut [RuntimeCoverageHotPath],
) {
    findings.sort_by(|left, right| {
        runtime_verdict_rank(left.verdict)
            .cmp(&runtime_verdict_rank(right.verdict))
            .then_with(|| left.path.cmp(&right.path))
            .then_with(|| left.function.cmp(&right.function))
    });
    hot_paths.sort_by(|left, right| {
        right
            .invocations
            .cmp(&left.invocations)
            .then_with(|| left.path.cmp(&right.path))
            .then_with(|| left.function.cmp(&right.function))
    });
}

fn cloud_report_verdict(findings: &[RuntimeCoverageFinding]) -> RuntimeCoverageReportVerdict {
    if findings.is_empty() {
        RuntimeCoverageReportVerdict::Clean
    } else {
        RuntimeCoverageReportVerdict::ColdCodeDetected
    }
}

fn cloud_report_summary(snapshot: &CloudRuntimeContext) -> RuntimeCoverageSummary {
    RuntimeCoverageSummary {
        data_source: RuntimeCoverageDataSource::Cloud,
        last_received_at: snapshot.summary.last_received_at.clone(),
        functions_tracked: snapshot.summary.functions_tracked,
        functions_hit: snapshot.summary.functions_hit,
        functions_unhit: snapshot.summary.functions_unhit,
        functions_untracked: snapshot.summary.functions_untracked,
        coverage_percent: snapshot.summary.coverage_percent,
        trace_count: snapshot.summary.trace_count,
        period_days: snapshot.window.period_days,
        deployments_seen: snapshot.summary.deployments_seen,
        capture_quality: cloud_capture_quality(snapshot),
    }
}

struct RuntimeTrustOutput {
    actionable: bool,
    actionability_reason: Option<String>,
    actionability_verdict: Option<String>,
    provenance: RuntimeCoverageProvenance,
}

fn cloud_runtime_trust_output(snapshot: &CloudRuntimeContext) -> RuntimeTrustOutput {
    let functions_tracked = snapshot.summary.functions_tracked;
    let functions_untracked = snapshot.summary.functions_untracked;
    let fallback_actionable = functions_tracked > 0;
    let actionable = snapshot.actionable.unwrap_or(fallback_actionable);
    let (actionability_reason, actionability_verdict) = if actionable {
        (None, None)
    } else {
        (
            snapshot
                .actionability_reason
                .clone()
                .or_else(|| runtime_actionability_reason(functions_tracked)),
            snapshot
                .verdict
                .clone()
                .or_else(|| runtime_actionability_verdict(functions_tracked))
                .or_else(|| Some("insufficient_evidence".to_owned())),
        )
    };

    RuntimeTrustOutput {
        actionable,
        actionability_reason,
        actionability_verdict,
        provenance: cloud_runtime_provenance(
            snapshot.provenance.as_ref(),
            functions_tracked,
            functions_untracked,
        ),
    }
}

fn cloud_runtime_provenance(
    provenance: Option<&CloudRuntimeProvenance>,
    functions_tracked: usize,
    functions_untracked: usize,
) -> RuntimeCoverageProvenance {
    RuntimeCoverageProvenance {
        data_source: RuntimeCoverageDataSource::Cloud,
        is_production: provenance
            .and_then(|value| value.is_production.as_ref())
            .map_or_else(|| "unknown".to_owned(), |value| value.label()),
        freshness_days: provenance.and_then(|value| value.freshness_days),
        untracked_ratio: provenance
            .and_then(|value| value.untracked_ratio)
            .unwrap_or_else(|| runtime_untracked_ratio(functions_tracked, functions_untracked)),
        unresolved_ratio: provenance
            .and_then(|value| value.unresolved_ratio)
            .unwrap_or(0.0),
        stale: provenance.and_then(|value| value.stale).unwrap_or(false),
        stale_after_days: provenance
            .and_then(|value| value.stale_after_days)
            .unwrap_or(RUNTIME_STALE_AFTER_DAYS),
    }
}

fn runtime_actionability_reason(functions_tracked: usize) -> Option<String> {
    (functions_tracked == 0).then(|| {
        "No functions were tracked at runtime in this capture, so there is no usable runtime evidence to act on. Treat all functions as do-not-act; this is NOT cold."
            .to_owned()
    })
}

fn runtime_actionability_verdict(functions_tracked: usize) -> Option<String> {
    (functions_tracked == 0).then(|| "insufficient_evidence".to_owned())
}

fn runtime_untracked_ratio(functions_tracked: usize, functions_untracked: usize) -> f64 {
    let denominator = functions_tracked + functions_untracked;
    if denominator == 0 {
        0.0
    } else {
        functions_untracked as f64 / denominator as f64
    }
}

struct CloudMergeEntries {
    findings: Vec<RuntimeCoverageFinding>,
    hot_paths: Vec<RuntimeCoverageHotPath>,
    synthesized_blast_radius: Vec<fallow_output::RuntimeCoverageBlastRadiusEntry>,
    synthesized_importance: Vec<(fallow_output::RuntimeCoverageImportanceEntry, Option<u32>)>,
    unmatched_cloud_functions: Vec<UnmatchedCloudFunction>,
}

/// One cloud runtime function with no local counterpart, kept so the drop is
/// inspectable instead of only counted.
#[derive(Debug, Clone)]
pub struct UnmatchedCloudFunction {
    pub path: String,
    pub name: String,
    pub line: Option<u32>,
    pub invocations: u64,
}

fn collect_cloud_merge_entries(
    snapshot: &CloudRuntimeContext,
    static_index: &StaticIndex,
    min_invocations_hot: u64,
) -> CloudMergeEntries {
    let mut entries = CloudMergeEntries {
        findings: Vec::new(),
        hot_paths: Vec::new(),
        synthesized_blast_radius: Vec::new(),
        synthesized_importance: Vec::new(),
        unmatched_cloud_functions: Vec::new(),
    };
    for function in &snapshot.functions {
        let Some(local) = match_cloud_function(function, static_index) else {
            let line = function.start_line.or(function.line_number);
            tracing::debug!(
                path = %function.file_path,
                function = %function.function_name,
                line = ?line,
                invocations = function.hit_count.unwrap_or(0),
                "cloud runtime function has no local counterpart; omitted from findings"
            );
            entries
                .unmatched_cloud_functions
                .push(UnmatchedCloudFunction {
                    path: function.file_path.clone(),
                    name: function.function_name.clone(),
                    line,
                    invocations: function.hit_count.unwrap_or(0),
                });
            continue;
        };
        if matches!(function.tracking_state, CloudTrackingState::Called) {
            collect_called_cloud_function(&mut entries, function, &local, min_invocations_hot);
        } else {
            entries
                .findings
                .push(cloud_finding(function, &local, snapshot.window.period_days));
        }
    }
    entries
}

fn collect_called_cloud_function(
    entries: &mut CloudMergeEntries,
    function: &CloudRuntimeFunction,
    local: &StaticFunctionInfo,
    min_invocations_hot: u64,
) {
    if let Some(invocations) = function.hit_count
        && invocations >= min_invocations_hot
    {
        entries.hot_paths.push(cloud_hot_path(local, invocations));
    }
    if let Some(invocations) = function.hit_count {
        entries
            .synthesized_blast_radius
            .push(cloud_blast_radius(local, invocations, function));
        entries
            .synthesized_importance
            .push(cloud_importance(local, invocations));
    }
}

fn cloud_blast_radius_entries(
    snapshot: &CloudRuntimeContext,
    synthesized: Vec<fallow_output::RuntimeCoverageBlastRadiusEntry>,
) -> Vec<fallow_output::RuntimeCoverageBlastRadiusEntry> {
    if snapshot.blast_radius.is_empty() {
        return synthesized;
    }
    snapshot
        .blast_radius
        .iter()
        .map(|entry| fallow_output::RuntimeCoverageBlastRadiusEntry {
            id: entry.id.clone(),
            stable_id: entry.stable_id.clone(),
            file: PathBuf::from(&entry.file),
            function: entry.function.clone(),
            line: entry.line,
            caller_count: entry.caller_count.unwrap_or(0),
            caller_count_weighted_by_traffic: entry.caller_count_weighted_by_traffic.unwrap_or(0),
            deploys_touched: entry.deploys_touched,
            risk_band: map_cloud_risk_band(entry.risk_band),
        })
        .collect()
}

fn cloud_importance_entries(
    snapshot: &CloudRuntimeContext,
    synthesized: Vec<(fallow_output::RuntimeCoverageImportanceEntry, Option<u32>)>,
) -> Vec<fallow_output::RuntimeCoverageImportanceEntry> {
    if snapshot.importance.is_empty() {
        return rank_importance(synthesized);
    }
    snapshot
        .importance
        .iter()
        .map(|entry| fallow_output::RuntimeCoverageImportanceEntry {
            id: entry.id.clone(),
            stable_id: entry.stable_id.clone(),
            file: PathBuf::from(&entry.file),
            function: entry.function.clone(),
            line: entry.line,
            invocations: entry.invocations,
            cyclomatic: entry.cyclomatic.unwrap_or(0),
            owner_count: entry.owner_count.unwrap_or(0),
            importance_score: entry.importance_score,
            reason: entry.reason.clone(),
        })
        .collect()
}

fn cloud_hot_path(local: &StaticFunctionInfo, invocations: u64) -> RuntimeCoverageHotPath {
    RuntimeCoverageHotPath {
        id: stable_runtime_id("hot", &local.path, &local.name, local.start_line),
        stable_id: Some(local.stable_id.clone()),
        path: local.path.clone(),
        function: local.name.clone(),
        line: local.start_line,
        end_line: local.end_line,
        invocations,
        percentile: 100,
        actions: Vec::new(),
        optimization_target: local
            .cost
            .map(|cost| optimization_target(invocations, cost, None)),
    }
}

fn cloud_blast_radius(
    local: &StaticFunctionInfo,
    invocations: u64,
    function: &CloudRuntimeFunction,
) -> fallow_output::RuntimeCoverageBlastRadiusEntry {
    let weighted = invocations.saturating_mul(u64::from(local.caller_count));
    fallow_output::RuntimeCoverageBlastRadiusEntry {
        id: stable_runtime_id("blast", &local.path, &local.name, local.start_line),
        stable_id: Some(local.stable_id.clone()),
        file: local.path.clone(),
        function: local.name.clone(),
        line: local.start_line,
        caller_count: local.caller_count,
        caller_count_weighted_by_traffic: weighted,
        deploys_touched: Some(function.deployments_observed),
        risk_band: blast_radius_risk_band(local.caller_count, weighted),
    }
}

fn cloud_importance(
    local: &StaticFunctionInfo,
    invocations: u64,
) -> (fallow_output::RuntimeCoverageImportanceEntry, Option<u32>) {
    let owner_count = local.owner_count.unwrap_or(0);
    (
        fallow_output::RuntimeCoverageImportanceEntry {
            id: stable_runtime_id("importance", &local.path, &local.name, local.start_line),
            stable_id: Some(local.stable_id.clone()),
            file: local.path.clone(),
            function: local.name.clone(),
            line: local.start_line,
            invocations,
            cyclomatic: local.cyclomatic,
            owner_count,
            importance_score: 0.0,
            reason: importance_reason(invocations, local.cyclomatic, local.owner_count),
        },
        local.owner_count,
    )
}

fn cloud_finding(
    function: &CloudRuntimeFunction,
    local: &StaticFunctionInfo,
    observation_days: u32,
) -> RuntimeCoverageFinding {
    let (verdict, confidence, invocations) = cloud_finding_decision(function, local);
    RuntimeCoverageFinding {
        id: stable_runtime_id("prod", &local.path, &local.name, local.start_line),
        stable_id: Some(local.stable_id.clone()),
        source_hash: local.source_hash.clone(),
        path: local.path.clone(),
        function: local.name.clone(),
        line: local.start_line,
        verdict,
        invocations,
        confidence,
        evidence: RuntimeCoverageEvidence {
            static_status: if local.static_used { "used" } else { "unused" }.to_owned(),
            test_coverage: if local.test_covered {
                "covered"
            } else {
                "not_covered"
            }
            .to_owned(),
            test_only_reference: local.test_only_reference,
            v8_tracking: cloud_v8_tracking(function.tracking_state).to_owned(),
            untracked_reason: function.untracked_reason.clone(),
            observation_days,
            deployments_observed: function.deployments_observed,
        },
        actions: runtime_actions(
            verdict,
            local.test_only_reference == Some(true),
            local.is_callback.then_some(local.name.as_str()),
        ),
        // The cloud-join path (analyze --cloud) does not carry the window
        // trace_count + thresholds here, so it omits the #321 discriminator
        // block; that surface's discriminator contract is #328 territory.
        discriminators: None,
    }
}

fn rank_importance(
    entries: Vec<(fallow_output::RuntimeCoverageImportanceEntry, Option<u32>)>,
) -> Vec<fallow_output::RuntimeCoverageImportanceEntry> {
    let max_log = entries
        .iter()
        .map(|(entry, _)| (entry.invocations as f64).ln_1p())
        .fold(0.0_f64, f64::max);
    let mut ranked = entries
        .into_iter()
        .map(|(mut entry, owner_count)| {
            let normalized_traffic = if max_log <= f64::EPSILON {
                0.0
            } else {
                (entry.invocations as f64).ln_1p() / max_log
            };
            let complexity_weight = 1.0 + (f64::from(entry.cyclomatic).min(20.0) / 20.0);
            let ownership_risk_weight = match owner_count {
                Some(count) if count <= 1 => 1.5,
                Some(_) => 1.0,
                None => 1.2,
            };
            entry.importance_score =
                (normalized_traffic * 50.0 * complexity_weight * ownership_risk_weight)
                    .clamp(0.0, 100.0);
            entry.importance_score = (entry.importance_score * 10.0).round() / 10.0;
            entry
        })
        .collect::<Vec<_>>();
    ranked.sort_by(|left, right| {
        right
            .importance_score
            .total_cmp(&left.importance_score)
            .then_with(|| right.invocations.cmp(&left.invocations))
            .then_with(|| left.file.cmp(&right.file))
            .then_with(|| left.function.cmp(&right.function))
    });
    ranked
}

fn importance_reason(invocations: u64, cyclomatic: u32, owner_count: Option<u32>) -> String {
    let traffic = if invocations >= 1_000_000 {
        "High traffic"
    } else if invocations >= 10_000 {
        "Moderate traffic"
    } else {
        "Low traffic"
    };
    let complexity = if cyclomatic >= 10 {
        "high complexity"
    } else if cyclomatic >= 5 {
        "moderate complexity"
    } else {
        "low complexity"
    };
    let ownership = match owner_count {
        Some(0) => "unowned",
        Some(1) => "single owner",
        Some(_) => "multiple owners",
        None => "no CODEOWNERS data",
    };
    format!("{traffic}, {complexity}, {ownership}")
}

fn blast_radius_risk_band(caller_count: u32, weighted: u64) -> RuntimeCoverageRiskBand {
    if caller_count >= 20 || weighted >= 1_000_000 {
        RuntimeCoverageRiskBand::High
    } else if caller_count >= 5 || weighted >= 50_000 {
        RuntimeCoverageRiskBand::Medium
    } else {
        RuntimeCoverageRiskBand::Low
    }
}

const fn map_cloud_risk_band(
    risk_band: crate::coverage::cloud_client::CloudRuntimeRiskBand,
) -> RuntimeCoverageRiskBand {
    match risk_band {
        crate::coverage::cloud_client::CloudRuntimeRiskBand::Low => RuntimeCoverageRiskBand::Low,
        crate::coverage::cloud_client::CloudRuntimeRiskBand::Medium => {
            RuntimeCoverageRiskBand::Medium
        }
        crate::coverage::cloud_client::CloudRuntimeRiskBand::High => RuntimeCoverageRiskBand::High,
        crate::coverage::cloud_client::CloudRuntimeRiskBand::Unknown => {
            RuntimeCoverageRiskBand::Low
        }
    }
}

fn cloud_finding_decision(
    function: &CloudRuntimeFunction,
    local: &StaticFunctionInfo,
) -> (
    RuntimeCoverageVerdict,
    RuntimeCoverageConfidence,
    Option<u64>,
) {
    match function.tracking_state {
        CloudTrackingState::NeverCalled => match function.never_called_source {
            CloudNeverCalledSource::RuntimeObserved => (
                // A function that only a test file reaches is statically
                // unused in the production graph, but deleting it breaks that
                // test, so it is never safe to delete on runtime evidence
                // alone.
                if local.static_used || local.test_only_reference == Some(true) {
                    RuntimeCoverageVerdict::ReviewRequired
                } else {
                    RuntimeCoverageVerdict::SafeToDelete
                },
                RuntimeCoverageConfidence::High,
                Some(0),
            ),
            CloudNeverCalledSource::InventoryBackfill | CloudNeverCalledSource::Unknown => (
                RuntimeCoverageVerdict::ReviewRequired,
                RuntimeCoverageConfidence::Low,
                Some(0),
            ),
        },
        CloudTrackingState::Untracked => (
            RuntimeCoverageVerdict::CoverageUnavailable,
            RuntimeCoverageConfidence::None,
            None,
        ),
        CloudTrackingState::Unknown | CloudTrackingState::Called => (
            RuntimeCoverageVerdict::Unknown,
            RuntimeCoverageConfidence::Low,
            function.hit_count,
        ),
    }
}

fn cloud_v8_tracking(state: CloudTrackingState) -> &'static str {
    match state {
        CloudTrackingState::Called | CloudTrackingState::NeverCalled => "tracked",
        CloudTrackingState::Untracked | CloudTrackingState::Unknown => "untracked",
    }
}

fn cloud_warnings(
    snapshot: &CloudRuntimeContext,
    unmatched_cloud_functions: usize,
) -> Vec<RuntimeCoverageMessage> {
    let mut warnings = snapshot
        .warnings
        .iter()
        .enumerate()
        .map(|(index, warning)| match warning {
            CloudRuntimeWarning::Message(message) => RuntimeCoverageMessage {
                code: format!("cloud_warning_{index}"),
                message: message.clone(),
            },
            CloudRuntimeWarning::Object { code, message } => RuntimeCoverageMessage {
                code: code
                    .clone()
                    .unwrap_or_else(|| format!("cloud_warning_{index}")),
                message: message.clone().unwrap_or_default(),
            },
        })
        .collect::<Vec<_>>();
    let server_emitted_no_runtime_data = warnings
        .iter()
        .any(|warning| warning.code == "no_runtime_data");
    if snapshot.summary.trace_count == 0
        && snapshot.functions.is_empty()
        && !server_emitted_no_runtime_data
    {
        let repo = if snapshot.repo.trim().is_empty() {
            "this repository"
        } else {
            snapshot.repo.as_str()
        };
        warnings.push(RuntimeCoverageMessage {
            code: "no_runtime_data".to_owned(),
            message: format!(
                "No runtime coverage data received for {repo} in the last {} days.",
                snapshot.window.period_days
            ),
        });
    }
    if unmatched_cloud_functions > 0 {
        warnings.push(RuntimeCoverageMessage {
            code: "cloud_functions_unmatched".to_owned(),
            message: format!(
                "{unmatched_cloud_functions} cloud runtime function(s) were not matched in the local AST/static analysis and were omitted from findings."
            ),
        });
    }
    dedupe_warnings(warnings)
}

/// Deduplicate warnings by `(code, message)`. The server-side runtime-context
/// emits `no_runtime_data` in its empty-window response while the CLI also
/// derives the same code from `trace_count == 0 && functions.is_empty()`, so
/// the merged list can contain identical entries.
fn dedupe_warnings(warnings: Vec<RuntimeCoverageMessage>) -> Vec<RuntimeCoverageMessage> {
    let mut seen: FxHashSet<(String, String)> = FxHashSet::default();
    warnings
        .into_iter()
        .filter(|warning| seen.insert((warning.code.clone(), warning.message.clone())))
        .collect()
}

fn cloud_capture_quality(snapshot: &CloudRuntimeContext) -> Option<RuntimeCoverageCaptureQuality> {
    let has_data = snapshot.summary.functions_tracked > 0
        || snapshot.summary.functions_untracked > 0
        || snapshot.summary.trace_count > 0
        || snapshot.summary.deployments_seen > 0;
    if !has_data {
        return None;
    }
    let tracked = snapshot.summary.functions_tracked;
    let untracked = snapshot.summary.functions_untracked;
    let total = tracked.saturating_add(untracked);
    let untracked_ratio_percent = if total == 0 {
        0.0
    } else {
        let raw = (untracked as f64) * 100.0 / (total as f64);
        (raw * 100.0).round() / 100.0
    };
    Some(RuntimeCoverageCaptureQuality {
        window_seconds: u64::from(snapshot.window.period_days).saturating_mul(86_400),
        instances_observed: snapshot.summary.deployments_seen,
        lazy_parse_warning: untracked_ratio_percent > 30.0,
        untracked_ratio_percent,
    })
}

fn match_cloud_function(
    function: &CloudRuntimeFunction,
    static_index: &StaticIndex,
) -> Option<StaticFunctionInfo> {
    if let Some(stable_id) = function.stable_id.as_deref()
        && let Some(info) = static_index.by_stable_id.get(stable_id)
    {
        return Some(info.clone());
    }
    let runtime_path = normalize_runtime_path(Path::new(&function.file_path));
    let path = resolve_cloud_path(static_index, &runtime_path)?.to_owned();
    let line = function.start_line.or(function.line_number)?;
    if let Some(info) =
        static_index
            .by_key
            .get(&(path.clone(), function.function_name.clone(), line))
    {
        if let Some(stable_id) = function.stable_id.as_deref()
            && stable_id != info.stable_id
        {
            tracing::debug!(
                cloud_stable_id = stable_id,
                local_stable_id = %info.stable_id,
                path = %path,
                function = %function.function_name,
                "stable_id present on both sides but diverged; matched by path/name/line"
            );
        }
        return Some(info.clone());
    }
    if let Some(info) = static_index
        .by_path_name
        .get(&(path.clone(), function.function_name.clone()))
        .and_then(|candidates| nearest_cloud_candidate(candidates, line, function.end_line))
    {
        return Some(info);
    }
    static_index
        .by_path_line
        .get(&(path, line))
        .and_then(|candidates| positional_cloud_candidate(candidates, function.end_line))
}

/// Rebase a runtime file path onto the repo-relative path the static index is
/// keyed by.
///
/// A runtime path is whatever the process saw: a containerized service reports
/// `/app/src/a.ts`, a bundled worker reports a path below its output root. None
/// of those equal the repo-relative path, so comparing the full string drops
/// every function in the file. Candidates are looked up by file name and
/// accepted when one path is a segment-wise suffix of the other; the longest
/// such overlap wins, and a tie between two different local files is left
/// unmatched rather than guessed.
fn resolve_cloud_path<'index>(
    static_index: &'index StaticIndex,
    runtime_path: &str,
) -> Option<&'index str> {
    let candidates = static_index
        .paths_by_file_name
        .get(path_file_name(runtime_path))?;
    let runtime_segments: Vec<&str> = runtime_path.split('/').filter(|s| !s.is_empty()).collect();
    let mut best: Option<(&str, usize)> = None;
    let mut tied = false;
    for candidate in candidates {
        let candidate_segments: Vec<&str> =
            candidate.split('/').filter(|s| !s.is_empty()).collect();
        let overlap = candidate_segments.len().min(runtime_segments.len());
        if runtime_segments[runtime_segments.len() - overlap..]
            != candidate_segments[candidate_segments.len() - overlap..]
        {
            continue;
        }
        match best {
            None => {
                best = Some((candidate.as_str(), overlap));
                tied = false;
            }
            Some((_, current)) if overlap > current => {
                best = Some((candidate.as_str(), overlap));
                tied = false;
            }
            Some((_, current)) if overlap == current => tied = true,
            Some(_) => {}
        }
    }
    if tied {
        None
    } else {
        best.map(|(path, _)| path)
    }
}

/// Pick the one static function that starts on the runtime function's line.
///
/// Reached only after the name tiers missed, so the name is known to disagree.
/// A single candidate on the line is that function; several mean nested
/// definitions opening on one line, and the end line breaks the tie when the
/// runtime reported one. Anything still ambiguous stays unmatched.
fn positional_cloud_candidate(
    candidates: &[StaticFunctionInfo],
    end_line: Option<u32>,
) -> Option<StaticFunctionInfo> {
    if let [only] = candidates {
        return Some(only.clone());
    }
    let end_line = end_line?;
    let mut best: Option<(&StaticFunctionInfo, u32)> = None;
    let mut tied = false;
    for candidate in candidates {
        let distance = candidate.end_line.abs_diff(end_line);
        match best {
            None => {
                best = Some((candidate, distance));
                tied = false;
            }
            Some((_, current)) if distance < current => {
                best = Some((candidate, distance));
                tied = false;
            }
            Some((_, current)) if distance == current => tied = true,
            Some(_) => {}
        }
    }
    if tied {
        None
    } else {
        best.map(|(candidate, _)| candidate.clone())
    }
}

fn nearest_cloud_candidate(
    candidates: &[StaticFunctionInfo],
    start_line: u32,
    end_line: Option<u32>,
) -> Option<StaticFunctionInfo> {
    let mut best: Option<(&StaticFunctionInfo, (u32, u32))> = None;
    let mut tied = false;

    for candidate in candidates {
        let start_delta = candidate.start_line.abs_diff(start_line);
        if start_delta > 5 {
            continue;
        }
        let end_delta = match end_line {
            Some(line) => {
                let delta = candidate.end_line.abs_diff(line);
                if delta > 5 {
                    continue;
                }
                delta
            }
            None => 0,
        };
        let distance = (start_delta, end_delta);
        match best {
            None => {
                best = Some((candidate, distance));
                tied = false;
            }
            Some((_, current)) if distance < current => {
                best = Some((candidate, distance));
                tied = false;
            }
            Some((_, current)) if distance == current => {
                tied = true;
            }
            Some(_) => {}
        }
    }

    if tied {
        None
    } else {
        best.map(|(candidate, _)| candidate.clone())
    }
}

fn normalize_runtime_path(path: &Path) -> String {
    path.to_string_lossy()
        .trim_start_matches('/')
        .replace('\\', "/")
}

/// Follow-up actions for a finding. `test_only_reference` narrows the
/// review-required advice to the case where the only remaining callers live in
/// files production mode excluded, because there the reviewer has a concrete
/// choice to make rather than a general "look at this".
/// Build the action list for one runtime finding.
///
/// `callback_callee` is set when the function has no declared name of its own
/// and is known only as the callee it was passed to, so the copy can point at
/// the call site (`callback passed to map`) rather than tell the reader to go
/// find a declaration that does not exist.
fn runtime_actions(
    verdict: RuntimeCoverageVerdict,
    test_only_reference: bool,
    callback_callee: Option<&str>,
) -> Vec<RuntimeCoverageAction> {
    match verdict {
        RuntimeCoverageVerdict::SafeToDelete => vec![RuntimeCoverageAction {
            kind: "delete-cold-code".to_owned(),
            description: callback_callee.map_or_else(
                || "Remove cold code after confirming ownership.".to_owned(),
                |callee| {
                    format!(
                        "Callback passed to {callee}; remove the cold code after confirming ownership."
                    )
                },
            ),
            auto_fixable: false,
        }],
        RuntimeCoverageVerdict::ReviewRequired if test_only_reference => {
            vec![RuntimeCoverageAction {
                kind: "review-runtime".to_owned(),
                description: "Only tests reference this export; delete the test usage together with the function or keep it."
                    .to_owned(),
                auto_fixable: false,
            }]
        }
        RuntimeCoverageVerdict::ReviewRequired => vec![RuntimeCoverageAction {
            kind: "review-runtime".to_owned(),
            description: callback_callee.map_or_else(
                || "Review runtime-cold code before changing it.".to_owned(),
                |callee| {
                    format!("Callback passed to {callee}; review the runtime-cold code before changing it.")
                },
            ),
            auto_fixable: false,
        }],
        RuntimeCoverageVerdict::CoverageUnavailable
        | RuntimeCoverageVerdict::LowTraffic
        | RuntimeCoverageVerdict::Active
        | RuntimeCoverageVerdict::Unknown => Vec::new(),
    }
}

const fn runtime_verdict_rank(verdict: RuntimeCoverageVerdict) -> u8 {
    match verdict {
        RuntimeCoverageVerdict::SafeToDelete => 0,
        RuntimeCoverageVerdict::ReviewRequired => 1,
        RuntimeCoverageVerdict::CoverageUnavailable => 2,
        RuntimeCoverageVerdict::LowTraffic => 3,
        RuntimeCoverageVerdict::Unknown => 4,
        RuntimeCoverageVerdict::Active => 5,
    }
}

fn stable_runtime_id(prefix: &str, path: &Path, function: &str, line: u32) -> String {
    let file = normalize_runtime_path(path);
    match prefix {
        "hot" => fallow_cov_protocol::hot_path_id(&file, function, line),
        "blast" => fallow_cov_protocol::blast_radius_id(&file, function, line),
        "importance" => fallow_cov_protocol::importance_id(&file, function, line),
        _ => fallow_cov_protocol::finding_id(&file, function, line),
    }
}

fn print_runtime_report(
    report: &RuntimeCoverageReport,
    ctx: &RunContext<'_>,
    elapsed: std::time::Duration,
    args: &AnalyzeArgs,
) -> ExitCode {
    match ctx.output {
        OutputFormat::Human => print_runtime_human(report, elapsed, args, ctx.root),
        _ => print_runtime_json(report, elapsed, ctx.explain, ctx.json_style),
    }
}

fn apply_top_limit(report: &mut RuntimeCoverageReport, top: Option<usize>) {
    let Some(top) = top else {
        return;
    };
    report.findings.truncate(top);
    report.hot_paths.truncate(top);
    report.blast_radius.truncate(top);
    report.importance.truncate(top);
}

fn print_runtime_json(
    report: &RuntimeCoverageReport,
    elapsed: std::time::Duration,
    explain: bool,
    json_style: crate::json_style::JsonStyle,
) -> ExitCode {
    let output = match runtime_json_output(report, elapsed, explain) {
        Ok(value) => value,
        Err(err) => {
            eprintln!("Error: failed to serialize runtime coverage report: {err}");
            return ExitCode::from(2);
        }
    };
    crate::report::emit_report_json(&output, "runtime coverage JSON", json_style)
}

fn runtime_json_output(
    report: &RuntimeCoverageReport,
    elapsed: std::time::Duration,
    explain: bool,
) -> Result<serde_json::Value, serde_json::Error> {
    debug_assert_eq!(
        RUNTIME_COVERAGE_SCHEMA_VERSION, "1",
        "the schema-version enum has one variant serialized as \"1\"; bump CoverageAnalyzeSchemaVersion if the constant moves"
    );

    let envelope =
        fallow_output::build_coverage_analyze_output(report, elapsed, env!("CARGO_PKG_VERSION"));
    fallow_output::serialize_coverage_analyze_json_output(
        envelope,
        explain.then(crate::explain::coverage_analyze_meta),
        crate::output_runtime::telemetry_analysis_run_id().as_deref(),
    )
}

const HUMAN_DEFAULT_DISPLAY_LIMIT: usize = 10;

/// Build-output directories where bundlers emit `*.map` files, checked in order
/// so the upload nudge can name the dir the user most likely needs.
const SOURCE_MAP_BUILD_DIRS: &[&str] = &["dist", ".next", "out", "build"];

/// Max recursion depth for the build-dir source-map scan: deep enough to reach
/// `.next/static/chunks` / `dist/assets` without walking an entire tree.
const SOURCE_MAP_SCAN_MAX_DEPTH: usize = 6;

/// First build directory under `root` that contains at least one `.map` file, or
/// `None`. A bounded, early-returning scan used only to name `--dir` in the
/// upload nudge, never an exhaustive walk.
fn find_local_source_map_dir(root: &Path) -> Option<&'static str> {
    SOURCE_MAP_BUILD_DIRS.iter().copied().find(|dir| {
        let candidate = root.join(dir);
        candidate.is_dir() && dir_contains_source_map(&candidate, SOURCE_MAP_SCAN_MAX_DEPTH)
    })
}

/// Whether `dir` (or a subdirectory within `depth` levels) holds a `.map` file.
/// Skips `node_modules` and stops at the first hit.
fn dir_contains_source_map(dir: &Path, depth: usize) -> bool {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return false;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_dir() {
            if depth > 0
                && path.file_name().is_none_or(|name| name != "node_modules")
                && dir_contains_source_map(&path, depth - 1)
            {
                return true;
            }
        } else if path
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("map"))
        {
            return true;
        }
    }
    false
}

/// Copy-paste upload hint for the human report. Returned only when the cloud
/// reported `coverage_unresolved` (runtime positions could not map to source)
/// AND the project has built source maps on disk, so the hint can name the exact
/// `--dir`. Re-running the upload fixes both the never-uploaded and the stale-SHA
/// cases (it uploads maps for the current commit), so one hint covers both. The
/// hint is human-only: JSON consumers already get the structured
/// `coverage_unresolved` warning in `report.warnings`.
fn source_map_upload_hint(warnings: &[RuntimeCoverageMessage], root: &Path) -> Option<String> {
    if !warnings
        .iter()
        .any(|warning| warning.code.as_str() == "coverage_unresolved")
    {
        return None;
    }
    let dir = find_local_source_map_dir(root)?;
    Some(format!(
        "Hint: found source maps under {dir}/ that may not be uploaded for this commit.\n  Run `fallow coverage upload-source-maps --dir {dir}` so runtime coverage attributes to your source files."
    ))
}

fn print_runtime_human(
    report: &RuntimeCoverageReport,
    elapsed: std::time::Duration,
    args: &AnalyzeArgs,
    root: &Path,
) -> ExitCode {
    let display_limit = args.top.unwrap_or(HUMAN_DEFAULT_DISPLAY_LIMIT);
    println!("Runtime coverage: {}", report.verdict);
    println!(
        "  {} tracked, {} hit, {} unhit, {} untracked ({:.1}% covered)",
        report.summary.functions_tracked,
        report.summary.functions_hit,
        report.summary.functions_unhit,
        report.summary.functions_untracked,
        report.summary.coverage_percent,
    );
    println!(
        "  based on {} traces over {} days ({} deployments)",
        report.summary.trace_count, report.summary.period_days, report.summary.deployments_seen
    );
    for finding in report.findings.iter().take(display_limit) {
        println!("{}", human_finding_line(finding));
    }
    if args.blast_radius {
        print_runtime_blast_radius(report, display_limit);
    }
    if args.importance {
        print_runtime_importance(report, display_limit);
    }
    for warning in &report.warnings {
        println!("  warning [{}]: {}", warning.code, warning.message);
    }
    if let Some(hint) = source_map_upload_hint(&report.warnings, root) {
        println!("{hint}");
    }
    eprintln!("runtime coverage analyzed in {:.2}s", elapsed.as_secs_f64());
    ExitCode::SUCCESS
}

/// One human-output finding row. A test-only reference is called out inline
/// because the verdict alone ("review required") does not say why the function
/// looks dead, and that is exactly the case where deleting it breaks the suite.
fn human_finding_line(finding: &RuntimeCoverageFinding) -> String {
    format!(
        "  {}:{} {} [{}, {}{}]",
        finding.path.display(),
        finding.line,
        finding.function,
        finding.invocations.map_or_else(
            || "untracked".to_owned(),
            |hits| format!("{hits} invocations")
        ),
        finding.verdict.human_label(),
        if finding.evidence.test_only_reference == Some(true) {
            ", referenced only from tests"
        } else {
            ""
        },
    )
}

/// Print the human-format blast-radius section, capped at `display_limit`.
fn print_runtime_blast_radius(report: &RuntimeCoverageReport, display_limit: usize) {
    if report.blast_radius.is_empty() {
        return;
    }
    println!("  blast radius:");
    for entry in report.blast_radius.iter().take(display_limit) {
        println!(
            "  {}:{} {} ({} callers, weighted {}, {})",
            entry.file.display(),
            entry.line,
            entry.function,
            entry.caller_count,
            entry.caller_count_weighted_by_traffic,
            entry.risk_band,
        );
    }
}

/// Print the human-format importance section, capped at `display_limit`.
fn print_runtime_importance(report: &RuntimeCoverageReport, display_limit: usize) {
    if report.importance.is_empty() {
        return;
    }
    println!("  importance:");
    for entry in report.importance.iter().take(display_limit) {
        println!(
            "  {}:{} {} ({:.1}, {} invocations, cyclomatic {}, owners {}) - {}",
            entry.file.display(),
            entry.line,
            entry.function,
            entry.importance_score,
            entry.invocations,
            entry.cyclomatic,
            entry.owner_count,
            entry.reason,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fallow_output::{
        RuntimeCoverageBlastRadiusEntry, RuntimeCoverageCostBasis, RuntimeCoverageImportanceEntry,
    };

    #[test]
    fn api_key_alone_does_not_enable_cloud_source() {
        let args = AnalyzeArgs::default();
        assert!(!args.cloud);
        assert!(args.runtime_coverage.is_none());
    }

    #[test]
    fn analyze_args_debug_masks_api_key() {
        let args = AnalyzeArgs {
            cloud: true,
            api_key: Some("fallow_live_secret_token_value".to_owned()),
            api_endpoint: Some("https://api.fallow.cloud".to_owned()),
            repo: Some("acme/web".to_owned()),
            ..AnalyzeArgs::default()
        };
        let formatted = format!("{args:?}");
        assert!(
            !formatted.contains("fallow_live_secret_token_value"),
            "api_key leaked through Debug: {formatted}"
        );
        assert!(
            formatted.contains("api_key: Some(\"***\")"),
            "expected explicit redaction marker, got: {formatted}"
        );
        assert!(formatted.contains("repo: Some(\"acme/web\")"));
        assert!(format!("{:?}", AnalyzeArgs::default()).contains("api_key: None"));
    }

    #[test]
    fn analyze_args_debug_includes_non_secret_options() {
        let args = AnalyzeArgs {
            runtime_coverage: Some(PathBuf::from("coverage-final.json")),
            cloud: true,
            api_key: Some("fallow_live_secret_token_value".to_owned()),
            api_endpoint: Some("https://api.example.test".to_owned()),
            repo: Some("acme/web".to_owned()),
            project_id: Some("apps/web".to_owned()),
            coverage_period: 14,
            environment: Some("production".to_owned()),
            commit_sha: Some("abc123".to_owned()),
            production: true,
            min_invocations_hot: 250,
            min_observation_volume: Some(50),
            low_traffic_threshold: Some(0.25),
            top: Some(5),
            blast_radius: true,
            importance: true,
            debug_unmatched: true,
        };

        let formatted = format!("{args:?}");

        assert!(!formatted.contains("fallow_live_secret_token_value"));
        for expected in [
            "runtime_coverage: Some(\"coverage-final.json\")",
            "cloud: true",
            "api_endpoint: Some(\"https://api.example.test\")",
            "repo: Some(\"acme/web\")",
            "project_id: Some(\"apps/web\")",
            "coverage_period: 14",
            "environment: Some(\"production\")",
            "commit_sha: Some(\"abc123\")",
            "production: true",
            "min_invocations_hot: 250",
            "min_observation_volume: Some(50)",
            "low_traffic_threshold: Some(0.25)",
            "top: Some(5)",
            "blast_radius: true",
            "importance: true",
            "debug_unmatched: true",
        ] {
            assert!(
                formatted.contains(expected),
                "missing {expected:?} in {formatted}"
            );
        }
    }

    #[test]
    fn validate_output_format_accepts_only_json_and_human() {
        assert!(validate_output_format(OutputFormat::Json).is_ok());
        assert!(validate_output_format(OutputFormat::Human).is_ok());

        let error = validate_output_format(OutputFormat::Sarif)
            .expect_err("sarif should be rejected for coverage analyze");
        assert!(error.contains("only supports --format json or --format human"));
        assert!(error.contains("Sarif"));
    }

    #[test]
    fn resolve_api_key_prefers_trimmed_explicit_value() {
        assert_eq!(
            resolve_api_key(Some("  fallow_live_token  ")).expect("explicit key should resolve"),
            "fallow_live_token"
        );
    }

    #[test]
    fn resolve_repo_prefers_trimmed_explicit_value() {
        let dir = tempfile::TempDir::new().expect("temp dir should be created");

        assert_eq!(
            resolve_repo(Some("  fallow-rs/fallow  "), dir.path())
                .expect("explicit repo should resolve"),
            "fallow-rs/fallow"
        );
    }

    #[test]
    fn resolve_repo_infers_origin_remote() {
        let dir = tempfile::TempDir::new().expect("temp dir should be created");
        let init = fallow_engine::changed_files::clear_ambient_git_env(&mut Command::new("git"))
            .args(["init", "-b", "main"])
            .current_dir(dir.path())
            .output()
            .expect("git init should run");
        assert!(init.status.success());
        let remote = fallow_engine::changed_files::clear_ambient_git_env(&mut Command::new("git"))
            .args([
                "remote",
                "add",
                "origin",
                "git@github.com:fallow-rs/fallow.git",
            ])
            .current_dir(dir.path())
            .output()
            .expect("git remote add should run");
        assert!(remote.status.success());

        assert_eq!(
            resolve_repo(None, dir.path()).expect("repo should resolve from origin"),
            "fallow-rs/fallow"
        );
    }

    #[test]
    fn cloud_never_called_static_unused_becomes_safe_to_delete() {
        let mut static_index = StaticIndex::default();
        let info = StaticFunctionInfo {
            path: PathBuf::from("src/a.ts"),
            name: "oldFlow".to_owned(),
            start_line: 10,
            end_line: 20,
            static_used: false,
            test_only_reference: None,
            test_covered: false,
            cyclomatic: 4,
            cost: None,
            caller_count: 0,
            owner_count: None,
            stable_id: function_identity_id("src/a.ts", "oldFlow", 10),
            source_hash: None,
            is_callback: false,
        };
        index_static_function(&mut static_index, "src/a.ts", info);
        let mut snapshot = cloud_context(1, 0);
        snapshot.summary.trace_count = 100;
        snapshot.summary.deployments_seen = 2;
        snapshot.summary.functions_hit = 0;
        snapshot.summary.functions_unhit = 1;
        snapshot.summary.coverage_percent = 0.0;
        snapshot.summary.last_received_at = Some("2026-04-30T10:00:00.000Z".to_owned());
        let mut matched = cloud_function("src/a.ts", "oldFlow", Some(10), Some(10), Some(20));
        matched.never_called_source = CloudNeverCalledSource::RuntimeObserved;
        matched.deployments_observed = 2;
        let mut unmatched =
            cloud_function("src/missing.ts", "missingInAst", Some(1), Some(1), Some(3));
        unmatched.deployments_observed = 2;
        snapshot.functions = vec![matched, unmatched];
        let report = merge_cloud_snapshot(&snapshot, &static_index, 100).report;
        assert_eq!(report.findings.len(), 1);
        assert_eq!(
            report.findings[0].verdict,
            RuntimeCoverageVerdict::SafeToDelete
        );
        assert_eq!(report.summary.data_source, RuntimeCoverageDataSource::Cloud);
        assert_eq!(
            report.summary.last_received_at.as_deref(),
            Some("2026-04-30T10:00:00.000Z")
        );
        assert_eq!(
            report
                .summary
                .capture_quality
                .as_ref()
                .map(|quality| quality.instances_observed),
            Some(2)
        );
        assert_eq!(report.findings[0].evidence.test_coverage, "not_covered");
        assert_eq!(report.findings[0].evidence.v8_tracking, "tracked");
        assert_eq!(
            report.findings[0].actions.first().map(|a| a.kind.as_str()),
            Some("delete-cold-code")
        );
        assert_eq!(
            report.warnings.first().map(|warning| warning.code.as_str()),
            Some("cloud_functions_unmatched")
        );
    }

    #[test]
    fn cloud_called_function_emits_hot_path_blast_radius_and_importance() {
        let info = StaticFunctionInfo {
            caller_count: 8,
            cyclomatic: 12,
            cost: Some(StaticCost {
                cognitive: 9,
                cyclomatic: 12,
                line_count: 12,
            }),
            owner_count: Some(1),
            ..static_info("src/api.ts", "handler", 10, 22)
        };
        let static_index = static_index_with(vec![info]);
        let mut function = cloud_function("src/api.ts", "handler", Some(10), Some(10), Some(22));
        function.tracking_state = CloudTrackingState::Called;
        function.hit_count = Some(20_000);
        function.deployments_observed = 4;
        let snapshot = CloudRuntimeContext {
            repo: "acme/web".to_owned(),
            actionable: None,
            actionability_reason: None,
            verdict: None,
            provenance: None,
            window: crate::coverage::cloud_client::CloudRuntimeWindow { period_days: 14 },
            summary: crate::coverage::cloud_client::CloudRuntimeSummary {
                trace_count: 10,
                deployments_seen: 4,
                functions_tracked: 1,
                functions_hit: 1,
                functions_unhit: 0,
                functions_untracked: 0,
                coverage_percent: 100.0,
                last_received_at: None,
            },
            blast_radius: vec![],
            importance: vec![],
            functions: vec![function],
            warnings: vec![],
        };

        let report = merge_cloud_snapshot(&snapshot, &static_index, 100).report;

        assert_eq!(report.verdict, RuntimeCoverageReportVerdict::Clean);
        assert!(report.findings.is_empty());
        assert_eq!(report.hot_paths[0].function, "handler");
        assert_eq!(report.hot_paths[0].invocations, 20_000);
        let target = report.hot_paths[0]
            .optimization_target
            .as_ref()
            .expect("cloud hot path carries an optimization target");
        assert_eq!(target.cost_basis, RuntimeCoverageCostBasis::Cognitive);
        assert_eq!(target.cost_score, 180_000);
        assert_eq!(target.inner_iterations_per_call, None);
        assert_eq!(report.blast_radius[0].caller_count, 8);
        assert_eq!(
            report.blast_radius[0].risk_band,
            RuntimeCoverageRiskBand::Medium
        );
        assert_eq!(report.importance[0].function, "handler");
        assert!(
            report.importance[0]
                .reason
                .contains("Moderate traffic, high complexity, single owner")
        );
        assert!(report.summary.capture_quality.is_some());
    }

    #[test]
    fn cloud_match_rejects_same_name_when_line_does_not_match() {
        let static_index = static_index_with(vec![
            static_info("src/api.ts", "handler", 10, 20),
            static_info("src/api.ts", "handler", 80, 90),
        ]);
        let function = cloud_function("src/api.ts", "handler", Some(40), Some(40), Some(50));

        assert!(match_cloud_function(&function, &static_index).is_none());
    }

    #[test]
    fn cloud_match_allows_small_line_drift() {
        let static_index = static_index_with(vec![static_info("src/api.ts", "handler", 10, 20)]);
        let function = cloud_function("src/api.ts", "handler", Some(12), Some(12), Some(22));

        let matched = match_cloud_function(&function, &static_index).expect("nearby line matches");
        assert_eq!(matched.start_line, 10);
        assert_eq!(matched.end_line, 20);
    }

    #[test]
    fn cloud_match_requires_line_data_for_fuzzy_match() {
        let static_index = static_index_with(vec![static_info("src/api.ts", "handler", 10, 20)]);
        let function = cloud_function("src/api.ts", "handler", None, None, Some(20));

        assert!(match_cloud_function(&function, &static_index).is_none());
    }

    #[test]
    fn cloud_match_rejects_ambiguous_fuzzy_match() {
        let static_index = static_index_with(vec![
            static_info("src/api.ts", "handler", 10, 20),
            static_info("src/api.ts", "handler", 14, 20),
        ]);
        let function = cloud_function("src/api.ts", "handler", Some(12), Some(12), Some(20));

        assert!(match_cloud_function(&function, &static_index).is_none());
    }

    /// Fixture project mirroring the shapes a containerized service reports:
    /// a top-level arrow, an object-literal method, an accessor, and two
    /// anonymous callbacks that runtime instrumentation names after the callee
    /// they were passed to.
    const CLOUD_FIXTURE_SOURCE: &str = r"export const createClient = (url: string) => {
  const rows = [1, 2, 3];
  return {
    execute: async (sql: string) => {
      return rows.map((row) => row + sql.length);
    },
    get closed() {
      return url.length === 0;
    },
  };
};

export const register = (app: { get: (path: string, handler: () => string) => void }) => {
  app.get('/health', () => {
    return 'ok';
  });
};
";

    fn fixture_static_index(source: &str) -> (tempfile::TempDir, StaticIndex) {
        fixture_static_index_at("src/db/file-client.ts", source)
    }

    fn fixture_static_index_at(
        relative_path: &str,
        source: &str,
    ) -> (tempfile::TempDir, StaticIndex) {
        let dir = tempfile::TempDir::new().expect("temp dir should be created");
        let file = dir.path().join(relative_path);
        std::fs::create_dir_all(file.parent().expect("fixture path should have a parent"))
            .expect("fixture directory should be created");
        std::fs::write(&file, source).expect("fixture source should be written");
        std::fs::write(
            dir.path().join("package.json"),
            r#"{"name":"fixture","version":"0.0.0","type":"module"}"#,
        )
        .expect("package.json should be written");
        let config_path = None;
        let ctx = RunContext {
            root: dir.path(),
            config_path: &config_path,
            output: OutputFormat::Json,
            json_style: crate::json_style::JsonStyle::Compact,
            quiet: true,
            no_cache: true,
            threads: 1,
            explain: false,
            allow_remote_extends: false,
        };
        let index = build_static_index(&ctx, false).expect("static index should build");
        (dir, index)
    }

    /// A module with three exports: one only a test file references, one the
    /// production entry point references, and one nothing references at all.
    const TEST_ONLY_EXPORT_SOURCE: &str = r"export const resetForTests = () => {
  return 'reset';
};

export const handler = (input: string) => {
  return input.trim();
};

export const orphan = () => {
  return 'orphan';
};
";

    /// Build a static index for a project whose only reference to
    /// `resetForTests` comes from a `*.test.ts` file, analysed with the
    /// production filter on (the mode that drops that test file).
    fn test_only_export_index() -> (tempfile::TempDir, StaticIndex) {
        let dir = tempfile::TempDir::new().expect("temp dir should be created");
        std::fs::create_dir_all(dir.path().join("src")).expect("src should be created");
        std::fs::write(dir.path().join("src/helpers.ts"), TEST_ONLY_EXPORT_SOURCE)
            .expect("helpers source should be written");
        std::fs::write(
            dir.path().join("src/helpers.test.ts"),
            "import { resetForTests } from './helpers';\n\nresetForTests();\n",
        )
        .expect("test source should be written");
        std::fs::write(
            dir.path().join("src/index.ts"),
            "import { handler } from './helpers';\n\nexport const main = (input: string) => handler(input);\n",
        )
        .expect("entry source should be written");
        std::fs::write(
            dir.path().join("package.json"),
            r#"{"name":"fixture","version":"0.0.0","type":"module","main":"src/index.ts"}"#,
        )
        .expect("package.json should be written");
        let config_path = None;
        let ctx = RunContext {
            root: dir.path(),
            config_path: &config_path,
            output: OutputFormat::Json,
            json_style: crate::json_style::JsonStyle::Compact,
            quiet: true,
            no_cache: true,
            threads: 1,
            explain: false,
            allow_remote_extends: false,
        };
        let index = build_static_index(&ctx, true).expect("static index should build");
        (dir, index)
    }

    fn never_called_cloud_function(
        name: &str,
        start_line: u32,
        end_line: u32,
    ) -> CloudRuntimeFunction {
        let mut function = cloud_function(
            "src/helpers.ts",
            name,
            Some(start_line),
            Some(start_line),
            Some(end_line),
        );
        function.tracking_state = CloudTrackingState::NeverCalled;
        function.never_called_source = CloudNeverCalledSource::RuntimeObserved;
        function.hit_count = Some(0);
        function
    }

    #[test]
    fn production_test_only_export_never_reaches_safe_to_delete() {
        let (_dir, static_index) = test_only_export_index();
        let mut snapshot = cloud_context(2, 0);
        snapshot.functions = vec![
            never_called_cloud_function("resetForTests", 1, 3),
            never_called_cloud_function("orphan", 9, 11),
        ];

        let CloudMergeOutput { report, .. } = merge_cloud_snapshot(&snapshot, &static_index, 100);

        let reset = report
            .findings
            .iter()
            .find(|finding| finding.function == "resetForTests")
            .expect("the test-only export must produce a finding");
        assert_eq!(
            reset.verdict,
            RuntimeCoverageVerdict::ReviewRequired,
            "an export a test file references must never read as safe to delete"
        );
        assert_eq!(reset.evidence.test_only_reference, Some(true));
        assert_eq!(reset.evidence.static_status, "unused");
        assert_eq!(
            reset
                .actions
                .first()
                .map(|action| action.description.as_str()),
            Some(
                "Only tests reference this export; delete the test usage together with the function or keep it."
            )
        );

        let orphan = report
            .findings
            .iter()
            .find(|finding| finding.function == "orphan")
            .expect("the unreferenced export must produce a finding");
        assert_eq!(
            orphan.verdict,
            RuntimeCoverageVerdict::SafeToDelete,
            "an export nothing references at all stays deletable"
        );
        assert_eq!(orphan.evidence.test_only_reference, Some(false));

        let reset_line = human_finding_line(reset);
        assert!(
            reset_line.ends_with(", referenced only from tests]"),
            "{reset_line}"
        );
        assert!(!human_finding_line(orphan).contains("referenced only from tests"));

        let json = serde_json::to_value(&reset.evidence).expect("evidence should serialize");
        assert_eq!(
            json.get("test_only_reference"),
            Some(&serde_json::json!(true))
        );
    }

    fn prefixed_cloud_function(
        name: &str,
        start_line: u32,
        end_line: u32,
        hits: u64,
    ) -> CloudRuntimeFunction {
        let mut function = cloud_function(
            "/app/src/db/file-client.ts",
            name,
            Some(start_line),
            Some(start_line),
            Some(end_line),
        );
        function.tracking_state = CloudTrackingState::Called;
        function.hit_count = Some(hits);
        function
    }

    #[test]
    fn cloud_functions_match_through_container_prefix_and_runtime_names() {
        let (_dir, static_index) = fixture_static_index(CLOUD_FIXTURE_SOURCE);
        let functions = vec![
            prefixed_cloud_function("createClient", 1, 11, 611),
            prefixed_cloud_function("execute", 4, 6, 100_000),
            prefixed_cloud_function("map", 5, 5, 833_000_000),
            prefixed_cloud_function("get closed", 7, 9, 2),
            prefixed_cloud_function("register", 13, 17, 400),
            prefixed_cloud_function("get", 14, 16, 379_000),
        ];
        let mut snapshot = cloud_context(functions.len(), 0);
        snapshot.functions = functions;

        let CloudMergeOutput { report, unmatched } =
            merge_cloud_snapshot(&snapshot, &static_index, 100);

        assert!(
            unmatched.is_empty(),
            "every fixture function has a local counterpart, got {unmatched:?}"
        );
        assert!(
            !report
                .warnings
                .iter()
                .any(|warning| warning.code == "cloud_functions_unmatched")
        );
        assert_eq!(
            report.hot_paths.first().map(|hot| hot.invocations),
            Some(833_000_000),
            "the busiest runtime function must lead the hot paths"
        );
        for hot_path in &report.hot_paths {
            assert_eq!(hot_path.path, PathBuf::from("src/db/file-client.ts"));
        }
        let mut hot_lines: Vec<u32> = report.hot_paths.iter().map(|hot| hot.line).collect();
        hot_lines.sort_unstable();
        assert_eq!(
            hot_lines,
            vec![1, 4, 5, 13, 14],
            "the callback and object-method functions must reach the hot paths"
        );
    }

    /// Fixture project carrying the shapes the health/complexity pass does not
    /// enumerate: a Drizzle-style schema whose table and column callbacks are
    /// anonymous arrows, an object returned from a factory with methods, a
    /// getter, and a `.map(...)` chain. Runtime instrumentation names each of
    /// them after the callee it was passed to or after the property key.
    const SCHEMA_FIXTURE_SOURCE: &str = r"declare const sqliteTable: (
  name: string,
  columns: Record<string, unknown>,
  extra?: (table: Record<string, unknown>) => unknown[],
) => Record<string, unknown>;
declare const text: (name: string) => {
  primaryKey: () => unknown;
  references: (target: () => unknown) => unknown;
};
declare const index: (name: string) => { on: (column: unknown) => unknown };

export const orgs = sqliteTable('orgs', {
  id: text('id').primaryKey(),
});

export const users = sqliteTable(
  'users',
  {
    id: text('id').primaryKey(),
    orgId: text('org_id').references(() => orgs.id),
  },
  (table) => [index('users_org_idx').on(table.orgId)],
);

export const createStore = (rows: string[]) => {
  return {
    execute: async (sql: string) => {
      return rows.map((row) => row + sql.length);
    },
    rollback: () => rows.length,
    get closed() {
      return rows.length === 0;
    },
  };
};
";

    /// A cloud row for `SCHEMA_FIXTURE_SOURCE`, carrying the container-prefixed
    /// runtime path the service reports and the `stable_id` the cloud hashes
    /// over the repo-relative path, so the join has to land on the stable-id
    /// tier rather than on a positional fallback.
    fn schema_cloud_function(
        name: &str,
        start_line: u32,
        end_line: u32,
        hits: u64,
    ) -> CloudRuntimeFunction {
        let mut function = cloud_function(
            "/app/src/db/schema.ts",
            name,
            Some(start_line),
            Some(start_line),
            Some(end_line),
        );
        function.stable_id = Some(function_identity_id("src/db/schema.ts", name, start_line));
        function.tracking_state = CloudTrackingState::Called;
        function.hit_count = Some(hits);
        function
    }

    #[test]
    fn instrumenter_named_callbacks_and_members_match_on_stable_id() {
        let (_dir, static_index) =
            fixture_static_index_at("src/db/schema.ts", SCHEMA_FIXTURE_SOURCE);
        let functions = vec![
            schema_cloud_function("references", 20, 20, 27),
            schema_cloud_function("sqliteTable", 22, 22, 25),
            schema_cloud_function("createStore", 25, 35, 611),
            schema_cloud_function("execute", 27, 29, 100_000),
            schema_cloud_function("map", 28, 28, 21_200_000),
            schema_cloud_function("rollback", 30, 30, 33),
            schema_cloud_function("get closed", 31, 33, 36_800_000),
        ];
        let identities: Vec<(String, String)> = functions
            .iter()
            .map(|function| {
                (
                    function.function_name.clone(),
                    function
                        .stable_id
                        .clone()
                        .expect("the fixture sets a stable id"),
                )
            })
            .collect();
        let mut snapshot = cloud_context(functions.len(), 0);
        snapshot.functions = functions;

        let CloudMergeOutput { report, unmatched } =
            merge_cloud_snapshot(&snapshot, &static_index, 1);

        assert!(
            unmatched.is_empty(),
            "every instrumenter-named function has a local counterpart, got {unmatched:?}"
        );
        for (name, stable_id) in &identities {
            let indexed = static_index.by_stable_id.get(stable_id);
            assert_eq!(
                indexed.map(|info| info.name.as_str()),
                Some(name.as_str()),
                "{name} must join on the stable-id tier, not on a positional fallback"
            );
        }
        let mut hot: Vec<(String, u64)> = report
            .hot_paths
            .iter()
            .map(|path| (path.function.clone(), path.invocations))
            .collect();
        hot.sort_by_key(|(_, invocations)| std::cmp::Reverse(*invocations));
        assert_eq!(
            hot.first().map(|(name, hits)| (name.as_str(), *hits)),
            Some(("get closed", 36_800_000)),
            "the busiest instrumenter-named function must lead the hot paths"
        );
        assert!(
            hot.iter()
                .any(|(name, hits)| name == "map" && *hits == 21_200_000),
            "the map callback must reach the hot paths with its invocation count, got {hot:?}"
        );
        for hot_path in &report.hot_paths {
            assert_eq!(hot_path.path, PathBuf::from("src/db/schema.ts"));
        }
    }

    #[test]
    fn cold_callback_verdict_copy_points_at_the_call_site() {
        let (_dir, static_index) =
            fixture_static_index_at("src/db/schema.ts", SCHEMA_FIXTURE_SOURCE);
        let mut cold = schema_cloud_function("references", 20, 20, 0);
        cold.tracking_state = CloudTrackingState::NeverCalled;
        cold.never_called_source = CloudNeverCalledSource::RuntimeObserved;
        let mut snapshot = cloud_context(1, 0);
        snapshot.summary.trace_count = 100;
        snapshot.functions = vec![cold];

        let CloudMergeOutput { report, unmatched } =
            merge_cloud_snapshot(&snapshot, &static_index, 1);

        assert!(
            unmatched.is_empty(),
            "the callback must match, got {unmatched:?}"
        );
        let finding = report
            .findings
            .iter()
            .find(|finding| finding.function == "references")
            .expect("the cold callback must produce a finding");
        let description = finding
            .actions
            .first()
            .map(|action| action.description.clone())
            .unwrap_or_default();
        assert!(
            description.starts_with("Callback passed to references;"),
            "callback copy must name the call site, got {description:?}"
        );
    }

    #[test]
    fn cloud_function_without_local_counterpart_stays_unmatched() {
        let (_dir, static_index) = fixture_static_index(CLOUD_FIXTURE_SOURCE);
        let mut snapshot = cloud_context(1, 0);
        let mut absent = cloud_function(
            "/app/src/db/gone.ts",
            "removedFlow",
            Some(3),
            Some(3),
            Some(9),
        );
        absent.tracking_state = CloudTrackingState::Called;
        absent.hit_count = Some(12);
        snapshot.functions = vec![absent];

        let CloudMergeOutput { report, unmatched } =
            merge_cloud_snapshot(&snapshot, &static_index, 100);

        assert_eq!(unmatched.len(), 1);
        assert_eq!(unmatched[0].name, "removedFlow");
        assert!(report.hot_paths.is_empty());
        assert!(
            report
                .warnings
                .iter()
                .any(|warning| warning.code == "cloud_functions_unmatched")
        );
    }

    #[test]
    fn resolve_cloud_path_rebases_container_prefix() {
        let static_index =
            static_index_with(vec![static_info("src/db/file-client.ts", "run", 1, 4)]);

        assert_eq!(
            resolve_cloud_path(&static_index, "app/src/db/file-client.ts"),
            Some("src/db/file-client.ts")
        );
        assert_eq!(
            resolve_cloud_path(&static_index, "src/db/file-client.ts"),
            Some("src/db/file-client.ts")
        );
        assert_eq!(resolve_cloud_path(&static_index, "app/other.ts"), None);
    }

    #[test]
    fn resolve_cloud_path_rejects_two_equally_deep_local_files() {
        let static_index = static_index_with(vec![
            static_info("packages/api/src/index.ts", "run", 1, 4),
            static_info("packages/web/src/index.ts", "run", 1, 4),
        ]);

        assert_eq!(resolve_cloud_path(&static_index, "app/src/index.ts"), None);
    }

    #[test]
    fn positional_match_rejects_two_functions_opening_on_one_line() {
        let static_index = static_index_with(vec![
            static_info("src/api.ts", "outer", 10, 14),
            static_info("src/api.ts", "<arrow>", 10, 14),
        ]);
        let function = cloud_function("src/api.ts", "then", Some(10), Some(10), Some(14));

        assert!(match_cloud_function(&function, &static_index).is_none());
    }

    #[test]
    fn positional_match_breaks_a_line_tie_on_the_end_line() {
        let static_index = static_index_with(vec![
            static_info("src/api.ts", "outer", 10, 40),
            static_info("src/api.ts", "<arrow>", 10, 14),
        ]);
        let function = cloud_function("src/api.ts", "then", Some(10), Some(10), Some(14));

        let matched = match_cloud_function(&function, &static_index).expect("end line breaks tie");
        assert_eq!(matched.end_line, 14);
    }

    #[test]
    fn cloud_never_called_static_used_emits_review_runtime_action() {
        let actions = runtime_actions(RuntimeCoverageVerdict::ReviewRequired, false, None);
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].kind, "review-runtime");
    }

    #[test]
    fn cloud_finding_decision_maps_tracking_states() {
        let mut used = static_info("src/api.ts", "handler", 10, 20);
        used.static_used = true;
        let mut unused = used.clone();
        unused.static_used = false;

        let mut never_called =
            cloud_function("src/api.ts", "handler", Some(10), Some(10), Some(20));
        never_called.never_called_source = CloudNeverCalledSource::RuntimeObserved;
        assert_eq!(
            cloud_finding_decision(&never_called, &used),
            (
                RuntimeCoverageVerdict::ReviewRequired,
                RuntimeCoverageConfidence::High,
                Some(0)
            )
        );
        assert_eq!(
            cloud_finding_decision(&never_called, &unused),
            (
                RuntimeCoverageVerdict::SafeToDelete,
                RuntimeCoverageConfidence::High,
                Some(0)
            )
        );

        let mut inventory_backfill = never_called.clone();
        inventory_backfill.never_called_source = CloudNeverCalledSource::InventoryBackfill;
        assert_eq!(
            cloud_finding_decision(&inventory_backfill, &unused),
            (
                RuntimeCoverageVerdict::ReviewRequired,
                RuntimeCoverageConfidence::Low,
                Some(0)
            )
        );

        let mut legacy = never_called.clone();
        legacy.never_called_source = CloudNeverCalledSource::Unknown;
        assert_eq!(
            cloud_finding_decision(&legacy, &unused),
            (
                RuntimeCoverageVerdict::ReviewRequired,
                RuntimeCoverageConfidence::Low,
                Some(0)
            )
        );

        let mut untracked = never_called.clone();
        untracked.tracking_state = CloudTrackingState::Untracked;
        untracked.hit_count = None;
        assert_eq!(
            cloud_finding_decision(&untracked, &used),
            (
                RuntimeCoverageVerdict::CoverageUnavailable,
                RuntimeCoverageConfidence::None,
                None
            )
        );

        let mut unknown = never_called;
        unknown.tracking_state = CloudTrackingState::Unknown;
        unknown.hit_count = Some(42);
        assert_eq!(
            cloud_finding_decision(&unknown, &used),
            (
                RuntimeCoverageVerdict::Unknown,
                RuntimeCoverageConfidence::Low,
                Some(42)
            )
        );
    }

    #[test]
    fn cloud_warnings_dedupe_server_and_cli_no_runtime_data() {
        let snapshot = CloudRuntimeContext {
            repo: "nonexistent-repo".to_owned(),
            actionable: None,
            actionability_reason: None,
            verdict: None,
            provenance: None,
            window: crate::coverage::cloud_client::CloudRuntimeWindow { period_days: 30 },
            summary: crate::coverage::cloud_client::CloudRuntimeSummary {
                trace_count: 0,
                deployments_seen: 0,
                functions_tracked: 0,
                functions_hit: 0,
                functions_unhit: 0,
                functions_untracked: 0,
                coverage_percent: 0.0,
                last_received_at: None,
            },
            blast_radius: vec![],
            importance: vec![],
            functions: vec![],
            warnings: vec![CloudRuntimeWarning::Object {
                code: Some("no_runtime_data".to_owned()),
                message: Some(
                    "No runtime coverage data received for nonexistent-repo in the last 30 days."
                        .to_owned(),
                ),
            }],
        };
        let warnings = cloud_warnings(&snapshot, 0);
        let no_data_count = warnings
            .iter()
            .filter(|w| w.code == "no_runtime_data")
            .count();
        assert_eq!(
            no_data_count, 1,
            "expected exactly one no_runtime_data warning, got: {warnings:?}"
        );
    }

    #[test]
    fn cloud_warnings_dedupe_when_server_message_includes_project_id() {
        let snapshot = CloudRuntimeContext {
            repo: "fallow-cloud".to_owned(),
            actionable: None,
            actionability_reason: None,
            verdict: None,
            provenance: None,
            window: crate::coverage::cloud_client::CloudRuntimeWindow { period_days: 30 },
            summary: crate::coverage::cloud_client::CloudRuntimeSummary {
                trace_count: 0,
                deployments_seen: 0,
                functions_tracked: 0,
                functions_hit: 0,
                functions_unhit: 0,
                functions_untracked: 0,
                coverage_percent: 0.0,
                last_received_at: None,
            },
            blast_radius: vec![],
            importance: vec![],
            functions: vec![],
            warnings: vec![CloudRuntimeWarning::Object {
                code: Some("no_runtime_data".to_owned()),
                message: Some(
                    "No runtime coverage data received for apps/dashboard in fallow-cloud in the last 30 days.".to_owned(),
                ),
            }],
        };
        let warnings = cloud_warnings(&snapshot, 0);
        let no_data_count = warnings
            .iter()
            .filter(|w| w.code == "no_runtime_data")
            .count();
        assert_eq!(
            no_data_count, 1,
            "expected exactly one no_runtime_data warning, got: {warnings:?}"
        );
    }

    #[test]
    fn cloud_capture_quality_reports_untracked_ratio_only_when_data_exists() {
        let mut snapshot = CloudRuntimeContext {
            repo: "acme/web".to_owned(),
            actionable: None,
            actionability_reason: None,
            verdict: None,
            provenance: None,
            window: crate::coverage::cloud_client::CloudRuntimeWindow { period_days: 7 },
            summary: crate::coverage::cloud_client::CloudRuntimeSummary {
                trace_count: 0,
                deployments_seen: 0,
                functions_tracked: 0,
                functions_hit: 0,
                functions_unhit: 0,
                functions_untracked: 0,
                coverage_percent: 0.0,
                last_received_at: None,
            },
            blast_radius: vec![],
            importance: vec![],
            functions: vec![],
            warnings: vec![],
        };
        assert!(cloud_capture_quality(&snapshot).is_none());

        snapshot.summary.functions_tracked = 1;
        snapshot.summary.functions_untracked = 3;
        snapshot.summary.deployments_seen = 2;
        let quality = cloud_capture_quality(&snapshot).expect("data should emit quality");

        assert_eq!(quality.window_seconds, 604_800);
        assert_eq!(quality.instances_observed, 2);
        assert!((quality.untracked_ratio_percent - 75.0).abs() < f64::EPSILON);
        assert!(quality.lazy_parse_warning);
    }

    #[test]
    fn cloud_report_preserves_non_actionable_server_verdict_and_provenance() {
        let mut snapshot = cloud_context(1, 0);
        snapshot.actionable = Some(false);
        snapshot.actionability_reason =
            Some("7 of 10,000 required observations collected.".to_owned());
        snapshot.verdict = Some("insufficient_evidence".to_owned());
        snapshot.provenance = Some(CloudRuntimeProvenance {
            is_production: Some(
                crate::coverage::cloud_client::CloudRuntimeProductionStatus::Known(true),
            ),
            freshness_days: Some(3),
            untracked_ratio: Some(0.25),
            unresolved_ratio: Some(0.4),
            stale: Some(false),
            stale_after_days: Some(14),
        });

        let report = merge_cloud_snapshot(&snapshot, &StaticIndex::default(), 100).report;

        assert!(!report.actionable);
        assert_eq!(
            report.actionability_reason.as_deref(),
            Some("7 of 10,000 required observations collected.")
        );
        assert_eq!(
            report.actionability_verdict.as_deref(),
            Some("insufficient_evidence")
        );
        assert_eq!(report.provenance.is_production, "true");
        assert_eq!(report.provenance.freshness_days, Some(3));
        assert!((report.provenance.untracked_ratio - 0.25).abs() < f64::EPSILON);
        assert!((report.provenance.unresolved_ratio - 0.4).abs() < f64::EPSILON);
        assert!(!report.provenance.stale);
        assert_eq!(report.provenance.stale_after_days, 14);
    }

    #[test]
    fn cloud_report_uses_legacy_actionability_fallback_when_fields_are_absent() {
        let report =
            merge_cloud_snapshot(&cloud_context(1, 2), &StaticIndex::default(), 100).report;

        assert!(report.actionable);
        assert_eq!(report.actionability_reason, None);
        assert_eq!(report.actionability_verdict, None);
        assert_eq!(report.provenance.is_production, "unknown");
        assert_eq!(report.provenance.freshness_days, None);
        assert!((report.provenance.untracked_ratio - (2.0 / 3.0)).abs() < f64::EPSILON);
        assert!(report.provenance.unresolved_ratio.abs() < f64::EPSILON);
        assert!(!report.provenance.stale);
        assert_eq!(report.provenance.stale_after_days, RUNTIME_STALE_AFTER_DAYS);
    }

    #[test]
    fn validate_output_format_accepts_json_and_human() {
        assert!(validate_output_format(OutputFormat::Json).is_ok());
        assert!(validate_output_format(OutputFormat::Human).is_ok());
    }

    #[test]
    fn top_limit_truncates_all_runtime_arrays() {
        let mut report = RuntimeCoverageReport {
            schema_version: RuntimeCoverageSchemaVersion::V1,
            verdict: RuntimeCoverageReportVerdict::Clean,
            signals: Vec::new(),
            summary: RuntimeCoverageSummary::default(),
            findings: vec![
                runtime_finding("fallow:prod:00000001"),
                runtime_finding("fallow:prod:00000002"),
            ],
            hot_paths: vec![
                runtime_hot_path("fallow:hot:00000001"),
                runtime_hot_path("fallow:hot:00000002"),
            ],
            blast_radius: vec![
                runtime_blast_radius("fallow:blast:00000001"),
                runtime_blast_radius("fallow:blast:00000002"),
            ],
            importance: vec![
                runtime_importance("fallow:importance:00000001"),
                runtime_importance("fallow:importance:00000002"),
            ],
            watermark: None,
            warnings: vec![],
            actionable: true,
            actionability_reason: None,
            actionability_verdict: None,
            provenance: fallow_output::RuntimeCoverageProvenance::default(),
        };
        apply_top_limit(&mut report, Some(1));
        assert_eq!(report.findings.len(), 1);
        assert_eq!(report.hot_paths.len(), 1);
        assert_eq!(report.blast_radius.len(), 1);
        assert_eq!(report.importance.len(), 1);
    }

    #[test]
    fn cloud_importance_scores_missing_codeowners_lower_than_unowned() {
        let no_codeowners = runtime_importance("fallow:importance:00000001");
        let unowned = RuntimeCoverageImportanceEntry {
            id: "fallow:importance:00000002".to_owned(),
            owner_count: 0,
            reason: "High traffic, low complexity, unowned".to_owned(),
            ..runtime_importance("fallow:importance:00000002")
        };

        let ranked = rank_importance(vec![(no_codeowners, None), (unowned, Some(0))]);
        assert_eq!(ranked[0].id, "fallow:importance:00000002");
        assert!((ranked[0].importance_score - 78.8).abs() < f64::EPSILON);
        assert!((ranked[1].importance_score - 63.0).abs() < f64::EPSILON);
    }

    #[test]
    fn stable_runtime_id_emits_eight_hex_chars() {
        let path = PathBuf::from("src/foo.ts");
        let id = stable_runtime_id("prod", &path, "doThing", 42);
        let suffix = id
            .strip_prefix("fallow:prod:")
            .expect("id has fallow:prod: prefix");
        assert_eq!(suffix.len(), 8, "expected 8 hex chars, got {suffix:?}");
        assert!(
            suffix
                .chars()
                .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()),
            "expected lowercase hex chars, got {suffix:?}"
        );
    }

    #[test]
    fn runtime_helper_tables_cover_actions_ranks_tracking_and_paths() {
        let delete_actions = runtime_actions(RuntimeCoverageVerdict::SafeToDelete, false, None);
        assert_eq!(delete_actions.len(), 1);
        assert_eq!(delete_actions[0].kind, "delete-cold-code");
        assert!(runtime_actions(RuntimeCoverageVerdict::Active, false, None).is_empty());
        assert!(runtime_actions(RuntimeCoverageVerdict::Unknown, false, None).is_empty());

        assert!(
            runtime_verdict_rank(RuntimeCoverageVerdict::SafeToDelete)
                < runtime_verdict_rank(RuntimeCoverageVerdict::ReviewRequired)
        );
        assert!(
            runtime_verdict_rank(RuntimeCoverageVerdict::Unknown)
                < runtime_verdict_rank(RuntimeCoverageVerdict::Active)
        );

        assert_eq!(
            blast_radius_risk_band(25, 10),
            RuntimeCoverageRiskBand::High
        );
        assert_eq!(
            blast_radius_risk_band(5, 10),
            RuntimeCoverageRiskBand::Medium
        );
        assert_eq!(blast_radius_risk_band(1, 10), RuntimeCoverageRiskBand::Low);

        assert_eq!(cloud_v8_tracking(CloudTrackingState::Called), "tracked");
        assert_eq!(
            cloud_v8_tracking(CloudTrackingState::Untracked),
            "untracked"
        );
        assert_eq!(
            normalize_runtime_path(Path::new("/src\\feature\\handler.ts")),
            "src/feature/handler.ts"
        );
    }

    #[test]
    fn validate_output_format_rejects_other_formats() {
        for fmt in [
            OutputFormat::Compact,
            OutputFormat::Markdown,
            OutputFormat::Sarif,
            OutputFormat::CodeClimate,
            OutputFormat::PrCommentGithub,
            OutputFormat::PrCommentGitlab,
            OutputFormat::ReviewGithub,
            OutputFormat::ReviewGitlab,
            OutputFormat::Badge,
            OutputFormat::GithubAnnotations,
            OutputFormat::GithubSummary,
        ] {
            let err = validate_output_format(fmt).expect_err("must reject");
            assert!(
                err.contains("only supports --format json or --format human"),
                "rejection message must guide users; got: {err}"
            );
        }
    }

    fn runtime_finding(id: &str) -> RuntimeCoverageFinding {
        RuntimeCoverageFinding {
            id: id.to_owned(),
            stable_id: None,
            source_hash: None,
            path: PathBuf::from("src/a.ts"),
            function: "a".to_owned(),
            line: 1,
            verdict: RuntimeCoverageVerdict::ReviewRequired,
            invocations: Some(0),
            confidence: RuntimeCoverageConfidence::Medium,
            evidence: RuntimeCoverageEvidence {
                static_status: "used".to_owned(),
                test_coverage: "not_covered".to_owned(),
                test_only_reference: None,
                v8_tracking: "tracked".to_owned(),
                untracked_reason: None,
                observation_days: 0,
                deployments_observed: 0,
            },
            actions: vec![],
            discriminators: None,
        }
    }

    fn static_info(path: &str, name: &str, start_line: u32, end_line: u32) -> StaticFunctionInfo {
        let rel = normalize_runtime_path(Path::new(path));
        StaticFunctionInfo {
            path: PathBuf::from(path),
            name: name.to_owned(),
            start_line,
            end_line,
            static_used: false,
            test_only_reference: None,
            test_covered: false,
            cyclomatic: 1,
            cost: None,
            caller_count: 0,
            owner_count: None,
            stable_id: function_identity_id(&rel, name, start_line),
            source_hash: None,
            is_callback: false,
        }
    }

    fn static_index_with(functions: Vec<StaticFunctionInfo>) -> StaticIndex {
        let mut static_index = StaticIndex::default();
        for function in functions {
            let path = normalize_runtime_path(&function.path);
            index_static_function(&mut static_index, &path, function);
        }
        static_index
    }

    fn cloud_function(
        path: &str,
        name: &str,
        line_number: Option<u32>,
        start_line: Option<u32>,
        end_line: Option<u32>,
    ) -> CloudRuntimeFunction {
        CloudRuntimeFunction {
            file_path: path.to_owned(),
            function_name: name.to_owned(),
            stable_id: None,
            line_number,
            start_line,
            end_line,
            hit_count: Some(0),
            tracking_state: CloudTrackingState::NeverCalled,
            never_called_source: CloudNeverCalledSource::Unknown,
            deployments_observed: 1,
            untracked_reason: None,
        }
    }

    fn cloud_context(functions_tracked: usize, functions_untracked: usize) -> CloudRuntimeContext {
        CloudRuntimeContext {
            repo: "acme/web".to_owned(),
            actionable: None,
            actionability_reason: None,
            verdict: None,
            provenance: None,
            window: crate::coverage::cloud_client::CloudRuntimeWindow { period_days: 30 },
            summary: crate::coverage::cloud_client::CloudRuntimeSummary {
                trace_count: 7,
                deployments_seen: 1,
                functions_tracked,
                functions_hit: functions_tracked,
                functions_unhit: 0,
                functions_untracked,
                coverage_percent: 100.0,
                last_received_at: Some("2026-07-23T08:00:00.000Z".to_owned()),
            },
            functions: vec![],
            blast_radius: vec![],
            importance: vec![],
            warnings: vec![],
        }
    }

    fn runtime_hot_path(id: &str) -> RuntimeCoverageHotPath {
        RuntimeCoverageHotPath {
            id: id.to_owned(),
            stable_id: None,
            path: PathBuf::from("src/a.ts"),
            function: "a".to_owned(),
            line: 1,
            end_line: 4,
            invocations: 1,
            percentile: 100,
            actions: vec![],
            optimization_target: None,
        }
    }

    fn runtime_blast_radius(id: &str) -> RuntimeCoverageBlastRadiusEntry {
        RuntimeCoverageBlastRadiusEntry {
            id: id.to_owned(),
            stable_id: None,
            file: PathBuf::from("src/a.ts"),
            function: "a".to_owned(),
            line: 1,
            caller_count: 1,
            caller_count_weighted_by_traffic: 1,
            deploys_touched: None,
            risk_band: RuntimeCoverageRiskBand::Low,
        }
    }

    fn runtime_importance(id: &str) -> RuntimeCoverageImportanceEntry {
        RuntimeCoverageImportanceEntry {
            id: id.to_owned(),
            stable_id: None,
            file: PathBuf::from("src/a.ts"),
            function: "a".to_owned(),
            line: 1,
            invocations: 1,
            cyclomatic: 1,
            owner_count: 1,
            importance_score: 1.0,
            reason: "Low traffic, low complexity, single owner".to_owned(),
        }
    }

    fn unresolved_warning() -> Vec<RuntimeCoverageMessage> {
        vec![RuntimeCoverageMessage {
            code: "coverage_unresolved".to_owned(),
            message: "100% of runtime functions with attempted source resolution could not be mapped to source. No source maps were uploaded for this commit.".to_owned(),
        }]
    }

    #[test]
    fn upload_hint_absent_without_coverage_unresolved_warning() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("dist")).unwrap();
        std::fs::write(dir.path().join("dist").join("app.js.map"), "{}").unwrap();
        // A different warning code must not trigger the hint even with local maps.
        let warnings = vec![RuntimeCoverageMessage {
            code: "no_runtime_data".to_owned(),
            message: "no data".to_owned(),
        }];
        assert!(source_map_upload_hint(&warnings, dir.path()).is_none());
    }

    #[test]
    fn upload_hint_names_the_build_dir_holding_maps() {
        let dir = tempfile::tempdir().unwrap();
        let chunks = dir.path().join(".next").join("static").join("chunks");
        std::fs::create_dir_all(&chunks).unwrap();
        std::fs::write(chunks.join("main.js.map"), "{}").unwrap();
        let hint = source_map_upload_hint(&unresolved_warning(), dir.path()).expect("hint");
        assert!(
            hint.contains("fallow coverage upload-source-maps --dir .next"),
            "{hint}"
        );
    }

    #[test]
    fn upload_hint_absent_when_unresolved_but_no_local_maps() {
        let dir = tempfile::tempdir().unwrap();
        assert!(source_map_upload_hint(&unresolved_warning(), dir.path()).is_none());
    }

    #[test]
    fn source_map_scan_skips_node_modules() {
        let dir = tempfile::tempdir().unwrap();
        let nested = dir.path().join("dist").join("node_modules").join("pkg");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(nested.join("vendor.js.map"), "{}").unwrap();
        // The only .map lives under node_modules, which the scan skips.
        assert!(source_map_upload_hint(&unresolved_warning(), dir.path()).is_none());
    }
}
