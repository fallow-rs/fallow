//! `fallow security` command: opt-in local security-candidate surface.
//!
//! Ships the graph-structural `client-server-leak` rule plus the data-driven
//! `tainted-sink` catalogue (one `TaintedSink` kind covering every CWE category
//! in `security_matchers.toml`). Findings are CANDIDATES for downstream agent
//! verification, NOT verified vulnerabilities.
//! This command is the ONLY surface for security findings: they never appear
//! under bare `fallow` or the `audit` gate. There is no `confidence` or
//! `signal_strength` field; structural traces and reachability context are the
//! only honest signals.

use crate::report::sink::outln;
use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Instant;

use fallow_config::{OutputFormat, ProductionAnalysis, Severity};
use fallow_core::analyze::derive_security_severity;
use fallow_core::results::{
    AnalysisResults, SecurityAttackSurfaceEntry, SecurityDeadCodeKind, SecurityFinding,
    SecurityFindingKind, TraceHop, TraceHopRole,
};
use fallow_types::discover::DiscoveredFile;
use fallow_types::envelope::{ElapsedMs, Meta, ToolVersion};
use fallow_types::extract::ModuleInfo;
use fallow_types::results::{
    SecurityRuntimeContext, SecurityRuntimeState, SecuritySeverity,
    SecurityUnresolvedCalleeDiagnostic, TaintConfidence,
};
use rustc_hash::FxHashSet;
use serde::Serialize;
use xxhash_rust::xxh3::xxh3_64;

use crate::base_worktree::{BaseWorktree, git_rev_parse};
use crate::error::emit_error;
use crate::health::{HealthOptions, SharedParseData, SortBy};
use crate::health_types::{
    RuntimeCoverageFinding, RuntimeCoverageHotPath, RuntimeCoverageReport, RuntimeCoverageVerdict,
};
use crate::load_config_for_analysis;

const UNRESOLVED_CALLEE_SAMPLE_LIMIT: usize = 25;
const UNRESOLVED_CALLEE_TOP_FILES_LIMIT: usize = 10;

/// The `fallow security --format json` schema version. Independently versioned
/// from the main contract, mirroring `ImpactReportSchemaVersion`.
#[derive(Debug, Clone, Copy, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub enum SecuritySchemaVersion {
    /// First release of the `fallow security --format json` shape.
    #[allow(
        dead_code,
        reason = "kept so the generated schema documents historical v1"
    )]
    #[serde(rename = "1")]
    V1,
    /// Adds per-finding `severity` for verification-priority tiering.
    #[allow(
        dead_code,
        reason = "kept so the generated schema documents historical v2"
    )]
    #[serde(rename = "2")]
    V2,
    /// Adds version, elapsed time, explain metadata, and safe config metadata.
    #[allow(
        dead_code,
        reason = "kept so the generated schema documents historical v3"
    )]
    #[serde(rename = "3")]
    V3,
    /// Adds bounded diagnostics for unresolved callee blind spots.
    #[serde(rename = "4")]
    V4,
}

/// Gate mode for `fallow security --gate <mode>`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, clap::ValueEnum)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum SecurityGateMode {
    /// Fail when the change introduces a NEW security-sink candidate on a changed
    /// line (not merely a sink in a changed file). There is deliberately no `all`
    /// mode: gating on the whole candidate backlog is the anti-feature this gate
    /// exists to avoid.
    New,
    /// Fail when a candidate becomes runtime-reachable from an entry point in
    /// head but the matching candidate was not runtime-reachable in base.
    NewlyReachable,
}

/// Gate verdict on the wire. `fail` is the CI-state token; human output renders
/// it as "REVIEW REQUIRED" because these stay unverified candidates, never
/// confirmed vulnerabilities.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum SecurityGateVerdict {
    /// No new candidate in the changed lines.
    Pass,
    /// At least one new candidate in the changed lines; review required.
    Fail,
}

/// The `gate` block on `SecurityOutput`, present only when `--gate <mode>` ran.
/// Invariant: `verdict == Fail  IFF  exit code 8  IFF  new_count > 0`.
#[derive(Debug, Clone, Copy, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SecurityGate {
    /// Which delta the gate checked.
    pub mode: SecurityGateMode,
    /// `pass` or `fail`.
    pub verdict: SecurityGateVerdict,
    /// Number of candidates matching the selected gate mode.
    pub new_count: usize,
}

/// Allowlisted config context for `fallow security --format json`.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(
    feature = "schema",
    schemars(extend("required" = ["rules", "categories_include", "categories_exclude"]))
)]
pub struct SecurityOutputConfig {
    /// Relevant rule severities before and after this command applies its
    /// default-on behavior for security-only rules.
    pub rules: SecurityOutputRulesConfig,
    /// `security.categories.include` from config. `null` means unset, `[]`
    /// means explicitly empty.
    pub categories_include: Option<Vec<String>>,
    /// `security.categories.exclude` from config. `null` means unset, `[]`
    /// means explicitly empty.
    pub categories_exclude: Option<Vec<String>>,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SecurityOutputRulesConfig {
    pub security_client_server_leak: SecurityRuleSeverityConfig,
    pub security_sink: SecurityRuleSeverityConfig,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SecurityRuleSeverityConfig {
    /// Severity read from resolved config before the security command applies
    /// its default-on behavior.
    pub configured: Severity,
    /// Severity used for this command run.
    pub effective: Severity,
}

/// The `fallow security --format json` envelope. `FallowOutput` discriminates it
/// by the `kind: "security"` tag; the optional `gate` block is additive and is
/// not part of that discrimination.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SecurityOutput {
    /// Schema version of this envelope.
    pub schema_version: SecuritySchemaVersion,
    /// Fallow CLI version that produced this output.
    pub version: ToolVersion,
    /// Wall-clock milliseconds spent producing the report.
    pub elapsed_ms: ElapsedMs,
    /// Privacy-safe config context relevant to security candidate generation.
    pub config: SecurityOutputConfig,
    /// Security-specific rule and field metadata, emitted with `--explain`.
    #[serde(rename = "_meta", default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<Meta>,
    /// Gate verdict, present only when `--gate <mode>` was set (issue #886).
    /// Emitted on pass too (`verdict: "pass"`, `new_count: 0`) so consumers
    /// distinguish "gate ran and passed" from "gate did not run" (absent).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gate: Option<SecurityGate>,
    /// Security candidates. Paths are project-root-relative, forward-slash.
    pub security_findings: Vec<SecurityFinding>,
    /// Opt-in attack-surface inventory from untrusted entry points to reachable
    /// sinks. Present only when `--surface` was requested.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attack_surface: Option<Vec<SecurityAttackSurfaceEntry>>,
    /// In-band blind spot: number of `"use client"` files whose transitive
    /// import cone contains a dynamic `import()` the reachability BFS could not
    /// follow. A leak hidden behind such an edge would not be reported, so a
    /// zero finding count with a non-zero value here is NOT a clean bill.
    pub unresolved_edge_files: usize,
    /// In-band blind spot: number of sink-shaped nodes the catalogue detector
    /// could not flatten to a static callee path (dynamic dispatch, computed
    /// members, aliased bindings). A zero finding count with a non-zero value
    /// here is NOT a clean bill.
    pub unresolved_callee_sites: usize,
    /// Bounded diagnostics for unresolved callee blind spots.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unresolved_callee_diagnostics: Option<SecurityUnresolvedCalleeDiagnostics>,
}

/// Bounded unresolved-callee diagnostics for `fallow security --format json`.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SecurityUnresolvedCalleeDiagnostics {
    /// Deterministic sample rows, capped by `sample_limit`.
    pub sampled: Vec<SecurityUnresolvedCalleeSample>,
    /// Files with the most unresolved callees, capped by `top_files_limit`.
    pub top_files: Vec<SecurityUnresolvedCalleeTopFile>,
    /// Full count by unresolved-callee reason, sorted by count then reason.
    pub by_reason: Vec<SecurityUnresolvedCalleeReasonCount>,
    /// Maximum number of sample rows emitted.
    pub sample_limit: usize,
    /// Maximum number of top-file rows emitted.
    pub top_files_limit: usize,
}

/// One sampled unresolved-callee row.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SecurityUnresolvedCalleeSample {
    /// Project-relative source path.
    pub path: String,
    /// 1-based source line.
    pub line: u32,
    /// 0-based byte column.
    pub col: u32,
    /// Why the callee was skipped.
    pub reason: fallow_types::extract::SkippedSecurityCalleeReason,
    /// Compact syntax shape of the skipped callee.
    pub expression_kind: fallow_types::extract::SkippedSecurityCalleeExpressionKind,
}

/// Count of unresolved callees in one file.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SecurityUnresolvedCalleeTopFile {
    /// Project-relative source path.
    pub path: String,
    /// Number of unresolved callees in this file.
    pub count: usize,
}

/// Count of unresolved callees for one reason.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SecurityUnresolvedCalleeReasonCount {
    /// Why the callees were skipped.
    pub reason: fallow_types::extract::SkippedSecurityCalleeReason,
    /// Number of unresolved callees with this reason.
    pub count: usize,
}

/// Compact `fallow security --summary --format json` payload. Uses the same
/// `kind: "security"` discriminator as the full payload, but omits candidate
/// arrays and exposes only aggregate counts.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SecuritySummaryOutput {
    /// Schema version of this envelope.
    pub schema_version: SecuritySchemaVersion,
    /// Fallow CLI version that produced this output.
    pub version: ToolVersion,
    /// Wall-clock milliseconds spent producing the report.
    pub elapsed_ms: ElapsedMs,
    /// Privacy-safe config context relevant to security candidate generation.
    pub config: SecurityOutputConfig,
    /// Security-specific rule and field metadata, emitted with `--explain`.
    #[serde(rename = "_meta", default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<Meta>,
    /// Gate verdict, present only when `--gate <mode>` was set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gate: Option<SecurityGate>,
    /// Aggregate security counts after all filters, gates, and scopes.
    pub summary: SecuritySummary,
}

/// Aggregate counts for `fallow security --summary --format json`.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SecuritySummary {
    /// Number of security candidates after all filters, gates, and scopes.
    pub security_findings: usize,
    /// Fixed severity counts for the closed security severity enum.
    pub by_severity: SecuritySeverityCounts,
    /// Finding counts by catalogue category, or by kind for findings without a
    /// catalogue category.
    pub by_category: BTreeMap<String, usize>,
    /// Fixed reachability counts for ranking and triage signals.
    pub by_reachability: SecurityReachabilityCounts,
    /// Fixed runtime coverage counts for runtime-state triage signals.
    pub by_runtime_state: SecurityRuntimeStateCounts,
    /// Number of client files whose dynamic imports could not be followed.
    pub unresolved_edge_files: usize,
    /// Number of sink-shaped callees that could not be statically flattened.
    pub unresolved_callee_sites: usize,
    /// Number of attack-surface entries included in the prepared full output.
    pub attack_surface_entries: usize,
}

/// Fixed severity counters for summary JSON.
#[derive(Debug, Clone, Copy, Default, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SecuritySeverityCounts {
    pub high: usize,
    pub medium: usize,
    pub low: usize,
}

/// Fixed reachability counters for summary JSON.
#[derive(Debug, Clone, Copy, Default, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SecurityReachabilityCounts {
    pub entry_reachable: usize,
    pub untrusted_source_reachable: usize,
    pub arg_level: usize,
    pub module_level: usize,
    pub crosses_boundary: usize,
    pub source_backed: usize,
}

/// Fixed runtime coverage counters for summary JSON.
#[derive(Debug, Clone, Copy, Default, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SecurityRuntimeStateCounts {
    pub runtime_hot: usize,
    pub runtime_cold: usize,
    pub never_executed: usize,
    pub low_traffic: usize,
    pub coverage_unavailable: usize,
    pub runtime_unknown: usize,
    pub not_collected: usize,
}

/// Options for `fallow security`, mirroring the global CLI flags it honors.
pub struct SecurityOptions<'a> {
    /// Project root.
    pub root: &'a Path,
    /// Explicit config path (global `--config`).
    pub config_path: &'a Option<PathBuf>,
    /// Output format.
    pub output: OutputFormat,
    /// Disable the extraction cache.
    pub no_cache: bool,
    /// Resolved thread-pool size.
    pub threads: usize,
    /// Suppress progress output.
    pub quiet: bool,
    /// Exit with code 1 when candidates are found.
    pub fail_on_issues: bool,
    /// Write SARIF to a sidecar file in addition to the primary output.
    pub sarif_file: Option<&'a Path>,
    /// Show a compact human summary instead of per-finding detail.
    pub summary: bool,
    /// `--changed-since <ref>`: scope findings to files changed since the ref.
    pub changed_since: Option<&'a str>,
    /// Apply the shared `--diff-file` / `--diff-stdin` line filter.
    pub use_shared_diff_index: bool,
    /// `--workspace <patterns...>`: scope findings to selected workspace roots.
    pub workspace: Option<&'a [String]>,
    /// `--changed-workspaces <ref>`: scope to workspaces with changed files.
    pub changed_workspaces: Option<&'a str>,
    /// `--file <PATH>`: scope findings to selected files or trace hops.
    pub file: &'a [PathBuf],
    /// `--surface`: include the top-level attack-surface inventory in JSON.
    pub surface: bool,
    /// `--gate <mode>`: opt-in regression gate. `new` requires a diff source and
    /// reports candidates introduced in changed lines. `newly-reachable`
    /// requires `--changed-since <ref>` and reports candidates newly reachable
    /// from runtime entry points.
    pub gate: Option<SecurityGateMode>,
    /// Paid local runtime-coverage sidecar input.
    pub runtime_coverage: Option<&'a Path>,
    /// Threshold for hot-path classification when `--runtime-coverage` is set.
    pub min_invocations_hot: u64,
    /// Include security-specific `_meta` in JSON output.
    pub explain: bool,
}

