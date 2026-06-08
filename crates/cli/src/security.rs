//! `fallow security` command: opt-in local security-candidate surface.
//!
//! Ships the graph-structural `client-server-leak` rule plus the data-driven
//! `tainted-sink` catalogue (one `TaintedSink` kind covering every CWE category
//! in `security_matchers.toml`). Findings are CANDIDATES for downstream agent
//! verification, NOT verified vulnerabilities.
//! This command is the ONLY surface for security findings: they never appear
//! under bare `fallow` or the `audit` gate. There is no `confidence` or
//! `signal_strength` field; the structural trace is the only honest signal.

use crate::report::sink::outln;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use fallow_config::{OutputFormat, ProductionAnalysis, Severity};
use fallow_core::results::{
    SecurityDeadCodeKind, SecurityFinding, SecurityFindingKind, TraceHopRole,
};
use serde::Serialize;

use crate::error::emit_error;
use crate::load_config_for_analysis;

/// The `fallow security --format json` schema version. Independently versioned
/// from the main contract, mirroring `ImpactReportSchemaVersion`.
#[derive(Debug, Clone, Copy, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub enum SecuritySchemaVersion {
    /// First release of the `fallow security --format json` shape.
    #[serde(rename = "1")]
    V1,
}

/// The `fallow security --format json` envelope. `security_findings` is the
/// unique required field used for untagged narrowing in `FallowOutput`.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SecurityOutput {
    /// Schema version of this envelope.
    pub schema_version: SecuritySchemaVersion,
    /// Security candidates. Paths are project-root-relative, forward-slash.
    pub security_findings: Vec<SecurityFinding>,
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
}