/// Run `fallow security`. Always exits 0 unless the user explicitly raised the
/// `security-client-server-leak` rule to `error` AND findings exist (the rule
/// defaults to `off` and the command forces it to `warn`, so the common case is
/// advisory). Unsupported output formats exit 2.
pub fn run(opts: &SecurityOptions<'_>) -> ExitCode {
    let started = Instant::now();
    if !matches!(
        opts.output,
        OutputFormat::Human | OutputFormat::Json | OutputFormat::Sarif
    ) {
        return emit_error(
            "fallow security supports --format human, json, or sarif only.",
            2,
            opts.output,
        );
    }

    let mut config = match load_config_for_analysis(
        opts.root,
        opts.config_path,
        opts.output,
        opts.no_cache,
        opts.threads,
        None,
        opts.quiet,
        ProductionAnalysis::DeadCode,
    ) {
        Ok(config) => config,
        Err(code) => return code,
    };

    let configured_severity = config.rules.security_client_server_leak;
    let configured_sink_severity = config.rules.security_sink;
    force_security_rules(&mut config);
    let effective_severity = config.rules.security_client_server_leak;
    let effective_sink_severity = config.rules.security_sink;

    let mut analysis = match analyze_security_candidates(opts, &config) {
        Ok(analysis) => analysis,
        Err(code) => return code,
    };

    // Workspace scope (mutually exclusive flags resolved by the shared helper).
    let ws_roots = match crate::check::filtering::resolve_workspace_scope(
        opts.root,
        opts.workspace,
        opts.changed_workspaces,
        opts.output,
    ) {
        Ok(roots) => roots,
        Err(code) => return code,
    };
    if let Some(ref roots) = ws_roots {
        crate::check::filtering::filter_to_workspaces(&mut analysis.results, roots);
    }

    if !matches!(opts.gate, Some(SecurityGateMode::NewlyReachable)) {
        apply_changed_scope(opts, &mut analysis.results);
    }
    filter_to_files(&mut analysis.results, opts.root, opts.file, opts.quiet);

    let gate_mode = match apply_security_gate(opts, &config, &mut analysis.results) {
        Ok(mode) => mode,
        Err(code) => return code,
    };

    let unresolved_edge_files = analysis.results.security_unresolved_edge_files;
    let unresolved_callee_sites = analysis.results.security_unresolved_callee_sites;
    let unresolved_callee_diagnostics = unresolved_callee_diagnostics(
        &analysis.results.security_unresolved_callee_diagnostics,
        &config.root,
    );
    let runtime_report = match security_runtime_report(opts, &mut analysis) {
        Ok(report) => report,
        Err(code) => return code,
    };
    let mut findings: Vec<SecurityFinding> =
        std::mem::take(&mut analysis.results.security_findings)
            .into_iter()
            .map(|f| relativize_finding(f, &config.root))
            .collect();
    if let (Some(report), Some(modules), Some(files)) = (
        runtime_report.as_ref(),
        analysis.modules.as_ref(),
        analysis.files.as_ref(),
    ) {
        apply_runtime_context(&mut findings, modules, files, &config.root, report);
    }
    apply_security_severity(&mut findings);
    sort_by_security_severity(&mut findings);
    for finding in &mut findings {
        // Stamp the correlation id on the project-relative path so it matches
        // the SARIF fingerprint.
        finding.finding_id = security_finding_id(finding);
    }
    let (findings, attack_surface) = prepare_findings(findings, &config.root, opts.surface);

    // In gate mode the displayed set IS the strict "new" set, so its length is
    // the new-candidate count. The gate block is emitted unconditionally when a
    // gate ran (present on pass with verdict Pass / new_count 0) so consumers
    // distinguish "gate ran and passed" from "gate did not run".
    let gate = gate_mode.map(|mode| {
        let new_count = findings.len();
        SecurityGate {
            mode,
            verdict: if new_count > 0 {
                SecurityGateVerdict::Fail
            } else {
                SecurityGateVerdict::Pass
            },
            new_count,
        }
    });

    let advisory_fail = (opts.fail_on_issues
        || effective_severity == Severity::Error
        || effective_sink_severity == Severity::Error)
        && !findings.is_empty();

    let output = SecurityOutput {
        schema_version: SecuritySchemaVersion::V4,
        version: ToolVersion(env!("CARGO_PKG_VERSION").to_string()),
        elapsed_ms: ElapsedMs(started.elapsed().as_millis() as u64),
        config: security_output_config(
            &config,
            configured_severity,
            effective_severity,
            configured_sink_severity,
            effective_sink_severity,
        ),
        meta: opts.explain.then(crate::explain::security_meta),
        gate,
        security_findings: findings,
        attack_surface,
        unresolved_edge_files,
        unresolved_callee_sites,
        unresolved_callee_diagnostics,
    };
    crate::telemetry::note_result_count(output.security_findings.len());

    if let Some(path) = opts.sarif_file
        && let Err(message) = write_sarif_file(&output, path)
    {
        return emit_error(&message, 2, opts.output);
    }

    let rendered = match opts.output {
        OutputFormat::Json if opts.summary => render_json_summary(&output),
        OutputFormat::Json => render_json(&output),
        OutputFormat::Sarif => render_sarif(&output),
        _ if opts.summary => render_human_summary(&output),
        _ => render_human(&output),
    };
    outln!("{rendered}");

    // Exit-code contract (#886): in gate mode the gate is authoritative (8 when a
    // new candidate exists, else 0) and SUPERSEDES the advisory --fail-on-issues
    // path, because composing the two would re-gate on the pre-existing backlog
    // this gate exists to avoid. Code 8 is PURE: it means ONLY "new candidate
    // found", never "the gate could not run" (those are the exit-2 paths above).
    if let Some(gate) = &output.gate {
        if gate.verdict == SecurityGateVerdict::Fail {
            ExitCode::from(8)
        } else {
            ExitCode::SUCCESS
        }
    } else if advisory_fail {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}

fn force_security_rules(config: &mut fallow_config::ResolvedConfig) {
    // Respect explicit user severities; force the rules on when they are the
    // default off so this dedicated command actually surfaces candidates.
    if config.rules.security_client_server_leak == Severity::Off {
        config.rules.security_client_server_leak = Severity::Warn;
    }
    if config.rules.security_sink == Severity::Off {
        config.rules.security_sink = Severity::Warn;
    }
}

fn security_output_config(
    config: &fallow_config::ResolvedConfig,
    configured_severity: Severity,
    effective_severity: Severity,
    configured_sink_severity: Severity,
    effective_sink_severity: Severity,
) -> SecurityOutputConfig {
    let categories = config.security.categories.as_ref();
    SecurityOutputConfig {
        rules: SecurityOutputRulesConfig {
            security_client_server_leak: SecurityRuleSeverityConfig {
                configured: configured_severity,
                effective: effective_severity,
            },
            security_sink: SecurityRuleSeverityConfig {
                configured: configured_sink_severity,
                effective: effective_sink_severity,
            },
        },
        categories_include: categories.and_then(|categories| categories.include.clone()),
        categories_exclude: categories.and_then(|categories| categories.exclude.clone()),
    }
}

fn apply_changed_scope(opts: &SecurityOptions<'_>, results: &mut AnalysisResults) {
    if let Some(git_ref) = opts.changed_since
        && let Some(changed) = fallow_core::changed_files::get_changed_files(opts.root, git_ref)
    {
        fallow_core::changed_files::filter_results_by_changed_files(results, &changed);
    }
    if opts.use_shared_diff_index
        && let Some(diff_index) = crate::report::ci::diff_filter::shared_diff_index()
    {
        crate::check::filtering::filter_results_by_diff(results, diff_index, opts.root);
    }
}

fn apply_security_gate(
    opts: &SecurityOptions<'_>,
    config: &fallow_config::ResolvedConfig,
    results: &mut AnalysisResults,
) -> Result<Option<SecurityGateMode>, ExitCode> {
    let Some(mode) = opts.gate else {
        return Ok(None);
    };

    if matches!(mode, SecurityGateMode::NewlyReachable) {
        retain_gate_newly_reachable(opts, config, results)?;
        return Ok(Some(mode));
    }

    // Security gate (issue #886): narrow to the STRICT "new in changed lines"
    // predicate and drive a dedicated exit code. The gate requires a diff
    // source; a diff it cannot compute is a LOUD error (exit 2), never a green
    // gate (a silent miss defeats a security gate).
    let mut owned_gate_diff: Option<crate::report::ci::diff_filter::DiffIndex> = None;
    let gate_diff: &crate::report::ci::diff_filter::DiffIndex =
        if let Some(shared) = crate::report::ci::diff_filter::shared_diff_index() {
            shared
        } else if let Some(git_ref) = opts.changed_since {
            match fallow_core::changed_files::try_get_changed_diff(opts.root, git_ref) {
                Ok(text) => owned_gate_diff
                    .insert(crate::report::ci::diff_filter::DiffIndex::from_unified_diff(&text)),
                Err(err) => {
                    return Err(emit_error(
                        &format!(
                            "fallow security --gate could not compute the diff for '{git_ref}': {}",
                            err.describe()
                        ),
                        2,
                        opts.output,
                    ));
                }
            }
        } else {
            return Err(emit_error(
                "fallow security --gate requires a diff source: --changed-since <ref>, \
                     --diff-file <path>, or --diff-stdin.",
                2,
                opts.output,
            ));
        };
    crate::check::filtering::retain_gate_new(results, gate_diff, opts.root);
    Ok(Some(mode))
}

const SECURITY_BASE_SNAPSHOT_CACHE_VERSION: u8 = 1;
const MAX_SECURITY_BASE_SNAPSHOT_CACHE_SIZE: usize = 8 * 1024 * 1024;

#[derive(Debug, Clone)]
struct SecurityKeySnapshot {
    reachable: FxHashSet<String>,
}

struct SecurityBaseSnapshotCacheKey {
    hash: u64,
    base_sha: String,
}

#[derive(bitcode::Encode, bitcode::Decode)]
struct CachedSecurityKeySnapshot {
    version: u8,
    cli_version: String,
    key_hash: u64,
    base_sha: String,
    reachable: Vec<String>,
}

fn retain_gate_newly_reachable(
    opts: &SecurityOptions<'_>,
    config: &fallow_config::ResolvedConfig,
    results: &mut AnalysisResults,
) -> Result<(), ExitCode> {
    let Some(base_ref) = opts.changed_since else {
        return Err(emit_error(
            "fallow security --gate newly-reachable requires --changed-since <ref>; \
             --diff-file and --diff-stdin do not identify a base tree.",
            2,
            opts.output,
        ));
    };
    let Some(base_sha) = git_rev_parse(opts.root, base_ref) else {
        return Err(emit_error(
            &format!(
                "fallow security --gate newly-reachable could not resolve base ref '{base_ref}'."
            ),
            2,
            opts.output,
        ));
    };
    let cache_key = security_base_snapshot_cache_key(opts, config, &base_sha)?;
    let base = if let Some(snapshot) = load_cached_security_base_snapshot(config, &cache_key) {
        snapshot
    } else {
        let snapshot = compute_base_security_snapshot(opts, config, base_ref, &base_sha)?;
        save_cached_security_base_snapshot(config, &cache_key, &snapshot);
        snapshot
    };
    results.security_findings.retain(|finding| {
        security_reachability_key(finding, opts.root)
            .is_some_and(|key| !base.reachable.contains(&key))
    });
    Ok(())
}

fn compute_base_security_snapshot(
    opts: &SecurityOptions<'_>,
    config: &fallow_config::ResolvedConfig,
    base_ref: &str,
    base_sha: &str,
) -> Result<SecurityKeySnapshot, ExitCode> {
    let Some(worktree) = BaseWorktree::create(opts.root, base_ref, Some(base_sha)) else {
        return Err(emit_error(
            &format!("could not create a temporary worktree for base ref '{base_ref}'"),
            2,
            opts.output,
        ));
    };
    let base_root = base_analysis_root(opts.root, worktree.path());
    let current_config_path = opts
        .config_path
        .clone()
        .or_else(|| fallow_config::FallowConfig::find_config_path(opts.root));
    let mut base_config = load_config_for_analysis(
        &base_root,
        &current_config_path,
        opts.output,
        opts.no_cache,
        opts.threads,
        None,
        true,
        ProductionAnalysis::DeadCode,
    )?;
    base_config.cache_dir =
        remap_cache_dir_for_base_worktree(opts.root, &base_root, &config.cache_dir);
    force_security_rules(&mut base_config);
    let mut base_analysis = analyze_security_candidates(
        &SecurityOptions {
            root: &base_root,
            config_path: &current_config_path,
            output: opts.output,
            no_cache: opts.no_cache,
            threads: opts.threads,
            quiet: true,
            fail_on_issues: false,
            sarif_file: None,
            summary: false,
            changed_since: None,
            use_shared_diff_index: false,
            workspace: opts.workspace,
            changed_workspaces: None,
            file: &[],
            surface: false,
            gate: None,
            runtime_coverage: None,
            min_invocations_hot: opts.min_invocations_hot,
            explain: false,
        },
        &base_config,
    )?;
    if let Some(ref roots) = crate::check::filtering::resolve_workspace_scope(
        &base_root,
        opts.workspace,
        None,
        opts.output,
    )? {
        crate::check::filtering::filter_to_workspaces(&mut base_analysis.results, roots);
    }
    Ok(SecurityKeySnapshot {
        reachable: security_reachable_keys(&base_analysis.results.security_findings, &base_root),
    })
}

fn security_reachable_keys(findings: &[SecurityFinding], root: &Path) -> FxHashSet<String> {
    findings
        .iter()
        .filter_map(|finding| security_reachability_key(finding, root))
        .collect()
}

fn security_reachability_key(finding: &SecurityFinding, root: &Path) -> Option<String> {
    if !finding
        .reachability
        .as_ref()
        .is_some_and(|reachability| reachability.reachable_from_entry)
    {
        return None;
    }
    let category = finding.category.as_deref().unwrap_or("none");
    Some(format!(
        "security-reach:{}:{}:{}",
        relative_key(&finding.path, root),
        security_kind_key(finding.kind),
        category,
    ))
}

fn security_kind_key(kind: SecurityFindingKind) -> &'static str {
    match kind {
        SecurityFindingKind::ClientServerLeak => "client-server-leak",
        SecurityFindingKind::TaintedSink => "tainted-sink",
    }
}

fn security_base_snapshot_cache_key(
    opts: &SecurityOptions<'_>,
    config: &fallow_config::ResolvedConfig,
    base_sha: &str,
) -> Result<SecurityBaseSnapshotCacheKey, ExitCode> {
    let payload = serde_json::json!({
        "cache_version": SECURITY_BASE_SNAPSHOT_CACHE_VERSION,
        "cli_version": env!("CARGO_PKG_VERSION"),
        "base_sha": base_sha,
        "config_hash": format!("{:016x}", config.cache_config_hash),
        "security_client_server_leak": format!("{:?}", config.rules.security_client_server_leak),
        "security_sink": format!("{:?}", config.rules.security_sink),
        "workspace": opts.workspace,
        "changed_workspaces": opts.changed_workspaces,
    });
    let bytes = serde_json::to_vec(&payload).map_err(|err| {
        emit_error(
            &format!("failed to build security gate cache key: {err}"),
            2,
            opts.output,
        )
    })?;
    Ok(SecurityBaseSnapshotCacheKey {
        hash: xxh3_64(&bytes),
        base_sha: base_sha.to_owned(),
    })
}

fn security_base_snapshot_cache_dir(config: &fallow_config::ResolvedConfig) -> PathBuf {
    config.cache_dir.join("cache").join(format!(
        "security-base-v{SECURITY_BASE_SNAPSHOT_CACHE_VERSION}"
    ))
}

fn security_base_snapshot_cache_file(
    config: &fallow_config::ResolvedConfig,
    key: &SecurityBaseSnapshotCacheKey,
) -> PathBuf {
    security_base_snapshot_cache_dir(config).join(format!("{:016x}.bin", key.hash))
}

fn ensure_security_base_snapshot_cache_dir(dir: &Path) -> Result<(), std::io::Error> {
    std::fs::create_dir_all(dir)?;
    let gitignore = dir.join(".gitignore");
    if std::fs::read_to_string(&gitignore).ok().as_deref() != Some("*\n") {
        std::fs::write(gitignore, "*\n")?;
    }
    Ok(())
}

fn load_cached_security_base_snapshot(
    config: &fallow_config::ResolvedConfig,
    key: &SecurityBaseSnapshotCacheKey,
) -> Option<SecurityKeySnapshot> {
    if config.no_cache {
        return None;
    }
    let path = security_base_snapshot_cache_file(config, key);
    let data = std::fs::read(path).ok()?;
    if data.len() > MAX_SECURITY_BASE_SNAPSHOT_CACHE_SIZE {
        return None;
    }
    let cached: CachedSecurityKeySnapshot = bitcode::decode(&data).ok()?;
    if cached.version != SECURITY_BASE_SNAPSHOT_CACHE_VERSION
        || cached.cli_version != env!("CARGO_PKG_VERSION")
        || cached.key_hash != key.hash
        || cached.base_sha != key.base_sha
    {
        return None;
    }
    Some(SecurityKeySnapshot {
        reachable: cached.reachable.into_iter().collect(),
    })
}

fn save_cached_security_base_snapshot(
    config: &fallow_config::ResolvedConfig,
    key: &SecurityBaseSnapshotCacheKey,
    snapshot: &SecurityKeySnapshot,
) {
    if config.no_cache {
        return;
    }
    let dir = security_base_snapshot_cache_dir(config);
    if ensure_security_base_snapshot_cache_dir(&dir).is_err() {
        return;
    }
    let mut reachable = snapshot.reachable.iter().cloned().collect::<Vec<_>>();
    reachable.sort_unstable();
    let data = bitcode::encode(&CachedSecurityKeySnapshot {
        version: SECURITY_BASE_SNAPSHOT_CACHE_VERSION,
        cli_version: env!("CARGO_PKG_VERSION").to_owned(),
        key_hash: key.hash,
        base_sha: key.base_sha.clone(),
        reachable,
    });
    let Ok(mut tmp) = tempfile::NamedTempFile::new_in(&dir) else {
        return;
    };
    if tmp.write_all(&data).is_err() {
        return;
    }
    let _ = tmp.persist(security_base_snapshot_cache_file(config, key));
}

fn base_analysis_root(current_root: &Path, base_worktree_root: &Path) -> PathBuf {
    if current_root.is_absolute()
        && let Some(git_root) = crate::base_worktree::git_toplevel(current_root)
        && let Ok(relative) = current_root.strip_prefix(git_root)
    {
        return base_worktree_root.join(relative);
    }
    base_worktree_root.to_path_buf()
}

fn remap_cache_dir_for_base_worktree(
    current_root: &Path,
    base_worktree_root: &Path,
    cache_dir: &Path,
) -> PathBuf {
    if cache_dir.is_absolute()
        && let Ok(relative) = cache_dir.strip_prefix(current_root)
    {
        return base_worktree_root.join(relative);
    }
    cache_dir.to_path_buf()
}

struct SecurityAnalysisState {
    results: AnalysisResults,
    modules: Option<Vec<ModuleInfo>>,
    files: Option<Vec<DiscoveredFile>>,
    analysis_output: Option<fallow_core::AnalysisOutput>,
}

#[expect(
    deprecated,
    reason = "ADR-008 deprecates fallow_core::analyze APIs externally; the CLI uses the workspace path dependency"
)]
fn analyze_security_candidates(
    opts: &SecurityOptions<'_>,
    config: &fallow_config::ResolvedConfig,
) -> Result<SecurityAnalysisState, ExitCode> {
    if opts.runtime_coverage.is_none() {
        return fallow_core::analyze(config)
            .map(|results| SecurityAnalysisState {
                results,
                modules: None,
                files: None,
                analysis_output: None,
            })
            .map_err(|err| emit_error(&format!("Analysis error: {err}"), 2, opts.output));
    }

    fallow_core::analyze_retaining_modules(config, true, true)
        .map(|mut output| {
            let modules = output.modules.take();
            let files = output.files.take();
            let results = output.results.clone();
            SecurityAnalysisState {
                results,
                modules,
                files,
                analysis_output: Some(output),
            }
        })
        .map_err(|err| emit_error(&format!("Analysis error: {err}"), 2, opts.output))
}

fn security_runtime_report(
    opts: &SecurityOptions<'_>,
    analysis: &mut SecurityAnalysisState,
) -> Result<Option<RuntimeCoverageReport>, ExitCode> {
    let Some(path) = opts.runtime_coverage else {
        return Ok(None);
    };
    let (Some(modules), Some(files), Some(analysis_output)) = (
        analysis.modules.as_ref(),
        analysis.files.as_ref(),
        analysis.analysis_output.take(),
    ) else {
        return Ok(None);
    };
    analyze_security_runtime(opts, path, modules.clone(), files.clone(), analysis_output)
}

fn analyze_security_runtime(
    opts: &SecurityOptions<'_>,
    path: &Path,
    modules: Vec<ModuleInfo>,
    files: Vec<DiscoveredFile>,
    analysis_output: fallow_core::AnalysisOutput,
) -> Result<Option<RuntimeCoverageReport>, ExitCode> {
    let runtime_coverage = crate::health::coverage::prepare_options(
        path,
        opts.min_invocations_hot,
        None,
        None,
        opts.output,
    )?;
    let result = crate::health::execute_health_with_shared_parse(
        &HealthOptions {
            root: opts.root,
            config_path: opts.config_path,
            output: opts.output,
            no_cache: opts.no_cache,
            threads: opts.threads,
            quiet: opts.quiet,
            max_cyclomatic: None,
            max_cognitive: None,
            max_crap: None,
            top: None,
            sort: SortBy::Cyclomatic,
            production: true,
            production_override: Some(true),
            changed_since: opts.changed_since,
            diff_index: None,
            use_shared_diff_index: opts.use_shared_diff_index,
            workspace: opts.workspace,
            changed_workspaces: opts.changed_workspaces,
            baseline: None,
            save_baseline: None,
            complexity: false,
            complexity_breakdown: false,
            file_scores: false,
            coverage_gaps: false,
            config_activates_coverage_gaps: false,
            hotspots: false,
            ownership: false,
            ownership_emails: None,
            targets: false,
            force_full: false,
            score_only_output: false,
            enforce_coverage_gap_gate: false,
            effort: None,
            score: false,
            min_score: None,
            since: None,
            min_commits: None,
            explain: false,
            summary: false,
            save_snapshot: None,
            trend: false,
            group_by: None,
            coverage: None,
            coverage_root: None,
            performance: false,
            min_severity: None,
            report_only: false,
            runtime_coverage: Some(runtime_coverage),
            churn_file: None,
        },
        SharedParseData {
            files,
            modules,
            analysis_output: Some(analysis_output),
        },
    )?;
    Ok(result.report.runtime_coverage)
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct RuntimeFunctionKey {
    path: String,
    function: String,
    line: u32,
}

#[derive(Debug, Clone)]
struct FunctionSpan {
    key: RuntimeFunctionKey,
    end_line: u32,
}

fn apply_runtime_context(
    findings: &mut Vec<SecurityFinding>,
    modules: &[ModuleInfo],
    files: &[fallow_types::discover::DiscoveredFile],
    root: &Path,
    report: &RuntimeCoverageReport,
) {
    let spans = function_spans(modules, files, root);
    let runtime = SecurityRuntimeIndex::new(report);
    let mut indexed = findings.drain(..).enumerate().collect::<Vec<_>>();
    for (_, finding) in &mut indexed {
        if !matches!(finding.kind, SecurityFindingKind::TaintedSink) {
            continue;
        }
        finding.runtime = runtime_context_for_finding(finding, &spans, &runtime);
    }
    indexed.sort_by(|(left_index, left), (right_index, right)| {
        runtime_rank(left)
            .cmp(&runtime_rank(right))
            .then_with(|| left_index.cmp(right_index))
    });
    findings.extend(indexed.into_iter().map(|(_, finding)| finding));
}

fn function_spans(
    modules: &[ModuleInfo],
    files: &[fallow_types::discover::DiscoveredFile],
    root: &Path,
) -> Vec<FunctionSpan> {
    let paths_by_id = files
        .iter()
        .map(|file| (file.id, &file.path))
        .collect::<rustc_hash::FxHashMap<_, _>>();
    let mut spans = Vec::new();
    for module in modules {
        let Some(path) = paths_by_id.get(&module.file_id) else {
            continue;
        };
        let path = relative_key(path, root);
        for function in &module.complexity {
            spans.push(FunctionSpan {
                key: RuntimeFunctionKey {
                    path: path.clone(),
                    function: function.name.clone(),
                    line: function.line,
                },
                end_line: function.line.saturating_add(function.line_count),
            });
        }
    }
    spans
}

struct SecurityRuntimeIndex {
    hot_paths: Vec<(RuntimeFunctionKey, u32, SecurityRuntimeContext)>,
    findings: rustc_hash::FxHashMap<RuntimeFunctionKey, SecurityRuntimeContext>,
}

impl SecurityRuntimeIndex {
    fn new(report: &RuntimeCoverageReport) -> Self {
        let hot_paths = report
            .hot_paths
            .iter()
            .map(|hot| {
                (
                    runtime_hot_key(hot),
                    hot.end_line.max(hot.line),
                    SecurityRuntimeContext {
                        state: SecurityRuntimeState::RuntimeHot,
                        function: hot.function.clone(),
                        line: hot.line,
                        invocations: Some(hot.invocations),
                        stable_id: hot.stable_id.clone(),
                        evidence: Some(format!(
                            "production hot path observed with {} invocation{}",
                            hot.invocations,
                            crate::report::plural(hot.invocations as usize)
                        )),
                    },
                )
            })
            .collect();
        let findings = report
            .findings
            .iter()
            .map(runtime_finding_context)
            .collect();
        Self {
            hot_paths,
            findings,
        }
    }
}

fn runtime_context_for_finding(
    finding: &SecurityFinding,
    spans: &[FunctionSpan],
    runtime: &SecurityRuntimeIndex,
) -> Option<SecurityRuntimeContext> {
    let path = path_key(&finding.path);
    let span = spans
        .iter()
        .filter(|span| {
            span.key.path == path && span.key.line <= finding.line && finding.line <= span.end_line
        })
        .min_by_key(|span| span.end_line.saturating_sub(span.key.line))?;
    if let Some((_, _, context)) = runtime.hot_paths.iter().find(|(key, end_line, _)| {
        key == &span.key && key.line <= finding.line && finding.line <= *end_line
    }) {
        return Some(context.clone());
    }
    runtime.findings.get(&span.key).cloned().or_else(|| {
        Some(SecurityRuntimeContext {
            state: SecurityRuntimeState::RuntimeUnknown,
            function: span.key.function.clone(),
            line: span.key.line,
            invocations: None,
            stable_id: None,
            evidence: Some("runtime coverage carried no matching function evidence".to_owned()),
        })
    })
}

fn runtime_rank(finding: &SecurityFinding) -> u8 {
    match finding.runtime.as_ref().map(|runtime| runtime.state) {
        Some(SecurityRuntimeState::RuntimeHot) => 0,
        Some(SecurityRuntimeState::LowTraffic) => 1,
        None | Some(SecurityRuntimeState::RuntimeUnknown) => 2,
        Some(SecurityRuntimeState::CoverageUnavailable) => 3,
        Some(SecurityRuntimeState::RuntimeCold) => 4,
        Some(SecurityRuntimeState::NeverExecuted) => 5,
    }
}

fn apply_security_severity(findings: &mut [SecurityFinding]) {
    for finding in findings {
        finding.severity = derive_security_severity(finding);
    }
}

fn sort_by_security_severity(findings: &mut [SecurityFinding]) {
    findings.sort_by(compare_security_priority);
}

fn compare_security_priority(left: &SecurityFinding, right: &SecurityFinding) -> Ordering {
    security_severity_rank(left.severity)
        .cmp(&security_severity_rank(right.severity))
        .then_with(|| runtime_rank(left).cmp(&runtime_rank(right)))
        .then_with(|| {
            right
                .reachability
                .as_ref()
                .is_some_and(|reach| reach.reachable_from_entry)
                .cmp(
                    &left
                        .reachability
                        .as_ref()
                        .is_some_and(|reach| reach.reachable_from_entry),
                )
        })
        .then_with(|| taint_rank(left).cmp(&taint_rank(right)))
        .then_with(|| security_blast_radius(right).cmp(&security_blast_radius(left)))
        .then_with(|| security_crosses_boundary(right).cmp(&security_crosses_boundary(left)))
        .then_with(|| left.dead_code.is_some().cmp(&right.dead_code.is_some()))
        .then_with(|| left.path.cmp(&right.path))
        .then_with(|| left.line.cmp(&right.line))
        .then_with(|| left.col.cmp(&right.col))
        .then_with(|| left.category.cmp(&right.category))
}

fn taint_rank(finding: &SecurityFinding) -> u8 {
    match finding
        .reachability
        .as_ref()
        .and_then(|reach| reach.taint_confidence)
    {
        Some(TaintConfidence::ArgLevel) => 0,
        Some(TaintConfidence::ModuleLevel) => 1,
        None if finding.source_backed => 0,
        None if finding
            .reachability
            .as_ref()
            .is_some_and(|reach| reach.reachable_from_untrusted_source) =>
        {
            1
        }
        None => 2,
    }
}

fn security_blast_radius(finding: &SecurityFinding) -> u32 {
    finding
        .reachability
        .as_ref()
        .map_or(0, |reach| reach.blast_radius)
}

fn security_crosses_boundary(finding: &SecurityFinding) -> bool {
    finding
        .reachability
        .as_ref()
        .is_some_and(|reach| reach.crosses_boundary)
}

const fn security_severity_rank(severity: SecuritySeverity) -> u8 {
    match severity {
        SecuritySeverity::High => 0,
        SecuritySeverity::Medium => 1,
        SecuritySeverity::Low => 2,
    }
}

fn runtime_hot_key(hot: &RuntimeCoverageHotPath) -> RuntimeFunctionKey {
    RuntimeFunctionKey {
        path: path_key(&hot.path),
        function: hot.function.clone(),
        line: hot.line,
    }
}

fn runtime_finding_context(
    finding: &RuntimeCoverageFinding,
) -> (RuntimeFunctionKey, SecurityRuntimeContext) {
    let state = match finding.verdict {
        RuntimeCoverageVerdict::SafeToDelete => SecurityRuntimeState::NeverExecuted,
        RuntimeCoverageVerdict::ReviewRequired if finding.invocations.unwrap_or(0) == 0 => {
            SecurityRuntimeState::RuntimeCold
        }
        RuntimeCoverageVerdict::LowTraffic => SecurityRuntimeState::LowTraffic,
        RuntimeCoverageVerdict::CoverageUnavailable | RuntimeCoverageVerdict::Unknown => {
            SecurityRuntimeState::CoverageUnavailable
        }
        RuntimeCoverageVerdict::ReviewRequired | RuntimeCoverageVerdict::Active => {
            SecurityRuntimeState::RuntimeUnknown
        }
    };
    (
        RuntimeFunctionKey {
            path: path_key(&finding.path),
            function: finding.function.clone(),
            line: finding.line,
        },
        SecurityRuntimeContext {
            state,
            function: finding.function.clone(),
            line: finding.line,
            invocations: finding.invocations,
            stable_id: finding.stable_id.clone(),
            evidence: Some(format!("runtime coverage verdict: {}", finding.verdict)),
        },
    )
}

fn relative_key(path: &Path, root: &Path) -> String {
    path_key(path.strip_prefix(root).unwrap_or(path))
}

fn path_key(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn unresolved_callee_diagnostics(
    diagnostics: &[SecurityUnresolvedCalleeDiagnostic],
    root: &Path,
) -> Option<SecurityUnresolvedCalleeDiagnostics> {
    if diagnostics.is_empty() {
        return None;
    }

    let mut sorted = diagnostics.to_vec();
    sorted.sort_by(|a, b| {
        a.path
            .cmp(&b.path)
            .then(a.line.cmp(&b.line))
            .then(a.col.cmp(&b.col))
            .then(a.reason.cmp(&b.reason))
            .then(a.expression_kind.cmp(&b.expression_kind))
    });

    let sampled = sorted
        .iter()
        .take(UNRESOLVED_CALLEE_SAMPLE_LIMIT)
        .map(|diagnostic| SecurityUnresolvedCalleeSample {
            path: relative_key(&diagnostic.path, root),
            line: diagnostic.line,
            col: diagnostic.col,
            reason: diagnostic.reason,
            expression_kind: diagnostic.expression_kind,
        })
        .collect();

    let mut by_file: BTreeMap<String, usize> = BTreeMap::new();
    let mut by_reason: BTreeMap<fallow_types::extract::SkippedSecurityCalleeReason, usize> =
        BTreeMap::new();
    for diagnostic in &sorted {
        *by_file
            .entry(relative_key(&diagnostic.path, root))
            .or_insert(0) += 1;
        *by_reason.entry(diagnostic.reason).or_insert(0) += 1;
    }

    let mut top_files: Vec<_> = by_file
        .into_iter()
        .map(|(path, count)| SecurityUnresolvedCalleeTopFile { path, count })
        .collect();
    top_files.sort_by(|a, b| b.count.cmp(&a.count).then(a.path.cmp(&b.path)));
    top_files.truncate(UNRESOLVED_CALLEE_TOP_FILES_LIMIT);

    let mut by_reason: Vec<_> = by_reason
        .into_iter()
        .map(|(reason, count)| SecurityUnresolvedCalleeReasonCount { reason, count })
        .collect();
    by_reason.sort_by(|a, b| b.count.cmp(&a.count).then(a.reason.cmp(&b.reason)));

    Some(SecurityUnresolvedCalleeDiagnostics {
        sampled,
        top_files,
        by_reason,
        sample_limit: UNRESOLVED_CALLEE_SAMPLE_LIMIT,
        top_files_limit: UNRESOLVED_CALLEE_TOP_FILES_LIMIT,
    })
}

fn filter_to_files(
    results: &mut fallow_core::results::AnalysisResults,
    root: &Path,
    files: &[PathBuf],
    quiet: bool,
) {
    if files.is_empty() {
        return;
    }

    let resolved_files: Vec<PathBuf> = files
        .iter()
        .map(|path| {
            if crate::path_util::is_absolute_path_any_platform(path) {
                path.clone()
            } else {
                root.join(path)
            }
        })
        .collect();

    if !quiet {
        for (original, resolved) in files.iter().zip(&resolved_files) {
            if !resolved.exists() {
                eprintln!(
                    "Warning: --file '{}' (resolved to '{}') was not found in the project",
                    original.display(),
                    resolved.display()
                );
            }
        }
    }

    let file_set: rustc_hash::FxHashSet<PathBuf> = resolved_files.into_iter().collect();
    fallow_core::changed_files::filter_results_by_changed_files(results, &file_set);
}

fn prepare_findings(
    findings: Vec<SecurityFinding>,
    root: &Path,
    include_surface: bool,
) -> (
    Vec<SecurityFinding>,
    Option<Vec<SecurityAttackSurfaceEntry>>,
) {
    let mut findings: Vec<SecurityFinding> = findings
        .into_iter()
        .map(|f| {
            let mut f = relativize_finding(f, root);
            f.finding_id = security_finding_id(&f);
            f
        })
        .collect();
    let attack_surface = include_surface.then(|| {
        findings
            .iter()
            .filter_map(|finding| finding.attack_surface.clone())
            .collect()
    });
    for finding in &mut findings {
        finding.attack_surface = None;
    }
    (findings, attack_surface)
}

/// Rewrite a finding's anchor + every trace hop path to be project-root-relative
/// (forward-slash normalization happens at serialize time via `serde_path`).
fn relativize_finding(mut finding: SecurityFinding, root: &Path) -> SecurityFinding {
    finding.path = relativize(&finding.path, root);
    for hop in &mut finding.trace {
        hop.path = relativize(&hop.path, root);
    }
    if let Some(reachability) = &mut finding.reachability {
        for hop in &mut reachability.untrusted_source_trace {
            hop.path = relativize(&hop.path, root);
        }
    }
    finding.candidate.sink.path = relativize(&finding.candidate.sink.path, root);
    if let Some(flow) = &mut finding.taint_flow {
        flow.source.path = relativize(&flow.source.path, root);
        flow.sink.path = relativize(&flow.sink.path, root);
    }
    if let Some(surface) = &mut finding.attack_surface {
        surface.source.path = relativize(&surface.source.path, root);
        surface.sink.path = relativize(&surface.sink.path, root);
        for hop in &mut surface.path {
            hop.path = relativize(&hop.path, root);
        }
        for control in &mut surface.defensive_boundary.controls {
            control.path = relativize(&control.path, root);
        }
    }
    finding
}

fn relativize(path: &Path, root: &Path) -> PathBuf {
    path.strip_prefix(root)
        .map_or_else(|_| path.to_path_buf(), Path::to_path_buf)
}

/// JSON: the `SecurityOutput` envelope, pretty-printed.
#[must_use]
pub fn render_json(output: &SecurityOutput) -> String {
    let Ok(value) = crate::output_envelope::serialize_root_output(
        crate::output_envelope::FallowOutput::Security(output.clone()),
    ) else {
        return "{\"error\":\"failed to serialize security output\"}".to_owned();
    };
    serde_json::to_string_pretty(&value)
        .unwrap_or_else(|_| "{\"error\":\"failed to serialize security output\"}".to_owned())
}

/// JSON summary: compact aggregate payload without per-finding arrays.
#[must_use]
pub fn render_json_summary(output: &SecurityOutput) -> String {
    let summary = SecuritySummaryOutput {
        schema_version: output.schema_version,
        version: output.version.clone(),
        elapsed_ms: output.elapsed_ms,
        config: output.config.clone(),
        meta: output.meta.clone(),
        gate: output.gate,
        summary: security_summary(output),
    };
    let Ok(value) = crate::output_envelope::serialize_root_output_without_telemetry(
        crate::output_envelope::FallowOutput::SecuritySummary(summary),
    ) else {
        return "{\"error\":\"failed to serialize security summary output\"}".to_owned();
    };
    serde_json::to_string_pretty(&value).unwrap_or_else(|_| {
        "{\"error\":\"failed to serialize security summary output\"}".to_owned()
    })
}

fn security_summary(output: &SecurityOutput) -> SecuritySummary {
    let mut by_severity = SecuritySeverityCounts::default();
    let mut by_reachability = SecurityReachabilityCounts::default();
    let mut by_runtime_state = SecurityRuntimeStateCounts::default();
    let mut by_category = BTreeMap::new();

    for finding in &output.security_findings {
        match finding.severity {
            SecuritySeverity::High => by_severity.high += 1,
            SecuritySeverity::Medium => by_severity.medium += 1,
            SecuritySeverity::Low => by_severity.low += 1,
        }
        let category = finding
            .category
            .clone()
            .unwrap_or_else(|| security_kind_key(finding.kind).to_owned());
        *by_category.entry(category).or_insert(0) += 1;

        if finding.source_backed {
            by_reachability.source_backed += 1;
        }
        if let Some(reachability) = &finding.reachability {
            if reachability.reachable_from_entry {
                by_reachability.entry_reachable += 1;
            }
            if reachability.reachable_from_untrusted_source {
                by_reachability.untrusted_source_reachable += 1;
            }
            if reachability.crosses_boundary {
                by_reachability.crosses_boundary += 1;
            }
            match reachability.taint_confidence {
                Some(TaintConfidence::ArgLevel) => by_reachability.arg_level += 1,
                Some(TaintConfidence::ModuleLevel) => by_reachability.module_level += 1,
                None => {}
            }
        }

        match finding.runtime.as_ref().map(|runtime| runtime.state) {
            Some(SecurityRuntimeState::RuntimeHot) => by_runtime_state.runtime_hot += 1,
            Some(SecurityRuntimeState::RuntimeCold) => by_runtime_state.runtime_cold += 1,
            Some(SecurityRuntimeState::NeverExecuted) => by_runtime_state.never_executed += 1,
            Some(SecurityRuntimeState::LowTraffic) => by_runtime_state.low_traffic += 1,
            Some(SecurityRuntimeState::CoverageUnavailable) => {
                by_runtime_state.coverage_unavailable += 1;
            }
            Some(SecurityRuntimeState::RuntimeUnknown) => by_runtime_state.runtime_unknown += 1,
            None => by_runtime_state.not_collected += 1,
        }
    }

    SecuritySummary {
        security_findings: output.security_findings.len(),
        by_severity,
        by_category,
        by_reachability,
        by_runtime_state,
        unresolved_edge_files: output.unresolved_edge_files,
        unresolved_callee_sites: output.unresolved_callee_sites,
        attack_surface_entries: output.attack_surface.as_ref().map_or(0, Vec::len),
    }
}

fn write_sarif_file(output: &SecurityOutput, path: &Path) -> Result<(), String> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent).map_err(|err| {
            format!(
                "Failed to create directory for SARIF file {}: {err}",
                path.display()
            )
        })?;
    }
    std::fs::write(path, render_sarif(output))
        .map_err(|err| format!("Failed to write SARIF file {}: {err}", path.display()))
}