/// Run `fallow security`. Always exits 0 unless the user explicitly raised the
/// `security-client-server-leak` rule to `error` AND findings exist (the rule
/// defaults to `off` and the command forces it to `warn`, so the common case is
/// advisory). Unsupported output formats exit 2.
#[expect(
    deprecated,
    reason = "ADR-008 deprecates fallow_core::analyze externally; the CLI uses the workspace path dependency"
)]
pub fn run(opts: &SecurityOptions<'_>) -> ExitCode {
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

    // Respect an explicit user severity; force the rule on (warn) when it is the
    // default off, so the detector runs for this dedicated command. Both the
    // client-server-leak and the catalogue-driven tainted-sink rules are flipped.
    let effective_severity = config.rules.security_client_server_leak;
    if effective_severity == Severity::Off {
        config.rules.security_client_server_leak = Severity::Warn;
    }
    let effective_sink_severity = config.rules.security_sink;
    if effective_sink_severity == Severity::Off {
        config.rules.security_sink = Severity::Warn;
    }

    let mut results = match fallow_core::analyze(&config) {
        Ok(results) => results,
        Err(err) => return emit_error(&format!("Analysis error: {err}"), 2, opts.output),
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
        crate::check::filtering::filter_to_workspaces(&mut results, roots);
    }

    // Changed-since scope (canonical normalization via the core filter, which
    // now retains security_findings too).
    if let Some(git_ref) = opts.changed_since
        && let Some(changed) = fallow_core::changed_files::get_changed_files(opts.root, git_ref)
    {
        fallow_core::changed_files::filter_results_by_changed_files(&mut results, &changed);
    }
    if opts.use_shared_diff_index
        && let Some(diff_index) = crate::report::ci::diff_filter::shared_diff_index()
    {
        crate::check::filtering::filter_results_by_diff(&mut results, diff_index, opts.root);
    }
    filter_to_files(&mut results, opts.root, opts.file, opts.quiet);

    let unresolved_edge_files = results.security_unresolved_edge_files;
    let unresolved_callee_sites = results.security_unresolved_callee_sites;
    let findings: Vec<SecurityFinding> = std::mem::take(&mut results.security_findings)
        .into_iter()
        .map(|f| relativize_finding(f, &config.root))
        .collect();

    let fail = (opts.fail_on_issues
        || effective_severity == Severity::Error
        || effective_sink_severity == Severity::Error)
        && !findings.is_empty();

    let output = SecurityOutput {
        schema_version: SecuritySchemaVersion::V1,
        security_findings: findings,
        unresolved_edge_files,
        unresolved_callee_sites,
    };

    if let Some(path) = opts.sarif_file
        && let Err(message) = write_sarif_file(&output, path)
    {
        return emit_error(&message, 2, opts.output);
    }

    let rendered = match opts.output {
        OutputFormat::Json => render_json(&output),
        OutputFormat::Sarif => render_sarif(&output),
        _ if opts.summary => render_human_summary(&output),
        _ => render_human(&output),
    };
    outln!("{rendered}");

    if fail {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
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

/// Rewrite a finding's anchor + every trace hop path to be project-root-relative
/// (forward-slash normalization happens at serialize time via `serde_path`).
fn relativize_finding(mut finding: SecurityFinding, root: &Path) -> SecurityFinding {
    finding.path = relativize(&finding.path, root);
    for hop in &mut finding.trace {
        hop.path = relativize(&hop.path, root);
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

#[must_use]
fn render_human_summary(output: &SecurityOutput) -> String {
    use crate::report::plural;
    use std::fmt::Write as _;

    let count = output.security_findings.len();
    let mut out = format!(
        "Security candidates: {count} candidate{} found. These are NOT verified vulnerabilities; verify each before acting.\n",
        plural(count),
    );
    if output.unresolved_edge_files > 0 {
        let n = output.unresolved_edge_files;
        let _ = writeln!(
            out,
            "Unresolved dynamic import cones: {n} client file{}.",
            plural(n)
        );
    }
    if output.unresolved_callee_sites > 0 {
        let n = output.unresolved_callee_sites;
        let _ = writeln!(out, "Unresolved sink callees: {n} site{}.", plural(n));
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
    out.push_str("Security candidates (unverified; for agent or human verification)\n\n");

    if output.security_findings.is_empty() {
        out.push_str("No security candidates found.\n");
    } else {
        for finding in &output.security_findings {
            let kind = security_finding_label(finding);
            // [I] (info/advisory) is the design-system prefix for off-by-default
            // findings surfaced for review; it deliberately is NOT a severity glyph.
            out.push_str(&format!(
                "{} {kind}  {}:{}\n",
                "[I]".blue().bold(),
                finding.path.to_string_lossy().replace('\\', "/").bold(),
                finding.line,
            ));
            out.push_str(&format!("    {}\n", finding.evidence));
            if let Some(hint) = dead_code_hint(finding) {
                out.push_str(&format!("    dead-code: {hint}\n"));
            }
            if let Some(reach) = finding.reachability {
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
                    "    reach: {entry} (blast radius {}){boundary}\n",
                    reach.blast_radius,
                ));
            }
            if !finding.trace.is_empty() {
                out.push_str("    trace:\n");
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
                    "    Next: check whether the import is type-only, server-only, or behind a \
                     build-time guard; if the value never ships to the client bundle, this \
                     candidate is a false positive.\n",
                );
            } else if finding.dead_code.is_some() {
                out.push_str(
                    "    Next: verify the dead-code finding and delete the code if safe; \
                     otherwise verify and harden the sink.\n",
                );
            }
            out.push('\n');
        }
    }

    if output.unresolved_edge_files > 0 {
        let n = output.unresolved_edge_files;
        out.push_str(&format!(
            "{} {n} client file{} reached a dynamic import the reachability scan could not \
             follow; a leak behind those edges would not be reported, so an empty result is \
             not a clean bill.\n",
            "[I]".blue().bold(),
            plural(n),
        ));
    }

    if output.unresolved_callee_sites > 0 {
        let n = output.unresolved_callee_sites;
        out.push_str(&format!(
            "{} {n} sink site{} had a callee the catalogue scan could not resolve to a static \
             path (dynamic dispatch, computed members, aliased bindings); an empty result is \
             not a clean bill.\n",
            "[I]".blue().bold(),
            plural(n),
        ));
    }

    let count = output.security_findings.len();
    out.push_str(&format!(
        "\nFound {count} security candidate{}. These are NOT verified vulnerabilities; verify \
         each before acting.\n",
        plural(count),
    ));
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
        TraceHopRole::Intermediate => "intermediate",
        TraceHopRole::SecretSource => "secret source",
        TraceHopRole::Sink => "sink site",
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

/// Build the SARIF rule definition for a ruleId, deriving per-category metadata
/// (catalogue title + CWE tag) for `TaintedSink` findings so the CWE survives
/// into GHAS via the `external/cwe/cwe-NNN` tag convention.
fn sarif_rule_def(rule_id: &str, finding: &SecurityFinding) -> serde_json::Value {
    match finding.kind {
        SecurityFindingKind::ClientServerLeak => serde_json::json!({
            "id": rule_id,
            "shortDescription": { "text": "Client-server secret leak candidate (unverified)" },
            "fullDescription": { "text":
                "Unverified candidate, requires verification: a \"use client\" file \
                 transitively imports a module that reads a non-public process.env \
                 secret. fallow does not prove the secret reaches client-bundled code." },
            "helpUri": "https://github.com/fallow-rs/fallow",
            "defaultConfiguration": { "level": "note" }
        }),
        SecurityFindingKind::TaintedSink => {
            let title = finding
                .category
                .as_deref()
                .and_then(fallow_core::analyze::security_catalogue_title)
                .or(finding.category.as_deref())
                .unwrap_or("tainted-sink");
            let mut rule = serde_json::json!({
                "id": rule_id,
                "shortDescription": { "text": format!("{title} candidate (unverified)") },
                "fullDescription": { "text": format!(
                    "Unverified candidate, requires verification: {title}. fallow flags a \
                     syntactic sink reached by a non-literal argument; it does not prove the \
                     value is attacker-controlled or reaches the sink unsanitized."
                ) },
                "helpUri": "https://github.com/fallow-rs/fallow",
                "defaultConfiguration": { "level": "note" }
            });
            if let Some(cwe) = finding.cwe {
                rule["properties"] = serde_json::json!({
                    "tags": [format!("external/cwe/cwe-{cwe}")]
                });
            }
            rule
        }
    }
}

/// SARIF output. Emits `level: "note"` (never error/warning) so the candidate
/// framing survives into the GitHub Security tab. Each finding's ruleId is
/// per-category (`security/<category>` for tainted-sink, `security/client-server-leak`
/// for the graph rule); the `rules` array carries one definition per distinct
/// ruleId present, with the CWE tag for tainted-sink categories. Trace hops
/// become `relatedLocations` of the result.
#[must_use]
fn render_sarif(output: &SecurityOutput) -> String {
    let results: Vec<serde_json::Value> = output
        .security_findings
        .iter()
        .map(|finding| {
            let rule_id = sarif_rule_id(finding);
            let message = dead_code_hint(finding).map_or_else(
                || finding.evidence.clone(),
                |hint| format!("{} Dead-code cross-link: {hint}.", finding.evidence),
            );
            let related: Vec<serde_json::Value> = finding
                .trace
                .iter()
                .map(|hop| sarif_location(&hop.path, hop.line, hop.col))
                .collect();
            // Stable dedup key for GHAS: rule + anchor path + line. Without
            // partialFingerprints, every run re-opens previously triaged alerts.
            let fp = format!(
                "{rule_id}:{}:{}",
                finding.path.to_string_lossy().replace('\\', "/"),
                finding.line,
            );
            serde_json::json!({
                "ruleId": rule_id,
                "level": "note",
                "message": { "text": message },
                "locations": [sarif_location(&finding.path, finding.line, finding.col)],
                "relatedLocations": related,
                "partialFingerprints": { "fallowSecurity/v1": fnv_hex(&fp) },
            })
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
        rules.push(sarif_rule_def(&rule_id, finding));
    }

    let sarif = serde_json::json!({
        "version": "2.1.0",
        "$schema": "https://json.schemastore.org/sarif-2.1.0.json",
        "runs": [{
            "tool": { "driver": {
                "name": "fallow",
                "version": env!("CARGO_PKG_VERSION"),
                "informationUri": "https://github.com/fallow-rs/fallow",
                "rules": rules,
            }},
            "results": results,
        }],
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
        SecurityDeadCodeContext, SecurityDeadCodeKind, SecurityFinding, SecurityFindingKind,
        TraceHop, TraceHopRole,
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
        }
    }

    fn output_with(findings: Vec<SecurityFinding>, unresolved_edge_files: usize) -> SecurityOutput {
        SecurityOutput {
            schema_version: SecuritySchemaVersion::V1,
            security_findings: findings,
            unresolved_edge_files,
            unresolved_callee_sites: 0,
        }
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
        assert!(out.contains("0 candidates found"), "got: {out}");
        assert!(!out.contains("Unresolved dynamic import cones"));
    }

    #[test]
    fn human_summary_pluralizes_and_surfaces_unresolved_edges() {
        let root = Path::new("/proj/root");
        let out = render_human_summary(&output_with(vec![sample_finding(root)], 2));
        assert!(out.contains("1 candidate found"), "got: {out}");
        assert!(out.contains("Unresolved dynamic import cones: 2 client files."));
    }

    #[test]
    fn human_render_empty_states_no_candidates() {
        colored::control::set_override(false);
        let out = render_human(&output_with(vec![], 0));
        assert!(out.contains("No security candidates found."));
        assert!(out.contains("Found 0 security candidates"));
    }

    #[test]
    fn human_render_shows_finding_trace_and_next_action() {
        colored::control::set_override(false);
        let root = Path::new("/proj/root");
        let finding = relativize_finding(sample_finding(root), root);
        let out = render_human(&output_with(vec![finding], 0));
        assert!(out.contains("client-server-leak"));
        assert!(out.contains("src/app.tsx:12"));
        assert!(out.contains("reaches process.env.SECRET_KEY"));
        assert!(out.contains("trace:"));
        assert!(out.contains("src/lib/secret.ts:8 (secret source)"));
        assert!(out.contains("src/app.tsx:12 (client boundary)"));
        assert!(out.contains("Next:"));
        assert!(out.contains("Found 1 security candidate."));
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
        assert!(out.contains("delete the code if safe"), "got: {out}");
    }

    #[test]
    fn human_render_surfaces_unresolved_edge_blind_spot() {
        colored::control::set_override(false);
        let out = render_human(&output_with(vec![], 3));
        assert!(out.contains("3 client files reached a dynamic import"));
        assert!(out.contains("not a clean bill"));
    }

    #[test]
    fn json_render_carries_schema_version_and_findings() {
        let root = Path::new("/proj/root");
        let finding = relativize_finding(sample_finding(root), root);
        let rendered = render_json(&output_with(vec![finding], 1));
        let value: serde_json::Value = serde_json::from_str(&rendered).expect("valid JSON");
        assert_eq!(value["schema_version"], "1");
        assert_eq!(value["unresolved_edge_files"], 1);
        let findings = value["security_findings"].as_array().expect("array");
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0]["kind"], "client-server-leak");
        assert_eq!(findings[0]["path"], "src/app.tsx");
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
    fn sarif_render_emits_note_level_with_fingerprint_and_related_locations() {
        let root = Path::new("/proj/root");
        let finding = relativize_finding(sample_finding(root), root);
        let rendered = render_sarif(&output_with(vec![finding], 0));
        let sarif: serde_json::Value = serde_json::from_str(&rendered).expect("valid SARIF JSON");
        assert_eq!(sarif["version"], "2.1.0");
        let run = &sarif["runs"][0];
        assert_eq!(run["tool"]["driver"]["name"], "fallow");
        let result = &run["results"][0];
        // Candidate framing: never error/warning, and no CWE tag.
        assert_eq!(result["level"], "note");
        assert_eq!(result["ruleId"], "security/client-server-leak");
        assert_eq!(result["message"]["text"], "reaches process.env.SECRET_KEY");
        // Trace hops surface as relatedLocations (3 hops).
        assert_eq!(result["relatedLocations"].as_array().unwrap().len(), 3);
        // Stable dedup fingerprint present for GHAS.
        assert!(result["partialFingerprints"]["fallowSecurity/v1"].is_string());
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
    fn sarif_tainted_sink_uses_per_category_rule_id_and_cwe_tag() {
        let root = Path::new("/proj/root");
        let mut finding = sample_finding(root);
        finding.kind = SecurityFindingKind::TaintedSink;
        finding.category = Some("dangerous-html".to_owned());
        finding.cwe = Some(79);
        let rendered = render_sarif(&output_with(vec![relativize_finding(finding, root)], 0));
        let sarif: serde_json::Value = serde_json::from_str(&rendered).expect("valid SARIF JSON");
        let run = &sarif["runs"][0];
        // The finding is grouped under its own per-category rule, not collapsed
        // into client-server-leak, and stays at candidate (note) level.
        let result = &run["results"][0];
        assert_eq!(result["level"], "note");
        assert_eq!(result["ruleId"], "security/dangerous-html");
        // Exactly one rule definition, carrying the CWE as a GHAS tag.
        let rules = run["tool"]["driver"]["rules"].as_array().unwrap();
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0]["id"], "security/dangerous-html");
        let tags = rules[0]["properties"]["tags"].as_array().unwrap();
        assert!(tags.iter().any(|t| t == "external/cwe/cwe-79"));
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
        }
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