/// One-line gate verdict header. Leads with the ACTION ("REVIEW REQUIRED") and
/// immediately qualifies with the candidate framing, so a human never reads the
/// gate as "fallow confirmed a vulnerability". The wire `verdict` token stays
/// `fail`; only this human prose says "REVIEW REQUIRED".
fn gate_human_header(gate: &SecurityGate) -> String {
    use crate::report::plural;
    let checked = match gate.mode {
        SecurityGateMode::New => "in changed lines",
        SecurityGateMode::NewlyReachable => "newly reachable from entry points",
    };
    match gate.verdict {
        SecurityGateVerdict::Fail => format!(
            "Gate: REVIEW REQUIRED, {} new security item{} {checked}. fallow has not confirmed a vulnerability.",
            gate.new_count,
            plural(gate.new_count),
        ),
        SecurityGateVerdict::Pass => {
            format!("Gate: PASS, no new security items {checked}.")
        }
    }
}

fn unresolved_callee_human_hint(output: &SecurityOutput) -> Option<String> {
    let diagnostics = output.unresolved_callee_diagnostics.as_ref()?;
    let top_reason = diagnostics.by_reason.first()?;
    let top_file = diagnostics.top_files.first()?;
    Some(format!(
        "Most unresolved callees: {} in {}.",
        unresolved_callee_reason_label(top_reason.reason),
        top_file.path
    ))
}

fn unresolved_callee_reason_label(
    reason: fallow_types::extract::SkippedSecurityCalleeReason,
) -> &'static str {
    match reason {
        fallow_types::extract::SkippedSecurityCalleeReason::ComputedMember => "computed-member",
        fallow_types::extract::SkippedSecurityCalleeReason::DynamicDispatch => "dynamic-dispatch",
        fallow_types::extract::SkippedSecurityCalleeReason::UnsupportedAssignmentObject => {
            "unsupported-assignment-object"
        }
    }
}

#[must_use]
fn render_human_summary(output: &SecurityOutput) -> String {
    use crate::report::plural;
    use std::fmt::Write as _;

    let mut out = String::new();
    if let Some(gate) = &output.gate {
        out.push_str(&gate_human_header(gate));
        out.push('\n');
    }
    let count = output.security_findings.len();
    if count == 0 {
        out.push_str("Security review: no items to check in the scanned code.\n");
    } else {
        let _ = writeln!(
            out,
            "Security review: {count} item{} to check. These are unverified security candidates, not confirmed vulnerabilities.",
            plural(count),
        );
        out.push_str(
            "Next: check whether the listed code can run with unsafe input, secrets, or settings, and whether anything blocks the risk.\n",
        );
    }
    if output.unresolved_edge_files > 0 {
        let n = output.unresolved_edge_files;
        let verb = if n == 1 { "uses" } else { "use" };
        let _ = writeln!(
            out,
            "Blind spot: {n} client file{} {verb} dynamic imports that fallow could not follow.",
            plural(n)
        );
    }
    if output.unresolved_callee_sites > 0 {
        let n = output.unresolved_callee_sites;
        let verb = if n == 1 { "uses" } else { "use" };
        let _ = writeln!(
            out,
            "Blind spot: {n} call site{} {verb} code patterns that fallow could not resolve.",
            plural(n)
        );
        if let Some(hint) = unresolved_callee_human_hint(output) {
            let _ = writeln!(out, "{hint}");
        }
    }
    out
}

/// Human output. Frames findings as candidates and states the next human action
/// per finding; surfaces the unresolved-edge blind spot as a counted line.
#[must_use]
#[expect(
    clippy::format_push_string,
    reason = "small report renderer; readability over avoiding the extra allocation"
)]
pub fn render_human(output: &SecurityOutput) -> String {
    use crate::report::plural;
    use colored::Colorize;

    let mut out = String::new();
    if let Some(gate) = &output.gate {
        out.push_str(&gate_human_header(gate));
        out.push_str("\n\n");
    }
    let count = output.security_findings.len();
    out.push_str(&format!("Security review: {count} item{}", plural(count)));
    if count == 0 {
        out.push_str(" to check in the scanned code.\n");
    } else {
        out.push_str(" to check.\n");
        out.push_str(
            "These are unverified security candidates, not confirmed vulnerabilities. Check whether the listed code can run with unsafe input, secrets, or settings, and whether anything blocks the risk.\n",
        );
    }
    out.push('\n');

    if output.security_findings.is_empty() {
        out.push_str("No security details to show.\n");
    } else {
        for finding in &output.security_findings {
            let kind = security_finding_label(finding);
            let (glyph, label) = human_severity_marker(finding.severity);
            out.push_str(&format!(
                "{} {label} {kind}  {}:{}\n",
                glyph,
                finding.path.to_string_lossy().replace('\\', "/").bold(),
                finding.line,
            ));
            out.push_str(&format!("    evidence: {}\n", finding.evidence));
            if let Some(hint) = dead_code_hint(finding) {
                out.push_str(&format!("    dead-code: {hint}\n"));
            }
            if let Some(runtime) = finding.runtime.as_ref() {
                out.push_str(&format!("    runtime: {}\n", runtime_hint_text(runtime)));
            }
            if let Some(reach) = finding.reachability.as_ref() {
                let entry = if reach.reachable_from_entry {
                    "reachable from a runtime entry point"
                } else {
                    "not reached from any runtime entry point"
                };
                let boundary = if reach.crosses_boundary {
                    "; crosses an architecture boundary"
                } else {
                    ""
                };
                out.push_str(&format!(
                    "    code path: {entry} (blast radius {}){boundary}\n",
                    reach.blast_radius,
                ));
                if reach.reachable_from_untrusted_source {
                    let hops = reach.untrusted_source_hop_count.unwrap_or(0);
                    out.push_str(&format!(
                        "    input path: this module is reachable from a module that receives \
                         untrusted input via {hops} import hop{}\n",
                        crate::report::plural(hops as usize),
                    ));
                    if !reach.untrusted_source_trace.is_empty() {
                        out.push_str("    input import trace:\n");
                        for hop in &reach.untrusted_source_trace {
                            out.push_str(&format!(
                                "      {}:{} ({})\n",
                                hop.path.to_string_lossy().replace('\\', "/"),
                                hop.line,
                                hop_role_label(hop.role),
                            ));
                        }
                    }
                }
            }
            if !finding.trace.is_empty() {
                out.push_str("    import trace:\n");
                for hop in &finding.trace {
                    out.push_str(&format!(
                        "      {}:{} ({})\n",
                        hop.path.to_string_lossy().replace('\\', "/"),
                        hop.line,
                        hop_role_label(hop.role),
                    ));
                }
            }
            if matches!(finding.kind, SecurityFindingKind::ClientServerLeak) {
                out.push_str(
                    "    Next: check whether this import can ship a secret to the browser. If \
                     it is type-only, server-only, or removed at build time, mark it as a false \
                     positive.\n",
                );
            } else if finding.dead_code.is_some() {
                out.push_str(
                    "    Next: first verify the dead-code finding. If the code is safe to \
                     remove, delete it. Otherwise check and harden the risky call.\n",
                );
            } else {
                out.push_str(
                    "    Next: check whether unsafe input, secrets, or settings can reach this \
                     risky call without a safe guard. If not, mark it as a false positive.\n",
                );
            }
            out.push('\n');
        }
    }

    if output.unresolved_edge_files > 0 {
        let n = output.unresolved_edge_files;
        let verb = if n == 1 { "uses" } else { "use" };
        out.push_str(&format!(
            "{} Blind spot: {n} client file{} {verb} dynamic imports that fallow could not \
             follow. Code behind those imports may be missing from this report.\n",
            "[I]".blue().bold(),
            plural(n),
        ));
    }

    if output.unresolved_callee_sites > 0 {
        let n = output.unresolved_callee_sites;
        let verb = if n == 1 { "uses" } else { "use" };
        out.push_str(&format!(
            "{} Blind spot: {n} call site{} {verb} code patterns that fallow could not resolve, \
             such as dynamic dispatch, computed members, or aliased bindings.\n",
            "[I]".blue().bold(),
            plural(n),
        ));
        if let Some(hint) = unresolved_callee_human_hint(output) {
            out.push_str(&format!("    {hint}\n"));
        }
    }

    out.push_str(&format!(
        "\nResult: {count} security item{} to check.",
        plural(count),
    ));
    if count > 0 {
        out.push_str(" Review the listed evidence and trace before changing code.");
    }
    out.push('\n');
    out
}

/// Render the human-facing label for a finding. `ClientServerLeak` keeps its
/// bespoke kebab kind; `TaintedSink` uses the catalogue title plus the CWE
/// number carried on the finding.
fn security_finding_label(finding: &SecurityFinding) -> String {
    match finding.kind {
        SecurityFindingKind::ClientServerLeak => "client-server-leak".to_string(),
        SecurityFindingKind::TaintedSink => {
            let title = finding
                .category
                .as_deref()
                .and_then(fallow_core::analyze::security_catalogue_title)
                .or(finding.category.as_deref())
                .unwrap_or("tainted-sink");
            match finding.cwe {
                Some(cwe) => format!("{title} (CWE-{cwe})"),
                None => title.to_string(),
            }
        }
    }
}

fn human_severity_marker(severity: SecuritySeverity) -> (colored::ColoredString, &'static str) {
    use colored::Colorize;
    match severity {
        SecuritySeverity::High => ("[H]".red().bold(), "high"),
        SecuritySeverity::Medium => ("[M]".yellow().bold(), "medium"),
        SecuritySeverity::Low => ("[L]".blue().bold(), "low"),
    }
}

fn dead_code_hint(finding: &SecurityFinding) -> Option<String> {
    let context = finding.dead_code.as_ref()?;
    match context.kind {
        SecurityDeadCodeKind::UnusedFile => Some(
            "also reported as unused-file; delete this file instead of hardening the sink"
                .to_string(),
        ),
        SecurityDeadCodeKind::UnusedExport => Some(format!(
            "also reported as unused-export{}; remove the export instead of hardening the sink",
            context
                .export_name
                .as_ref()
                .map_or(String::new(), |name| format!(" `{name}`"))
        )),
    }
}

const fn hop_role_label(role: TraceHopRole) -> &'static str {
    match role {
        TraceHopRole::ClientBoundary => "client boundary",
        TraceHopRole::UntrustedSource => "untrusted source",
        TraceHopRole::ModuleSource => "source module",
        TraceHopRole::Intermediate => "intermediate",
        TraceHopRole::SecretSource => "secret source",
        TraceHopRole::Sink => "sink site",
    }
}

fn source_reachability_hint(finding: &SecurityFinding) -> Option<&'static str> {
    finding
        .reachability
        .as_ref()
        .filter(|reach| reach.reachable_from_untrusted_source)
        .map(|_| {
            "Module-level context: the sink module is reachable from an untrusted-source module; fallow does not prove value flow."
        })
}

fn runtime_hint_text(runtime: &SecurityRuntimeContext) -> String {
    use std::fmt::Write as _;

    let mut text = format!(
        "{} in {}:{}",
        runtime_state_label(runtime.state),
        runtime.function,
        runtime.line
    );
    if let Some(invocations) = runtime.invocations {
        let _ = write!(
            text,
            " ({} invocation{})",
            invocations,
            crate::report::plural(invocations as usize)
        );
    }
    if let Some(evidence) = runtime.evidence.as_deref() {
        text.push_str("; ");
        text.push_str(evidence);
    }
    text
}

const fn runtime_state_label(state: SecurityRuntimeState) -> &'static str {
    match state {
        SecurityRuntimeState::RuntimeHot => "runtime-hot",
        SecurityRuntimeState::RuntimeCold => "runtime-cold",
        SecurityRuntimeState::NeverExecuted => "never-executed",
        SecurityRuntimeState::LowTraffic => "low-traffic",
        SecurityRuntimeState::CoverageUnavailable => "coverage-unavailable",
        SecurityRuntimeState::RuntimeUnknown => "runtime-unknown",
    }
}

/// The SARIF ruleId for a finding. `client-server-leak` keeps its bespoke id;
/// each `TaintedSink` category gets `security/<category>` so the GitHub Security
/// tab groups and labels candidates per CWE class instead of collapsing every
/// finding under the client-server-leak rule.
fn sarif_rule_id(finding: &SecurityFinding) -> String {
    match finding.kind {
        SecurityFindingKind::ClientServerLeak => "security/client-server-leak".to_owned(),
        SecurityFindingKind::TaintedSink => {
            format!(
                "security/{}",
                finding.category.as_deref().unwrap_or("tainted-sink")
            )
        }
    }
}

fn security_help_text(title: &str) -> String {
    format!(
        "Verify this unverified {title} candidate before acting. Review the source, sink, \
         SARIF code flow, and any runtime or dead-code context. fallow does not prove \
         exploitability, attacker control, or missing sanitization."
    )
}

fn security_help_markdown(title: &str) -> String {
    format!(
        "Verify this unverified **{title}** candidate before acting.\n\n\
         1. Review the source and sink in the SARIF code flow.\n\
         2. Confirm whether attacker-controlled data can reach the sink unsanitized.\n\
         3. Use runtime and dead-code context only as triage signals."
    )
}

fn cwe_taxon_id(cwe: u32) -> String {
    format!("CWE-{cwe}")
}

fn cwe_taxon(cwe: u32) -> serde_json::Value {
    let id = cwe_taxon_id(cwe);
    serde_json::json!({
        "id": id,
        "name": id,
        "shortDescription": { "text": format!("Common Weakness Enumeration {id}") },
        "fullDescription": { "text": format!("MITRE Common Weakness Enumeration {id}") },
        "helpUri": format!("https://cwe.mitre.org/data/definitions/{cwe}.html")
    })
}

fn cwe_relationship(cwe: u32, taxon_index: usize) -> serde_json::Value {
    serde_json::json!({
        "target": {
            "id": cwe_taxon_id(cwe),
            "index": taxon_index,
            "toolComponent": {
                "name": "CWE",
                "index": 0
            }
        },
        "kinds": ["superset"]
    })
}

fn collect_cwes(findings: &[SecurityFinding]) -> Vec<u32> {
    let mut cwes: Vec<u32> = findings.iter().filter_map(|finding| finding.cwe).collect();
    cwes.sort_unstable();
    cwes.dedup();
    cwes
}

fn cwe_index(cwes: &[u32], cwe: u32) -> Option<usize> {
    cwes.iter().position(|existing| *existing == cwe)
}

fn cwe_taxonomy(cwes: &[u32]) -> Option<serde_json::Value> {
    if cwes.is_empty() {
        return None;
    }
    let taxa = cwes.iter().map(|cwe| cwe_taxon(*cwe)).collect::<Vec<_>>();
    Some(serde_json::json!({
        "name": "CWE",
        "fullName": "Common Weakness Enumeration",
        "organization": "MITRE",
        "informationUri": "https://cwe.mitre.org/",
        "taxa": taxa
    }))
}

/// Build the SARIF rule definition for a ruleId, deriving per-category metadata
/// (catalogue title + CWE tag and relationship) for `TaintedSink` findings so
/// CWE grouping survives in SARIF-aware consumers.
fn sarif_rule_def(
    rule_id: &str,
    finding: &SecurityFinding,
    cwe_taxon_index: Option<usize>,
) -> serde_json::Value {
    match finding.kind {
        SecurityFindingKind::ClientServerLeak => {
            let title = "Client-server secret leak";
            serde_json::json!({
                "id": rule_id,
                "name": title,
                "shortDescription": { "text": "Client-server secret leak candidate (unverified)" },
                "fullDescription": { "text":
                    "Unverified candidate, requires verification: a \"use client\" file \
                     transitively imports a module that reads a non-public process.env \
                     secret. fallow does not prove the secret reaches client-bundled code." },
                "help": {
                    "text": security_help_text(title),
                    "markdown": security_help_markdown(title)
                },
                "helpUri": "https://github.com/fallow-rs/fallow",
                "defaultConfiguration": { "level": "note" }
            })
        }
        SecurityFindingKind::TaintedSink => {
            let title = finding
                .category
                .as_deref()
                .and_then(fallow_core::analyze::security_catalogue_title)
                .or(finding.category.as_deref())
                .unwrap_or("tainted-sink");
            let mut rule = serde_json::json!({
                "id": rule_id,
                "name": title,
                "shortDescription": { "text": format!("{title} candidate (unverified)") },
                "fullDescription": { "text": format!(
                    "Unverified candidate, requires verification: {title}. fallow flags a \
                     syntactic sink reached by a non-literal argument; it does not prove the \
                     value is attacker-controlled or reaches the sink unsanitized."
                ) },
                "help": {
                    "text": security_help_text(title),
                    "markdown": security_help_markdown(title)
                },
                "helpUri": "https://github.com/fallow-rs/fallow",
                "defaultConfiguration": { "level": "note" }
            });
            if let Some(cwe) = finding.cwe {
                rule["properties"] = serde_json::json!({
                    "tags": [format!("external/cwe/cwe-{cwe}")]
                });
                if let Some(taxon_index) = cwe_taxon_index {
                    rule["relationships"] = serde_json::json!([cwe_relationship(cwe, taxon_index)]);
                }
            }
            rule
        }
    }
}

fn hop_role_token(role: TraceHopRole) -> &'static str {
    match role {
        TraceHopRole::ClientBoundary => "client-boundary",
        TraceHopRole::UntrustedSource => "untrusted-source",
        TraceHopRole::ModuleSource => "module-source",
        TraceHopRole::Intermediate => "intermediate",
        TraceHopRole::SecretSource => "secret-source",
        TraceHopRole::Sink => "sink",
    }
}

fn sarif_thread_flow_location(hop: &TraceHop) -> serde_json::Value {
    let role = hop_role_token(hop.role);
    serde_json::json!({
        "location": sarif_location(&hop.path, hop.line, hop.col),
        "kinds": [role],
        "properties": { "fallowTraceRole": role }
    })
}

fn primary_code_flow_hops(finding: &SecurityFinding) -> &[TraceHop] {
    if let Some(reachability) = finding.reachability.as_ref()
        && !reachability.untrusted_source_trace.is_empty()
    {
        return &reachability.untrusted_source_trace;
    }
    &finding.trace
}

fn sarif_code_flows(finding: &SecurityFinding) -> Option<serde_json::Value> {
    let hops = primary_code_flow_hops(finding);
    if hops.is_empty() {
        return None;
    }
    let locations = hops
        .iter()
        .map(sarif_thread_flow_location)
        .collect::<Vec<_>>();
    Some(serde_json::json!([
        {
            "threadFlows": [
                { "locations": locations }
            ]
        }
    ]))
}

fn push_related_location(related: &mut Vec<serde_json::Value>, hop: &TraceHop) {
    let location = sarif_location(&hop.path, hop.line, hop.col);
    if !related.iter().any(|existing| existing == &location) {
        related.push(location);
    }
}

fn sarif_related_locations(finding: &SecurityFinding) -> Vec<serde_json::Value> {
    let mut related = Vec::new();
    for hop in &finding.trace {
        push_related_location(&mut related, hop);
    }
    if let Some(reachability) = finding.reachability.as_ref() {
        for hop in &reachability.untrusted_source_trace {
            push_related_location(&mut related, hop);
        }
    }
    related
}

const fn sarif_level(severity: SecuritySeverity) -> &'static str {
    match severity {
        SecuritySeverity::High | SecuritySeverity::Medium => "warning",
        SecuritySeverity::Low => "note",
    }
}

/// SARIF output. Maps the candidate's verification-priority tier to SARIF
/// `level` while keeping the message text candidate-framed. Each finding's ruleId is
/// per-category (`security/<category>` for tainted-sink, `security/client-server-leak`
/// for the graph rule); the `rules` array carries one definition per distinct
/// ruleId present, with the CWE tag for tainted-sink categories. Detector trace
/// hops and source-reachability hops become `relatedLocations` of the result.
#[must_use]
fn render_sarif(output: &SecurityOutput) -> String {
    let cwes = collect_cwes(&output.security_findings);
    let results: Vec<serde_json::Value> = output
        .security_findings
        .iter()
        .map(|finding| {
            let rule_id = sarif_rule_id(finding);
            let mut message = dead_code_hint(finding).map_or_else(
                || finding.evidence.clone(),
                |hint| format!("{} Dead-code cross-link: {hint}.", finding.evidence),
            );
            if let Some(hint) = source_reachability_hint(finding) {
                message.push(' ');
                message.push_str(hint);
            }
            if let Some(runtime) = finding.runtime.as_ref() {
                message.push_str(" Runtime context: ");
                message.push_str(&runtime_hint_text(runtime));
                message.push('.');
            }
            let related = sarif_related_locations(finding);
            // Stable dedup key for GHAS: rule + anchor path + line. Without
            // partialFingerprints, every run re-opens previously triaged alerts.
            // Same helper as the JSON `finding_id` field so the two never drift
            // (issue #900).
            let mut result = serde_json::json!({
                "ruleId": rule_id,
                "level": sarif_level(finding.severity),
                "message": { "text": message },
                "locations": [sarif_location(&finding.path, finding.line, finding.col)],
                "relatedLocations": related,
                "partialFingerprints": { "fallowSecurity/v1": security_finding_id(finding) },
            });
            if let Some(code_flows) = sarif_code_flows(finding) {
                result["codeFlows"] = code_flows;
            }
            result
        })
        .collect();

    // One rule definition per distinct ruleId present in the findings.
    let mut seen: Vec<String> = Vec::new();
    let mut rules: Vec<serde_json::Value> = Vec::new();
    for finding in &output.security_findings {
        let rule_id = sarif_rule_id(finding);
        if seen.iter().any(|s| s == &rule_id) {
            continue;
        }
        seen.push(rule_id.clone());
        let cwe_taxon_index = finding.cwe.and_then(|cwe| cwe_index(&cwes, cwe));
        rules.push(sarif_rule_def(&rule_id, finding, cwe_taxon_index));
    }

    let mut run = serde_json::json!({
        "tool": { "driver": {
            "name": "fallow",
            "version": env!("CARGO_PKG_VERSION"),
            "informationUri": "https://github.com/fallow-rs/fallow",
            "rules": rules,
        }},
        "results": results,
    });
    if let Some(taxonomy) = cwe_taxonomy(&cwes) {
        run["taxonomies"] = serde_json::json!([taxonomy]);
        run["tool"]["driver"]["supportedTaxonomies"] = serde_json::json!([
            { "name": "CWE", "index": 0 }
        ]);
    }
    // Gate verdict rides as a RUN-level property, never on result severity.
    // Result levels come from candidate review-priority severity and deliberately
    // avoid `error`, so GHAS does not frame candidates as confirmed problems.
    if let Some(gate) = &output.gate
        && let Ok(gate_value) = serde_json::to_value(gate)
    {
        run["properties"] = serde_json::json!({ "fallowGate": gate_value });
    }

    let sarif = serde_json::json!({
        "version": "2.1.0",
        "$schema": "https://json.schemastore.org/sarif-2.1.0.json",
        "runs": [run],
    });
    serde_json::to_string_pretty(&sarif)
        .unwrap_or_else(|_| "{\"error\":\"failed to serialize sarif\"}".to_owned())
}

/// Small FNV-1a hex digest for SARIF `partialFingerprints` dedup stability.
fn fnv_hex(input: &str) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in input.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{hash:016x}")
}

/// Stable per-finding correlation id: FNV-1a hex of `rule:path:line`. The single
/// source of truth for BOTH the JSON `finding_id` field and the SARIF
/// `partialFingerprints` value, so an agent can join the two and they never
/// drift. Computed on the project-relative path, so it must run after the
/// finding is relativized (issue #900).
fn security_finding_id(finding: &SecurityFinding) -> String {
    let fp = format!(
        "{}:{}:{}",
        sarif_rule_id(finding),
        finding.path.to_string_lossy().replace('\\', "/"),
        finding.line,
    );
    fnv_hex(&fp)
}

fn sarif_location(path: &Path, line: u32, col: u32) -> serde_json::Value {
    serde_json::json!({
        "physicalLocation": {
            "artifactLocation": { "uri": path.to_string_lossy().replace('\\', "/") },
            "region": { "startLine": line.max(1), "startColumn": col.saturating_add(1) }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use fallow_core::results::{
        SecurityCandidate, SecurityCandidateBoundary, SecurityCandidateSink,
        SecurityDeadCodeContext, SecurityDeadCodeKind, SecurityFinding, SecurityFindingKind,
        TraceHop, TraceHopRole,
    };
    use fallow_types::results::{
        SecurityReachability, SecurityTaintFlow, TaintEndpoint, TaintPath,
    };

    /// Build a finding anchored under `root` with a three-hop client -> secret trace.
    fn sample_finding(root: &Path) -> SecurityFinding {
        SecurityFinding {
            kind: SecurityFindingKind::ClientServerLeak,
            path: root.join("src/app.tsx"),
            line: 12,
            col: 3,
            evidence: "reaches process.env.SECRET_KEY".to_owned(),
            source_backed: false,
            source_read: None,
            severity: SecuritySeverity::High,
            trace: vec![
                TraceHop {
                    path: root.join("src/app.tsx"),
                    line: 12,
                    col: 3,
                    role: TraceHopRole::ClientBoundary,
                },
                TraceHop {
                    path: root.join("src/lib/util.ts"),
                    line: 4,
                    col: 0,
                    role: TraceHopRole::Intermediate,
                },
                TraceHop {
                    path: root.join("src/lib/secret.ts"),
                    line: 8,
                    col: 2,
                    role: TraceHopRole::SecretSource,
                },
            ],
            actions: vec![],
            category: None,
            cwe: None,
            dead_code: None,
            reachability: None,
            finding_id: String::new(),
            candidate: SecurityCandidate {
                source_kind: None,
                sink: SecurityCandidateSink {
                    path: root.join("src/app.tsx"),
                    line: 12,
                    col: 3,
                    category: None,
                    cwe: None,
                    callee: None,
                },
                boundary: SecurityCandidateBoundary {
                    client_server: true,
                    cross_module: false,
                    architecture_zone: None,
                },
                network: None,
            },
            taint_flow: None,
            runtime: None,
            attack_surface: None,
        }
    }

    fn output_with(findings: Vec<SecurityFinding>, unresolved_edge_files: usize) -> SecurityOutput {
        SecurityOutput {
            schema_version: SecuritySchemaVersion::V4,
            version: ToolVersion("test".to_string()),
            elapsed_ms: ElapsedMs(0),
            config: test_output_config(),
            meta: None,
            gate: None,
            security_findings: findings,
            attack_surface: None,
            unresolved_edge_files,
            unresolved_callee_sites: 0,
            unresolved_callee_diagnostics: None,
        }
    }

    fn output_with_gate(verdict: SecurityGateVerdict, new_count: usize) -> SecurityOutput {
        SecurityOutput {
            schema_version: SecuritySchemaVersion::V4,
            version: ToolVersion("test".to_string()),
            elapsed_ms: ElapsedMs(0),
            config: test_output_config(),
            meta: None,
            gate: Some(SecurityGate {
                mode: SecurityGateMode::New,
                verdict,
                new_count,
            }),
            security_findings: vec![],
            attack_surface: None,
            unresolved_edge_files: 0,
            unresolved_callee_sites: 0,
            unresolved_callee_diagnostics: None,
        }
    }

    fn sample_unresolved_callee_diagnostics(root: &Path) -> SecurityUnresolvedCalleeDiagnostics {
        unresolved_callee_diagnostics(
            &[
                SecurityUnresolvedCalleeDiagnostic {
                    path: root.join("src/z.ts"),
                    line: 9,
                    col: 4,
                    reason: fallow_types::extract::SkippedSecurityCalleeReason::ComputedMember,
                    expression_kind:
                        fallow_types::extract::SkippedSecurityCalleeExpressionKind::ComputedMemberExpression,
                },
                SecurityUnresolvedCalleeDiagnostic {
                    path: root.join("src/a.ts"),
                    line: 3,
                    col: 2,
                    reason: fallow_types::extract::SkippedSecurityCalleeReason::DynamicDispatch,
                    expression_kind: fallow_types::extract::SkippedSecurityCalleeExpressionKind::Other,
                },
                SecurityUnresolvedCalleeDiagnostic {
                    path: root.join("src/a.ts"),
                    line: 4,
                    col: 2,
                    reason: fallow_types::extract::SkippedSecurityCalleeReason::DynamicDispatch,
                    expression_kind: fallow_types::extract::SkippedSecurityCalleeExpressionKind::Other,
                },
            ],
            root,
        )
        .expect("diagnostics summarized")
    }

    fn test_output_config() -> SecurityOutputConfig {
        SecurityOutputConfig {
            rules: SecurityOutputRulesConfig {
                security_client_server_leak: SecurityRuleSeverityConfig {
                    configured: Severity::Off,
                    effective: Severity::Warn,
                },
                security_sink: SecurityRuleSeverityConfig {
                    configured: Severity::Off,
                    effective: Severity::Warn,
                },
            },
            categories_include: None,
            categories_exclude: None,
        }
    }

    fn tainted_with_runtime(root: &Path, state: Option<SecurityRuntimeState>) -> SecurityFinding {
        let mut finding = sample_finding(root);
        finding.kind = SecurityFindingKind::TaintedSink;
        finding.category = Some("dangerous-html".to_owned());
        finding.cwe = Some(79);
        finding.runtime = state.map(|state| SecurityRuntimeContext {
            state,
            function: "render".to_owned(),
            line: 10,
            invocations: Some(123),
            stable_id: Some("fallow:fn:test".to_owned()),
            evidence: Some("production runtime evidence".to_owned()),
        });
        finding
    }

    #[test]
    fn runtime_rank_promotes_hot_and_demotes_never_executed() {
        let root = Path::new("/proj/root");
        let mut findings = [
            tainted_with_runtime(root, Some(SecurityRuntimeState::NeverExecuted)),
            tainted_with_runtime(root, None),
            tainted_with_runtime(root, Some(SecurityRuntimeState::RuntimeHot)),
            tainted_with_runtime(root, Some(SecurityRuntimeState::CoverageUnavailable)),
        ];

        findings.sort_by_key(runtime_rank);

        assert_eq!(
            findings
                .iter()
                .map(|finding| finding.runtime.as_ref().map(|runtime| runtime.state))
                .collect::<Vec<_>>(),
            vec![
                Some(SecurityRuntimeState::RuntimeHot),
                None,
                Some(SecurityRuntimeState::CoverageUnavailable),
                Some(SecurityRuntimeState::NeverExecuted),
            ]
        );
    }

    #[test]
    fn severity_sort_orders_tiers_then_location() {
        let root = Path::new("/proj/root");
        let mut high = sample_finding(root);
        high.path = root.join("z.ts");
        high.severity = SecuritySeverity::High;
        let mut low = sample_finding(root);
        low.path = root.join("a.ts");
        low.severity = SecuritySeverity::Low;
        let mut medium_a = sample_finding(root);
        medium_a.path = root.join("a.ts");
        medium_a.severity = SecuritySeverity::Medium;
        medium_a.reachability = Some(fallow_types::results::SecurityReachability {
            reachable_from_entry: false,
            reachable_from_untrusted_source: true,
            taint_confidence: Some(TaintConfidence::ModuleLevel),
            untrusted_source_hop_count: Some(1),
            untrusted_source_trace: vec![],
            blast_radius: 10,
            crosses_boundary: false,
        });
        let mut medium_b = sample_finding(root);
        medium_b.path = root.join("b.ts");
        medium_b.severity = SecuritySeverity::Medium;
        medium_b.source_backed = true;
        medium_b.reachability = Some(fallow_types::results::SecurityReachability {
            reachable_from_entry: false,
            reachable_from_untrusted_source: true,
            taint_confidence: Some(TaintConfidence::ArgLevel),
            untrusted_source_hop_count: Some(0),
            untrusted_source_trace: vec![],
            blast_radius: 1,
            crosses_boundary: false,
        });
        let mut findings = vec![low, medium_b, high, medium_a];

        sort_by_security_severity(&mut findings);

        assert_eq!(
            findings
                .iter()
                .map(|finding| (finding.severity, finding.path.file_name().unwrap()))
                .collect::<Vec<_>>(),
            vec![
                (SecuritySeverity::High, std::ffi::OsStr::new("z.ts")),
                (SecuritySeverity::Medium, std::ffi::OsStr::new("b.ts")),
                (SecuritySeverity::Medium, std::ffi::OsStr::new("a.ts")),
                (SecuritySeverity::Low, std::ffi::OsStr::new("a.ts")),
            ]
        );
    }

    #[test]
    fn human_render_includes_runtime_context_line() {
        let root = Path::new("/proj/root");
        let finding = relativize_finding(
            tainted_with_runtime(root, Some(SecurityRuntimeState::RuntimeHot)),
            root,
        );
        let out = render_human(&output_with(vec![finding], 0));

        assert!(
            out.contains("runtime: runtime-hot in render:10"),
            "got: {out}"
        );
        assert!(out.contains("production runtime evidence"), "got: {out}");
    }

    #[test]
    fn sarif_render_includes_runtime_context_in_message() {
        let root = Path::new("/proj/root");
        let finding = relativize_finding(
            tainted_with_runtime(root, Some(SecurityRuntimeState::RuntimeHot)),
            root,
        );
        let rendered = render_sarif(&output_with(vec![finding], 0));
        let sarif: serde_json::Value = serde_json::from_str(&rendered).expect("valid SARIF JSON");
        let message = sarif["runs"][0]["results"][0]["message"]["text"]
            .as_str()
            .expect("message text");

        assert!(message.contains("Runtime context"), "got: {message}");
        assert!(
            message.contains("runtime-hot in render:10"),
            "got: {message}"
        );
    }

    #[test]
    fn gate_human_header_fail_says_review_required_not_fail() {
        let gate = SecurityGate {
            mode: SecurityGateMode::New,
            verdict: SecurityGateVerdict::Fail,
            new_count: 2,
        };
        let header = gate_human_header(&gate);
        assert!(header.contains("REVIEW REQUIRED"));
        assert!(header.contains("2 new security items"));
        assert!(header.contains("not confirmed a vulnerability"));
        assert!(!header.to_uppercase().contains("GATE: FAIL"));
    }

    #[test]
    fn gate_human_header_fail_singular_for_one_candidate() {
        // The gate makes new_count == 1 the common case (one PR adds one sink).
        let gate = SecurityGate {
            mode: SecurityGateMode::New,
            verdict: SecurityGateVerdict::Fail,
            new_count: 1,
        };
        let header = gate_human_header(&gate);
        assert!(header.contains("1 new security item in changed lines"));
        assert!(!header.contains("1 new security candidates"));
    }

    #[test]
    fn gate_human_header_pass() {
        let gate = SecurityGate {
            mode: SecurityGateMode::New,
            verdict: SecurityGateVerdict::Pass,
            new_count: 0,
        };
        assert!(gate_human_header(&gate).contains("Gate: PASS"));
    }

    #[test]
    fn gate_json_block_is_snake_case_and_present_on_pass() {
        let json = render_json(&output_with_gate(SecurityGateVerdict::Pass, 0));
        assert!(json.contains("\"gate\""));
        assert!(json.contains("\"mode\": \"new\""));
        assert!(json.contains("\"verdict\": \"pass\""));
        assert!(json.contains("\"new_count\": 0"));
    }

    #[test]
    fn reachability_key_includes_path_kind_and_category() {
        let root = Path::new("/proj/root");
        let mut leak = sample_finding(root);
        leak.reachability = Some(SecurityReachability {
            reachable_from_entry: true,
            reachable_from_untrusted_source: false,
            taint_confidence: None,
            untrusted_source_hop_count: None,
            untrusted_source_trace: vec![],
            blast_radius: 0,
            crosses_boundary: false,
        });
        let mut sink = leak.clone();
        sink.kind = SecurityFindingKind::TaintedSink;
        sink.category = Some("dangerous-html".to_owned());

        assert_eq!(
            security_reachability_key(&leak, root).as_deref(),
            Some("security-reach:src/app.tsx:client-server-leak:none")
        );
        assert_eq!(
            security_reachability_key(&sink, root).as_deref(),
            Some("security-reach:src/app.tsx:tainted-sink:dangerous-html")
        );
    }

    #[test]
    fn reachability_key_ignores_unreachable_findings() {
        let root = Path::new("/proj/root");
        let finding = sample_finding(root);

        assert!(security_reachability_key(&finding, root).is_none());
    }

    #[test]
    fn gate_absent_from_json_when_no_gate_ran() {
        let json = render_json(&output_with(vec![], 0));
        assert!(!json.contains("\"gate\""));
    }

    #[test]
    fn gate_sarif_is_a_run_property_not_result_severity() {
        let sarif = render_sarif(&output_with_gate(SecurityGateVerdict::Fail, 1));
        assert!(sarif.contains("fallowGate"));
        // The gate verdict is a run property and creates no result severity.
        assert!(!sarif.contains("\"level\": \"error\""));
        assert!(!sarif.contains("\"level\": \"warning\""));
    }

    fn add_untrusted_source_reachability(finding: &mut SecurityFinding, root: &Path) {
        finding.reachability = Some(SecurityReachability {
            reachable_from_entry: true,
            reachable_from_untrusted_source: true,
            // Cross-module reachability is module-level (issue #1093).
            taint_confidence: Some(fallow_core::results::TaintConfidence::ModuleLevel),
            untrusted_source_hop_count: Some(1),
            untrusted_source_trace: vec![
                TraceHop {
                    path: root.join("src/routes/api.ts"),
                    line: 3,
                    col: 0,
                    role: TraceHopRole::ModuleSource,
                },
                TraceHop {
                    path: root.join("src/lib/sink.ts"),
                    line: 9,
                    col: 2,
                    role: TraceHopRole::Sink,
                },
            ],
            blast_radius: 2,
            crosses_boundary: false,
        });
    }

    fn add_taint_flow(finding: &mut SecurityFinding, root: &Path) {
        finding.taint_flow = Some(SecurityTaintFlow {
            source: TaintEndpoint {
                path: root.join("src/routes/api.ts"),
                line: 3,
                col: 0,
            },
            sink: TaintEndpoint {
                path: root.join("src/lib/sink.ts"),
                line: 9,
                col: 2,
            },
            path: TaintPath {
                intra_module: false,
                cross_module_hops: 1,
            },
        });
    }

    #[test]
    fn relativize_strips_root_prefix() {
        let root = Path::new("/proj/root");
        let abs = root.join("src/app.tsx");
        let rel = relativize(&abs, root);
        assert_eq!(rel.to_string_lossy().replace('\\', "/"), "src/app.tsx");
    }

    #[test]
    fn relativize_keeps_path_when_outside_root() {
        let root = Path::new("/proj/root");
        let outside = Path::new("/elsewhere/file.ts");
        // Not under root: the original path is returned unchanged.
        assert_eq!(relativize(outside, root), outside.to_path_buf());
    }

    #[test]
    fn relativize_finding_relativizes_anchor_and_every_hop() {
        let root = Path::new("/proj/root");
        let finding = relativize_finding(sample_finding(root), root);
        assert_eq!(
            finding.path.to_string_lossy().replace('\\', "/"),
            "src/app.tsx"
        );
        let hop_paths: Vec<String> = finding
            .trace
            .iter()
            .map(|h| h.path.to_string_lossy().replace('\\', "/"))
            .collect();
        assert_eq!(
            hop_paths,
            vec!["src/app.tsx", "src/lib/util.ts", "src/lib/secret.ts"]
        );
    }

    #[test]
    fn relativize_finding_relativizes_untrusted_source_trace() {
        let root = Path::new("/proj/root");
        let mut finding = sample_finding(root);
        add_untrusted_source_reachability(&mut finding, root);
        let finding = relativize_finding(finding, root);
        let reach = finding.reachability.as_ref().expect("reachability");
        let hop_paths: Vec<String> = reach
            .untrusted_source_trace
            .iter()
            .map(|h| h.path.to_string_lossy().replace('\\', "/"))
            .collect();
        assert_eq!(hop_paths, vec!["src/routes/api.ts", "src/lib/sink.ts"]);
    }

    #[test]
    fn fnv_hex_is_deterministic_and_16_hex_digits() {
        let a = fnv_hex("security/client-server-leak:src/app.tsx:12");
        let b = fnv_hex("security/client-server-leak:src/app.tsx:12");
        assert_eq!(a, b, "same input must hash identically");
        assert_eq!(a.len(), 16);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
        // Distinct input yields a distinct digest (anchor line differs).
        assert_ne!(a, fnv_hex("security/client-server-leak:src/app.tsx:13"));
    }

    #[test]
    fn hop_role_labels_cover_every_role() {
        assert_eq!(
            hop_role_label(TraceHopRole::ClientBoundary),
            "client boundary"
        );
        assert_eq!(
            hop_role_label(TraceHopRole::UntrustedSource),
            "untrusted source"
        );
        assert_eq!(hop_role_label(TraceHopRole::ModuleSource), "source module");
        assert_eq!(hop_role_label(TraceHopRole::Intermediate), "intermediate");
        assert_eq!(hop_role_label(TraceHopRole::SecretSource), "secret source");
        assert_eq!(hop_role_label(TraceHopRole::Sink), "sink site");
    }

    #[test]
    fn sarif_location_clamps_line_and_offsets_column() {
        // A zero line clamps to 1; the 0-based column becomes 1-based.
        let loc = sarif_location(Path::new("a\\b.ts"), 0, 0);
        let region = &loc["physicalLocation"]["region"];
        assert_eq!(region["startLine"], 1);
        assert_eq!(region["startColumn"], 1);
        // Backslash separators normalize to forward slashes in the URI.
        assert_eq!(loc["physicalLocation"]["artifactLocation"]["uri"], "a/b.ts");
    }

    #[test]
    fn human_summary_reports_zero_without_edge_line() {
        let out = render_human_summary(&output_with(vec![], 0));
        assert!(
            out.contains("Security review: no items to check in the scanned code."),
            "got: {out}"
        );
        assert!(!out.contains("Blind spot"));
    }

    #[test]
    fn human_summary_pluralizes_and_surfaces_unresolved_edges() {
        let root = Path::new("/proj/root");
        let out = render_human_summary(&output_with(vec![sample_finding(root)], 2));
        assert!(
            out.contains("Security review: 1 item to check."),
            "got: {out}"
        );
        assert!(out.contains("not confirmed vulnerabilities"));
        assert!(out.contains("unsafe input, secrets, or settings"));
        assert!(out.contains("Blind spot: 2 client files use dynamic imports"));
    }

    #[test]
    fn human_render_empty_states_no_candidates() {
        colored::control::set_override(false);
        let out = render_human(&output_with(vec![], 0));
        assert!(out.contains("Security review: 0 items to check"));
        assert!(out.contains("No security details to show."));
        assert!(out.contains("Result: 0 security items to check."));
    }

    #[test]
    fn human_render_shows_finding_trace_and_next_action() {
        colored::control::set_override(false);
        let root = Path::new("/proj/root");
        let finding = relativize_finding(sample_finding(root), root);
        let out = render_human(&output_with(vec![finding], 0));
        assert!(out.contains("[H] high client-server-leak"));
        assert!(out.contains("client-server-leak"));
        assert!(out.contains("src/app.tsx:12"));
        assert!(out.contains("evidence: reaches process.env.SECRET_KEY"));
        assert!(out.contains("import trace:"));
        assert!(out.contains("src/lib/secret.ts:8 (secret source)"));
        assert!(out.contains("src/app.tsx:12 (client boundary)"));
        assert!(out.contains("Next: check whether this import can ship a secret to the browser"));
        assert!(out.contains("Result: 1 security item to check."));
    }

    #[test]
    fn human_render_shows_dead_code_hint_and_delete_next_step() {
        colored::control::set_override(false);
        let root = Path::new("/proj/root");
        let mut finding = relativize_finding(sample_finding(root), root);
        finding.kind = SecurityFindingKind::TaintedSink;
        finding.dead_code = Some(SecurityDeadCodeContext {
            kind: SecurityDeadCodeKind::UnusedFile,
            export_name: None,
            line: None,
            guidance: "delete instead of harden".to_string(),
        });
        let out = render_human(&output_with(vec![finding], 0));
        assert!(
            out.contains("dead-code: also reported as unused-file"),
            "got: {out}"
        );
        assert!(
            out.contains("If the code is safe to remove, delete it"),
            "got: {out}"
        );
    }

    #[test]
    fn human_render_shows_untrusted_source_path_as_module_context() {
        colored::control::set_override(false);
        let root = Path::new("/proj/root");
        let mut finding = sample_finding(root);
        finding.kind = SecurityFindingKind::TaintedSink;
        finding.category = Some("command-injection".to_string());
        add_untrusted_source_reachability(&mut finding, root);
        let finding = relativize_finding(finding, root);

        let out = render_human(&output_with(vec![finding], 0));

        assert!(
            out.contains("reachable from a module that receives untrusted input via 1 import hop"),
            "got: {out}"
        );
        assert!(out.contains("input import trace:"), "got: {out}");
        assert!(
            out.contains("src/routes/api.ts:3 (source module)"),
            "got: {out}"
        );
    }

    #[test]
    fn human_render_surfaces_unresolved_edge_blind_spot() {
        colored::control::set_override(false);
        let out = render_human(&output_with(vec![], 3));
        assert!(out.contains("Blind spot: 3 client files use dynamic imports"));
        assert!(out.contains("Code behind those imports may be missing from this report."));
    }

    #[test]
    fn human_render_blind_spots_use_singular_verbs() {
        colored::control::set_override(false);
        let mut output = output_with(vec![], 1);
        output.unresolved_callee_sites = 1;

        let out = render_human(&output);

        assert!(out.contains("Blind spot: 1 client file uses dynamic imports"));
        assert!(out.contains("Blind spot: 1 call site uses code patterns"));
    }

    #[test]
    fn human_render_mentions_top_unresolved_callee_reason_and_file() {
        colored::control::set_override(false);
        let root = Path::new("/proj/root");
        let mut output = output_with(vec![], 0);
        output.unresolved_callee_sites = 3;
        output.unresolved_callee_diagnostics = Some(sample_unresolved_callee_diagnostics(root));

        let out = render_human(&output);

        assert!(
            out.contains("Most unresolved callees: dynamic-dispatch in src/a.ts."),
            "got: {out}"
        );
    }

    #[test]
    fn json_render_carries_schema_version_and_findings() {
        let root = Path::new("/proj/root");
        let finding = relativize_finding(sample_finding(root), root);
        let rendered = render_json(&output_with(vec![finding], 1));
        let value: serde_json::Value = serde_json::from_str(&rendered).expect("valid JSON");
        assert_eq!(value["schema_version"], "4");
        assert_eq!(value["version"], "test");
        assert_eq!(value["elapsed_ms"], 0);
        assert_eq!(
            value["config"]["rules"]["security_client_server_leak"]["configured"],
            "off"
        );
        assert_eq!(
            value["config"]["rules"]["security_client_server_leak"]["effective"],
            "warn"
        );
        assert!(value["config"]["categories_include"].is_null());
        assert!(value["config"]["categories_exclude"].is_null());
        assert_eq!(value["unresolved_edge_files"], 1);
        let findings = value["security_findings"].as_array().expect("array");
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0]["kind"], "client-server-leak");
        assert_eq!(findings[0]["path"], "src/app.tsx");
        assert_eq!(findings[0]["severity"], "high");
    }

    #[test]
    fn json_render_carries_bounded_unresolved_callee_diagnostics() {
        let root = Path::new("/proj/root");
        let mut output = output_with(vec![], 0);
        output.unresolved_callee_sites = 3;
        output.unresolved_callee_diagnostics = Some(sample_unresolved_callee_diagnostics(root));

        let rendered = render_json(&output);
        let value: serde_json::Value = serde_json::from_str(&rendered).expect("valid JSON");
        let diagnostics = &value["unresolved_callee_diagnostics"];

        assert_eq!(diagnostics["sample_limit"], 25);
        assert_eq!(diagnostics["top_files_limit"], 10);
        assert_eq!(diagnostics["sampled"][0]["path"], "src/a.ts");
        assert_eq!(diagnostics["sampled"][0]["reason"], "dynamic-dispatch");
        assert_eq!(diagnostics["sampled"][0]["expression_kind"], "other");
        assert_eq!(diagnostics["top_files"][0]["path"], "src/a.ts");
        assert_eq!(diagnostics["top_files"][0]["count"], 2);
        assert_eq!(diagnostics["by_reason"][0]["reason"], "dynamic-dispatch");
        assert_eq!(diagnostics["by_reason"][0]["count"], 2);
    }

    #[test]
    fn json_summary_omits_finding_arrays_and_counts_security_findings() {
        let root = Path::new("/proj/root");
        let mut leak = relativize_finding(sample_finding(root), root);
        leak.severity = SecuritySeverity::High;

        let mut sink = relativize_finding(sample_finding(root), root);
        sink.kind = SecurityFindingKind::TaintedSink;
        sink.category = Some("dangerous-html".to_string());
        sink.severity = SecuritySeverity::Medium;
        sink.source_backed = true;
        sink.reachability = Some(SecurityReachability {
            reachable_from_entry: true,
            reachable_from_untrusted_source: true,
            taint_confidence: Some(TaintConfidence::ArgLevel),
            untrusted_source_hop_count: Some(0),
            untrusted_source_trace: vec![],
            blast_radius: 3,
            crosses_boundary: true,
        });
        sink.runtime = Some(SecurityRuntimeContext {
            state: SecurityRuntimeState::RuntimeHot,
            function: "render".to_owned(),
            line: 10,
            invocations: Some(120),
            stable_id: Some("src/app.tsx::render:10".to_owned()),
            evidence: Some("production hot path observed".to_owned()),
        });

        let mut output = output_with(vec![leak, sink], 2);
        output.elapsed_ms = ElapsedMs(17);
        output.unresolved_callee_sites = 3;

        let rendered = render_json_summary(&output);
        let value: serde_json::Value = serde_json::from_str(&rendered).expect("valid JSON");

        assert_eq!(value["kind"], "security");
        assert_eq!(value["schema_version"], "4");
        assert_eq!(value["version"], "test");
        assert_eq!(value["elapsed_ms"], 17);
        assert!(value.get("config").is_some());
        assert!(value.get("security_findings").is_none());
        assert!(value.get("attack_surface").is_none());
        assert!(value.get("_meta").is_none());
        assert_eq!(value["summary"]["security_findings"], 2);
        assert_eq!(value["summary"]["by_severity"]["high"], 1);
        assert_eq!(value["summary"]["by_severity"]["medium"], 1);
        assert_eq!(value["summary"]["by_severity"]["low"], 0);
        assert_eq!(value["summary"]["by_category"]["client-server-leak"], 1);
        assert_eq!(value["summary"]["by_category"]["dangerous-html"], 1);
        assert_eq!(value["summary"]["by_reachability"]["entry_reachable"], 1);
        assert_eq!(
            value["summary"]["by_reachability"]["untrusted_source_reachable"],
            1
        );
        assert_eq!(value["summary"]["by_reachability"]["arg_level"], 1);
        assert_eq!(value["summary"]["by_reachability"]["module_level"], 0);
        assert_eq!(value["summary"]["by_reachability"]["crosses_boundary"], 1);
        assert_eq!(value["summary"]["by_reachability"]["source_backed"], 1);
        assert_eq!(value["summary"]["by_runtime_state"]["runtime_hot"], 1);
        assert_eq!(value["summary"]["by_runtime_state"]["runtime_cold"], 0);
        assert_eq!(value["summary"]["by_runtime_state"]["never_executed"], 0);
        assert_eq!(value["summary"]["by_runtime_state"]["low_traffic"], 0);
        assert_eq!(
            value["summary"]["by_runtime_state"]["coverage_unavailable"],
            0
        );
        assert_eq!(value["summary"]["by_runtime_state"]["runtime_unknown"], 0);
        assert_eq!(value["summary"]["by_runtime_state"]["not_collected"], 1);
        assert_eq!(value["summary"]["unresolved_edge_files"], 2);
        assert_eq!(value["summary"]["unresolved_callee_sites"], 3);
        assert_eq!(value["summary"]["attack_surface_entries"], 0);
    }

    #[test]
    fn json_summary_carries_security_meta_when_explain_requested() {
        let root = Path::new("/proj/root");
        let mut output = output_with(vec![relativize_finding(sample_finding(root), root)], 0);
        output.meta = Some(crate::explain::security_meta());

        let rendered = render_json_summary(&output);
        let value: serde_json::Value = serde_json::from_str(&rendered).expect("valid JSON");

        assert!(value.get("security_findings").is_none());
        assert!(value["_meta"]["field_definitions"]["security_findings[]"].is_string());
        assert!(value["_meta"]["field_definitions"]["summary.by_reachability"].is_string());
        assert!(value["_meta"]["field_definitions"]["summary.by_runtime_state"].is_string());
        assert!(value["_meta"]["field_definitions"]["unresolved_callee_sites"].is_string());
    }

    #[test]
    fn json_summary_preserves_gate_block() {
        let output = output_with_gate(SecurityGateVerdict::Fail, 1);
        let rendered = render_json_summary(&output);
        let value: serde_json::Value = serde_json::from_str(&rendered).expect("valid JSON");

        assert_eq!(value["kind"], "security");
        assert_eq!(value["gate"]["mode"], "new");
        assert_eq!(value["gate"]["verdict"], "fail");
        assert_eq!(value["gate"]["new_count"], 1);
        assert_eq!(value["summary"]["security_findings"], 0);
    }

    #[test]
    fn json_render_carries_security_meta_when_explain_requested() {
        let mut output = output_with(vec![], 0);
        output.meta = Some(crate::explain::security_meta());

        let rendered = render_json(&output);
        let value: serde_json::Value = serde_json::from_str(&rendered).expect("valid JSON");

        assert_eq!(
            value["_meta"]["field_definitions"]["security_findings[]"],
            "Unverified security candidates for downstream human or agent verification."
        );
        assert!(value["_meta"]["rules"]["security/tainted-sink"].is_object());
    }

    #[test]
    fn json_render_carries_candidate_record_and_omits_impact() {
        // Issue #900: every finding carries a 3-slot candidate record; there is
        // NO `impact` key on the wire (agent-owned, documented in the schema). A
        // client-server-leak has no source kind and no taint flow.
        let root = Path::new("/proj/root");
        let finding = relativize_finding(sample_finding(root), root);
        let rendered = render_json(&output_with(vec![finding], 0));
        let value: serde_json::Value = serde_json::from_str(&rendered).expect("valid JSON");
        let finding = &value["security_findings"][0];

        let candidate = &finding["candidate"];
        assert!(candidate.is_object(), "candidate record present");
        assert!(candidate["sink"].is_object(), "sink slot present");
        assert_eq!(candidate["boundary"]["client_server"], true);
        assert!(
            candidate.get("impact").is_none(),
            "impact must NOT be a wire field"
        );
        assert!(
            candidate.get("source_kind").is_none(),
            "client-server-leak has no source kind"
        );
        assert!(
            finding.get("taint_flow").is_none(),
            "no untrusted-source flow on a client-server-leak"
        );
        assert!(
            finding.get("finding_id").is_some(),
            "finding_id is on the wire"
        );
    }

    #[test]
    fn finding_id_is_stable_and_matches_sarif_fingerprint() {
        // Issue #900: one helper computes both the JSON finding_id and the SARIF
        // partialFingerprint, so an agent can join the two and they never drift.
        let root = Path::new("/proj/root");
        let finding = relativize_finding(sample_finding(root), root);
        let id = security_finding_id(&finding);
        assert!(!id.is_empty());
        assert_eq!(
            id,
            security_finding_id(&finding),
            "deterministic across calls"
        );

        let sarif: serde_json::Value =
            serde_json::from_str(&render_sarif(&output_with(vec![finding], 0)))
                .expect("valid SARIF");
        assert_eq!(
            sarif["runs"][0]["results"][0]["partialFingerprints"]["fallowSecurity/v1"],
            serde_json::Value::String(id)
        );
    }

    #[test]
    fn json_render_carries_dead_code_context() {
        let root = Path::new("/proj/root");
        let mut finding = relativize_finding(sample_finding(root), root);
        finding.kind = SecurityFindingKind::TaintedSink;
        finding.dead_code = Some(SecurityDeadCodeContext {
            kind: SecurityDeadCodeKind::UnusedExport,
            export_name: Some("handler".to_string()),
            line: Some(12),
            guidance: "remove export instead of harden".to_string(),
        });
        let rendered = render_json(&output_with(vec![finding], 0));
        let value: serde_json::Value = serde_json::from_str(&rendered).expect("valid JSON");
        let context = &value["security_findings"][0]["dead_code"];
        assert_eq!(context["kind"], "unused-export");
        assert_eq!(context["export_name"], "handler");
        assert_eq!(context["line"], 12);
    }

    #[test]
    fn sarif_render_emits_warning_level_with_fingerprint_and_related_locations() {
        let root = Path::new("/proj/root");
        let finding = relativize_finding(sample_finding(root), root);
        let rendered = render_sarif(&output_with(vec![finding], 0));
        let sarif: serde_json::Value = serde_json::from_str(&rendered).expect("valid SARIF JSON");
        assert_eq!(sarif["version"], "2.1.0");
        let run = &sarif["runs"][0];
        assert_eq!(run["tool"]["driver"]["name"], "fallow");
        let result = &run["results"][0];
        // Candidate framing: a high-priority finding is warning, never error.
        assert_eq!(result["level"], "warning");
        assert_eq!(result["ruleId"], "security/client-server-leak");
        assert_eq!(result["message"]["text"], "reaches process.env.SECRET_KEY");
        // Trace hops surface as relatedLocations and codeFlows.
        assert_eq!(result["relatedLocations"].as_array().unwrap().len(), 3);
        let flow_locations = result["codeFlows"][0]["threadFlows"][0]["locations"]
            .as_array()
            .expect("thread flow locations");
        assert_eq!(flow_locations.len(), 3);
        assert_eq!(
            flow_locations[0]["location"]["physicalLocation"]["artifactLocation"]["uri"],
            "src/app.tsx"
        );
        assert_eq!(
            flow_locations[2]["location"]["physicalLocation"]["artifactLocation"]["uri"],
            "src/lib/secret.ts"
        );
        assert_eq!(
            flow_locations[2]["kinds"][0],
            serde_json::json!("secret-source")
        );
        // Stable dedup fingerprint present for GHAS.
        assert!(result["partialFingerprints"]["fallowSecurity/v1"].is_string());

        let rules = run["tool"]["driver"]["rules"].as_array().unwrap();
        assert_eq!(rules[0]["name"], "Client-server secret leak");
        assert!(rules[0]["help"]["text"].is_string());
        assert!(rules[0].get("relationships").is_none());
        assert!(run.get("taxonomies").is_none());
    }

    #[test]
    fn sarif_render_keeps_low_severity_as_note() {
        let root = Path::new("/proj/root");
        let mut finding = sample_finding(root);
        finding.severity = SecuritySeverity::Low;
        let rendered = render_sarif(&output_with(vec![relativize_finding(finding, root)], 0));
        let sarif: serde_json::Value = serde_json::from_str(&rendered).expect("valid SARIF JSON");

        assert_eq!(sarif["runs"][0]["results"][0]["level"], "note");
    }

    #[test]
    fn sarif_render_includes_dead_code_hint_in_message() {
        let root = Path::new("/proj/root");
        let mut finding = relativize_finding(sample_finding(root), root);
        finding.kind = SecurityFindingKind::TaintedSink;
        finding.dead_code = Some(SecurityDeadCodeContext {
            kind: SecurityDeadCodeKind::UnusedFile,
            export_name: None,
            line: None,
            guidance: "delete instead of harden".to_string(),
        });
        let rendered = render_sarif(&output_with(vec![finding], 0));
        let sarif: serde_json::Value = serde_json::from_str(&rendered).expect("valid SARIF JSON");
        let message = sarif["runs"][0]["results"][0]["message"]["text"]
            .as_str()
            .expect("message text");
        assert!(message.contains("Dead-code cross-link"), "got: {message}");
        assert!(
            message.contains("delete this file instead of hardening"),
            "got: {message}"
        );
    }

    #[test]
    fn sarif_render_includes_untrusted_source_context_and_related_locations() {
        let root = Path::new("/proj/root");
        let mut finding = sample_finding(root);
        finding.kind = SecurityFindingKind::TaintedSink;
        finding.category = Some("command-injection".to_string());
        add_untrusted_source_reachability(&mut finding, root);
        add_taint_flow(&mut finding, root);
        finding.trace.push(TraceHop {
            path: root.join("src/lib/sink.ts"),
            line: 9,
            col: 2,
            role: TraceHopRole::Sink,
        });
        let rendered = render_sarif(&output_with(vec![relativize_finding(finding, root)], 0));
        let sarif: serde_json::Value = serde_json::from_str(&rendered).expect("valid SARIF JSON");
        let result = &sarif["runs"][0]["results"][0];
        let message = result["message"]["text"].as_str().expect("message text");
        assert!(message.contains("Module-level context"), "got: {message}");
        assert!(
            message.contains("does not prove value flow"),
            "got: {message}"
        );
        // The sink appears in both trace families, but SARIF relatedLocations requires unique items.
        assert_eq!(result["relatedLocations"].as_array().unwrap().len(), 5);
        let flow_locations = result["codeFlows"][0]["threadFlows"][0]["locations"]
            .as_array()
            .expect("thread flow locations");
        assert_eq!(flow_locations.len(), 2);
        assert_eq!(
            flow_locations[0]["location"]["physicalLocation"]["artifactLocation"]["uri"],
            "src/routes/api.ts"
        );
        assert_eq!(
            flow_locations[1]["location"]["physicalLocation"]["artifactLocation"]["uri"],
            "src/lib/sink.ts"
        );
    }

    #[test]
    fn sarif_tainted_sink_uses_per_category_rule_id_and_cwe_metadata() {
        let root = Path::new("/proj/root");
        let mut finding = sample_finding(root);
        finding.kind = SecurityFindingKind::TaintedSink;
        finding.category = Some("dangerous-html".to_owned());
        finding.cwe = Some(79);
        let rendered = render_sarif(&output_with(vec![relativize_finding(finding, root)], 0));
        let sarif: serde_json::Value = serde_json::from_str(&rendered).expect("valid SARIF JSON");
        let run = &sarif["runs"][0];
        // The finding is grouped under its own per-category rule, not collapsed
        // into client-server-leak, and stays candidate-framed.
        let result = &run["results"][0];
        assert_eq!(result["level"], "warning");
        assert_eq!(result["ruleId"], "security/dangerous-html");
        // Exactly one rule definition, carrying compatible tags plus SARIF-native CWE taxonomy.
        let rules = run["tool"]["driver"]["rules"].as_array().unwrap();
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0]["id"], "security/dangerous-html");
        assert_eq!(rules[0]["name"], "Dangerous HTML sink");
        assert!(
            rules[0]["help"]["text"]
                .as_str()
                .expect("help text")
                .contains("Verify this unverified")
        );
        assert!(
            rules[0]["help"]["markdown"]
                .as_str()
                .expect("help markdown")
                .contains("**Dangerous HTML sink**")
        );
        let tags = rules[0]["properties"]["tags"].as_array().unwrap();
        assert!(tags.iter().any(|t| t == "external/cwe/cwe-79"));
        let relationship = &rules[0]["relationships"][0];
        assert_eq!(relationship["target"]["id"], "CWE-79");
        assert_eq!(relationship["target"]["index"], 0);
        assert_eq!(relationship["target"]["toolComponent"]["name"], "CWE");
        assert_eq!(relationship["kinds"][0], "superset");

        let taxonomy = &run["taxonomies"][0];
        assert_eq!(taxonomy["name"], "CWE");
        assert_eq!(taxonomy["taxa"][0]["id"], "CWE-79");
        assert_eq!(
            run["tool"]["driver"]["supportedTaxonomies"][0]["name"],
            "CWE"
        );
    }

    #[test]
    fn write_sarif_file_creates_parent_dir_and_writes_valid_sarif() {
        let root = Path::new("/proj/root");
        let finding = relativize_finding(sample_finding(root), root);
        let output = output_with(vec![finding], 0);
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("nested/out.sarif");
        write_sarif_file(&output, &path).expect("write succeeds and creates parent dir");
        let written = std::fs::read_to_string(&path).expect("file exists");
        let sarif: serde_json::Value = serde_json::from_str(&written).expect("valid SARIF JSON");
        assert_eq!(sarif["version"], "2.1.0");
    }

    /// No explicit `--config`; static so the `&'a Option<PathBuf>` field borrows it.
    const NO_CONFIG: Option<PathBuf> = None;

    fn leak_fixture_root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/security-client-server-leak")
    }

    fn source_reachability_fixture_root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/security-source-reachability-885")
    }

    fn run_opts(root: &Path, output: OutputFormat, fail_on_issues: bool) -> SecurityOptions<'_> {
        SecurityOptions {
            root,
            config_path: &NO_CONFIG,
            output,
            no_cache: true,
            threads: 1,
            quiet: true,
            fail_on_issues,
            sarif_file: None,
            summary: false,
            changed_since: None,
            use_shared_diff_index: false,
            workspace: None,
            changed_workspaces: None,
            file: &[],
            surface: false,
            gate: None,
            runtime_coverage: None,
            min_invocations_hot: 100,
            explain: false,
        }
    }

    #[test]
    #[expect(
        deprecated,
        reason = "CLI fixture test uses the same workspace path dependency boundary as `run`"
    )]
    fn source_reachability_fixture_marks_cross_module_sink() {
        let root = source_reachability_fixture_root();
        let mut config = load_config_for_analysis(
            &root,
            &NO_CONFIG,
            OutputFormat::Json,
            true,
            1,
            None,
            true,
            ProductionAnalysis::DeadCode,
        )
        .expect("fixture config loads");
        config.rules.security_sink = Severity::Warn;

        let results = fallow_core::analyze(&config).expect("fixture analyzes");
        let finding = results
            .security_findings
            .iter()
            .find(|finding| finding.path.ends_with("src/runner.ts"))
            .expect("runner sink finding");
        let reach = finding.reachability.as_ref().expect("reachability");

        assert!(reach.reachable_from_untrusted_source);
        assert_eq!(reach.untrusted_source_hop_count, Some(1));
        // Cross-module reachability is module-level: the structured discriminator
        // says so, and the source node is honestly labeled `ModuleSource`, never
        // `UntrustedSource` (which is reserved for an arg-level same-module read).
        assert_eq!(
            reach.taint_confidence,
            Some(fallow_core::results::TaintConfidence::ModuleLevel)
        );
        assert_eq!(
            reach
                .untrusted_source_trace
                .iter()
                .map(|hop| hop.role)
                .collect::<Vec<_>>(),
            vec![TraceHopRole::ModuleSource, TraceHopRole::Sink]
        );
        assert!(
            reach.untrusted_source_trace[0]
                .path
                .ends_with("src/route.ts")
        );

        // Issue #900: the candidate boundary slot records the cross-module hop,
        // and the taint-flow triple re-projects the reachability endpoints + a
        // compact path (not a duplicate hop array).
        assert!(
            finding.candidate.boundary.cross_module,
            "a sink reached across a module hop crosses a module boundary"
        );
        let flow = finding.taint_flow.as_ref().expect("taint_flow present");
        assert!(!flow.path.intra_module);
        assert_eq!(flow.path.cross_module_hops, 1);
        assert!(flow.source.path.ends_with("src/route.ts"));
        assert!(flow.sink.path.ends_with("src/runner.ts"));
    }

    #[test]
    fn file_scope_keeps_security_finding_when_anchor_matches() {
        let root = Path::new("/proj/root");
        let mut results = fallow_core::results::AnalysisResults::default();
        results.security_findings.push(sample_finding(root));

        filter_to_files(&mut results, root, &[PathBuf::from("src/app.tsx")], true);

        assert_eq!(results.security_findings.len(), 1);
    }

    #[test]
    fn file_scope_keeps_security_finding_when_trace_hop_matches() {
        let root = Path::new("/proj/root");
        let mut results = fallow_core::results::AnalysisResults::default();
        results.security_findings.push(sample_finding(root));

        filter_to_files(
            &mut results,
            root,
            &[PathBuf::from("src/lib/secret.ts")],
            true,
        );

        assert_eq!(results.security_findings.len(), 1);
    }

    #[test]
    fn file_scope_drops_unrelated_security_finding() {
        let root = Path::new("/proj/root");
        let mut results = fallow_core::results::AnalysisResults::default();
        results.security_findings.push(sample_finding(root));

        filter_to_files(&mut results, root, &[PathBuf::from("src/other.ts")], true);

        assert!(results.security_findings.is_empty());
    }

    #[test]
    fn run_is_advisory_and_exits_zero_even_with_candidates() {
        // The rule defaults to off; the command forces it to warn, so findings on
        // the fixture are surfaced but the exit stays 0 (advisory) by default.
        let root = leak_fixture_root();
        let code = run(&run_opts(&root, OutputFormat::Json, false));
        assert_eq!(code, ExitCode::SUCCESS);
    }

    #[test]
    fn run_with_fail_on_issues_exits_one_when_candidates_found() {
        // The fixture has real leak candidates, so --fail-on-issues raises exit 1.
        let root = leak_fixture_root();
        let code = run(&run_opts(&root, OutputFormat::Human, true));
        assert_eq!(code, ExitCode::from(1));
    }

    #[test]
    fn run_rejects_unsupported_output_format() {
        // Only human / json / sarif are supported; compact exits 2 before analysis.
        let root = leak_fixture_root();
        let code = run(&run_opts(&root, OutputFormat::Compact, false));
        assert_eq!(code, ExitCode::from(2));
    }

    #[test]
    fn run_summary_mode_dispatches_compact_human_renderer() {
        let root = leak_fixture_root();
        let opts = SecurityOptions {
            summary: true,
            ..run_opts(&root, OutputFormat::Human, false)
        };
        assert_eq!(run(&opts), ExitCode::SUCCESS);
    }

    #[test]
    fn run_sarif_format_dispatches_sarif_renderer() {
        let root = leak_fixture_root();
        assert_eq!(
            run(&run_opts(&root, OutputFormat::Sarif, false)),
            ExitCode::SUCCESS
        );
    }

    #[test]
    fn run_writes_sarif_sidecar_file_when_requested() {
        let root = leak_fixture_root();
        let dir = tempfile::tempdir().expect("tempdir");
        let sidecar = dir.path().join("security.sarif");
        let opts = SecurityOptions {
            sarif_file: Some(&sidecar),
            ..run_opts(&root, OutputFormat::Human, false)
        };
        assert_eq!(run(&opts), ExitCode::SUCCESS);
        let written = std::fs::read_to_string(&sidecar).expect("sidecar SARIF written");
        let sarif: serde_json::Value = serde_json::from_str(&written).expect("valid SARIF JSON");
        assert_eq!(sarif["version"], "2.1.0");
    }
}
