//! Local security candidate detection (opt-in, surfaced only by `fallow security`).
//!
//! These are CANDIDATES for downstream agent verification, NOT verified
//! vulnerabilities. The graph-structural `client-server-leak` rule has two
//! sink predicates over the SAME `"use client"` transitive static-import cone:
//!
//! 1. The cone reaches a module that reads a non-public env secret
//!    (`category: None`, the original finding).
//! 2. The cone reaches a SERVER-ONLY module (`category: Some("server-only-import")`):
//!    a module carrying `"use server"`, importing the `server-only` poison
//!    package, or importing a server-only Next.js / Node API (see the shared
//!    `is_server_only_module` predicate in `analyze::server_only`).
//!
//! fallow emits the structural import-hop trace; it does not prove the path is
//! exploitable.
//!
//! Blind spots (surfaced in-band via [`UnresolvedEdgeStats`], not silently
//! dropped): the BFS follows only resolved static import edges, so glob-shaped
//! dynamic `import()` patterns and unresolved specifiers can hide a real leak.
//!
//! ssr:false escape hatch: a server module pulled in ONLY through
//! `next/dynamic(() => import('./X'), { ssr: false })` is the sanctioned
//! client-only escape hatch, NOT a leak. fallow's arrow-wrapped dynamic-import
//! detection resolves `next/dynamic(() => import('./X'))` to a STATIC graph edge
//! (it credits the target's default export), so that edge IS in the cone. The
//! extract layer records the `import()` span of each ssr:false call on
//! `ModuleInfo::client_only_dynamic_import_spans`, and the BFS excludes an edge
//! reached ONLY through such a span (see
//! `ModuleGraph::outgoing_edge_summaries_with_exclusions`). An edge also reached
//! via a real static import stays in the cone.

use rustc_hash::{FxHashMap, FxHashSet};
use std::collections::VecDeque;

use fallow_types::extract::ModuleInfo;
use fallow_types::output::{IssueAction, SuppressFileAction, SuppressFileKind};
use fallow_types::results::{
    SecurityCandidate, SecurityCandidateBoundary, SecurityCandidateSink, SecurityFinding,
    SecurityFindingKind, SecuritySeverity, TraceHop, TraceHopRole,
};
use fallow_types::suppress::IssueKind;

use super::{LineOffsetsMap, byte_offset_to_line_col};
use crate::discover::FileId;
use crate::graph::{ModuleGraph, ModuleNode};
use crate::suppress::SuppressionContext;

mod catalogue;
mod hardcoded_secret;
mod rank;
mod tainted_sink;

pub use hardcoded_secret::find_hardcoded_secret_candidates;
pub use rank::{annotate_dead_code_cross_links, derive_security_severity, rank_security_findings};
pub use tainted_sink::{CategoryFilter, TaintedSinkContext, find_tainted_sinks};

/// Segment-aware callee pattern matcher, re-exported for the boundary
/// forbidden-call detector (`analyze::boundary_calls`).
pub use catalogue::{CalleePattern, Matcher};

#[must_use]
pub fn catalogue_matchers() -> &'static [Matcher] {
    catalogue::catalogue().matchers()
}

#[must_use]
pub fn catalogue_title(id: &str) -> Option<&'static str> {
    if id == hardcoded_secret::CATEGORY_ID {
        Some(hardcoded_secret::CATEGORY_TITLE)
    } else {
        catalogue::catalogue_title(id)
    }
}

/// The inline suppression kind token for the client-server-leak rule.
const SUPPRESS_KIND: &str = "security-client-server-leak";

/// Stable `category` string distinguishing the server-only-import sink from the
/// secret-leak sink (`category: None`). Same rule, same suppress kind, same
/// `SecurityFinding` shape; only the category differs so JSON / human / SARIF
/// consumers can tell "reaches server-only code" apart from "reads a secret".
const SERVER_ONLY_CATEGORY: &str = "server-only-import";

/// Build the machine-actionable suppress hint emitted on every finding. Single
/// file-level suppress action (`auto_fixable: false`): there is no auto-fix
/// because verifying the candidate is the agent's job, not fallow's.
fn build_actions() -> Vec<IssueAction> {
    vec![IssueAction::SuppressFile(SuppressFileAction {
        kind: SuppressFileKind::SuppressFile,
        auto_fixable: false,
        description: "Suppress with a file-level comment at the top of the client file".to_string(),
        comment: format!("// fallow-ignore-file {SUPPRESS_KIND}"),
    })]
}

/// The React Server Components client-boundary directive.
const USE_CLIENT: &str = "use client";

/// Singular/plural noun for the count of secret vars named in the evidence.
const fn secret_word(count: usize) -> &'static str {
    if count == 1 { "secret" } else { "secrets" }
}

/// The `member_accesses` object string for a `process.env.X` read.
const PROCESS_ENV_OBJECT: &str = "process.env";
/// The `member_accesses` object string for an `import.meta.env.X` read.
const IMPORT_META_ENV_OBJECT: &str = "import.meta.env";
/// Static env source objects that feed the client/server leak candidate rule.
const ENV_SOURCE_OBJECTS: &[&str] = &[PROCESS_ENV_OBJECT, IMPORT_META_ENV_OBJECT];

// The public-env predicate (`is_public_env_var`, `PUBLIC_ENV_PREFIXES`) is shared
// with the extract layer in `fallow_types::extract` (issue #890), so public env
// vars are excluded consistently here and at source-recording time.
use fallow_types::extract::is_public_env_var;

/// Blind-spot accounting surfaced in-band so the user is never told "clean" when
/// the analysis could not see part of the import graph.
#[derive(Debug, Default, Clone, Copy)]
pub struct UnresolvedEdgeStats {
    /// Number of `"use client"` files whose transitive import cone contains a
    /// dynamic `import()` pattern the reachability BFS cannot follow, so a leak
    /// could be hiding behind it.
    pub client_files_with_unresolved_edges: usize,
}

/// Run the security MVP rules over the graph. Returns the findings plus the
/// blind-spot stats. Callers gate this on the `security_client_server_leak`
/// rule severity; it never runs under bare `fallow` or the `audit` gate.
#[must_use]
pub fn find_security_findings(
    graph: &ModuleGraph,
    modules: &[ModuleInfo],
    suppressions: &SuppressionContext<'_>,
    line_offsets_by_file: &LineOffsetsMap<'_>,
) -> (Vec<SecurityFinding>, UnresolvedEdgeStats) {
    let modules_by_id: FxHashMap<FileId, &ModuleInfo> =
        modules.iter().map(|m| (m.file_id, m)).collect();

    let secret_sources = compute_secret_source_set(&modules_by_id);
    let server_only_sources = compute_server_only_source_set(&modules_by_id);

    find_client_server_leaks(
        graph,
        &modules_by_id,
        &secret_sources,
        &server_only_sources,
        suppressions,
        line_offsets_by_file,
    )
}

/// Map each module that reads a non-public env secret to the source names it
/// reads. `process.env.X` and `import.meta.env.X` reads both surface as
/// `MemberAccess` rows from static-member capture.
fn compute_secret_source_set(
    modules_by_id: &FxHashMap<FileId, &ModuleInfo>,
) -> FxHashMap<FileId, Vec<String>> {
    let mut sources: FxHashMap<FileId, Vec<String>> = FxHashMap::default();
    for (&file_id, module) in modules_by_id {
        let mut vars: Vec<String> = module
            .member_accesses
            .iter()
            .filter(|ma| {
                ENV_SOURCE_OBJECTS.contains(&ma.object.as_str()) && !is_public_env_var(&ma.member)
            })
            .map(|ma| format!("{}.{}", ma.object, ma.member))
            .collect();
        if vars.is_empty() {
            continue;
        }
        vars.sort_unstable();
        vars.dedup();
        sources.insert(file_id, vars);
    }
    sources
}

/// Set of file ids whose module is a SERVER-ONLY sink: it carries a `"use server"`
/// directive, imports a server-only package, or imports a server-only named API
/// from `next/headers`. Delegates to the shared
/// [`is_server_only_module`](super::server_only::is_server_only_module) predicate
/// so the server-only definition is identical to the
/// `mixed_client_server_barrel` detector's.
fn compute_server_only_source_set(
    modules_by_id: &FxHashMap<FileId, &ModuleInfo>,
) -> FxHashSet<FileId> {
    let mut server_only: FxHashSet<FileId> = FxHashSet::default();
    for (&file_id, module) in modules_by_id {
        if super::server_only::is_server_only_module(module) {
            server_only.insert(file_id);
        }
    }
    server_only
}

/// For each `"use client"` file, BFS its transitive static-import cone. Two
/// distinct sink predicates run over the SAME cone:
///
/// - reaching a module that reads a non-public env secret emits the secret-leak
///   finding (`category: None`);
/// - reaching a SERVER-ONLY module emits the server-only finding
///   (`category: Some("server-only-import")`).
///
/// Both can fire for one client file (they are different concerns). Type-only
/// edges are skipped (erased at build, so they cannot carry a secret or a
/// server-only import into the client bundle).
/// Result of a single client file's import-cone BFS: the parent map for trace
/// reconstruction, the first secret / server-only module reached, and whether
/// any unresolved dynamic-import edge was seen in the cone.
struct ClientConeResult {
    parent: FxHashMap<FileId, (FileId, Option<u32>)>,
    reached_secret: Option<FileId>,
    reached_server_only: Option<FileId>,
    had_unresolved_edge: bool,
}

/// BFS the static import cone of a `"use client"` file, draining the FULL cone
/// (no early break) so the dynamic-import blind-spot count reflects every edge,
/// not just the path to the first finding. Type-only and `next/dynamic
/// ssr:false`-only edges are excluded (neither can leak into the client bundle).
fn walk_client_cone(
    graph: &ModuleGraph,
    client_id: FileId,
    modules_by_id: &FxHashMap<FileId, &ModuleInfo>,
    secret_sources: &FxHashMap<FileId, Vec<String>>,
    server_only_sources: &FxHashSet<FileId>,
    client_only_spans: &FxHashMap<FileId, FxHashSet<u32>>,
    empty_spans: &FxHashSet<u32>,
) -> ClientConeResult {
    let mut visited: FxHashSet<FileId> = FxHashSet::default();
    visited.insert(client_id);
    let mut result = ClientConeResult {
        parent: FxHashMap::default(),
        reached_secret: None,
        reached_server_only: None,
        had_unresolved_edge: false,
    };
    let mut queue: VecDeque<FileId> = VecDeque::new();
    queue.push_back(client_id);

    while let Some(current) = queue.pop_front() {
        if let Some(current_module) = modules_by_id.get(&current)
            && !current_module.dynamic_import_patterns.is_empty()
        {
            result.had_unresolved_edge = true;
        }

        if current != client_id {
            if result.reached_secret.is_none() && secret_sources.contains_key(&current) {
                result.reached_secret = Some(current);
            }
            if result.reached_server_only.is_none() && server_only_sources.contains(&current) {
                result.reached_server_only = Some(current);
            }
        }

        // Exclude edges reached only through a `next/dynamic ssr:false`
        // dynamic import made by `current` (the client-only escape hatch).
        let excluded = client_only_spans.get(&current).unwrap_or(empty_spans);
        for (target, all_type_only, span_start, all_client_only) in
            graph.outgoing_edge_summaries_with_exclusions(current, excluded)
        {
            if all_type_only {
                continue; // type-only imports are erased at build; cannot leak.
            }
            if all_client_only {
                // Reached only via next/dynamic ssr:false: the sanctioned
                // client-only escape hatch, not a leak edge.
                continue;
            }
            if visited.insert(target) {
                result.parent.insert(target, (current, span_start));
                queue.push_back(target);
            }
        }
    }

    result
}

fn find_client_server_leaks(
    graph: &ModuleGraph,
    modules_by_id: &FxHashMap<FileId, &ModuleInfo>,
    secret_sources: &FxHashMap<FileId, Vec<String>>,
    server_only_sources: &FxHashSet<FileId>,
    suppressions: &SuppressionContext<'_>,
    line_offsets_by_file: &LineOffsetsMap<'_>,
) -> (Vec<SecurityFinding>, UnresolvedEdgeStats) {
    let mut findings = Vec::new();
    let mut stats = UnresolvedEdgeStats::default();

    // Per-file set of `next/dynamic(..., { ssr: false })` dynamic-import span
    // starts. The BFS excludes an edge reached ONLY through one of these so a
    // server-only (or secret) module pulled in via the sanctioned client-only
    // escape hatch is not flagged. Empty sets are skipped at the call site.
    let client_only_spans: FxHashMap<FileId, FxHashSet<u32>> = modules_by_id
        .iter()
        .filter(|(_, m)| !m.client_only_dynamic_import_spans.is_empty())
        .map(|(&id, m)| {
            (
                id,
                m.client_only_dynamic_import_spans.iter().copied().collect(),
            )
        })
        .collect();
    let empty_spans: FxHashSet<u32> = FxHashSet::default();

    let scan = LeakScanInput {
        graph,
        modules_by_id,
        secret_sources,
        server_only_sources,
        suppressions,
        line_offsets_by_file,
        client_only_spans: &client_only_spans,
        empty_spans: &empty_spans,
    };

    for node in &graph.modules {
        scan_client_file_for_leaks(&scan, node, &mut findings, &mut stats);
    }

    findings.sort_by(|a, b| {
        a.path
            .cmp(&b.path)
            .then(a.line.cmp(&b.line))
            .then(a.category.cmp(&b.category))
    });
    (findings, stats)
}

/// Shared immutable inputs for the per-client-file leak scan.
struct LeakScanInput<'a> {
    graph: &'a ModuleGraph,
    modules_by_id: &'a FxHashMap<FileId, &'a ModuleInfo>,
    secret_sources: &'a FxHashMap<FileId, Vec<String>>,
    server_only_sources: &'a FxHashSet<FileId>,
    suppressions: &'a SuppressionContext<'a>,
    line_offsets_by_file: &'a LineOffsetsMap<'a>,
    client_only_spans: &'a FxHashMap<FileId, FxHashSet<u32>>,
    empty_spans: &'a FxHashSet<u32>,
}

/// Emit direct and transitive client-server leak findings for one module when it
/// is a non-suppressed `"use client"` file. Non-client and file-suppressed nodes
/// are skipped. Direct secret / server-only reads and transitive cone reaches
/// each emit at most one finding, never double-flagging the same file.
fn scan_client_file_for_leaks(
    scan: &LeakScanInput<'_>,
    node: &ModuleNode,
    findings: &mut Vec<SecurityFinding>,
    stats: &mut UnresolvedEdgeStats,
) {
    let Some(module) = scan.modules_by_id.get(&node.file_id) else {
        return;
    };
    if !module.directives.iter().any(|d| d == USE_CLIENT) {
        return;
    }
    let client_id = node.file_id;
    // A file-level `// fallow-ignore-file security-client-server-leak`
    // (or a blanket file ignore) opts the whole client file out. Routed
    // through the SuppressionContext so the marker is recorded as consumed
    // (otherwise a working suppression would later be flagged stale).
    if scan
        .suppressions
        .is_file_suppressed(client_id, IssueKind::SecurityClientServerLeak)
    {
        return;
    }

    // Direct case: the client file itself reads a non-public secret. The
    // most direct leak; no import hop needed.
    if scan.secret_sources.contains_key(&client_id) {
        findings.push(build_direct_finding(
            scan.graph,
            client_id,
            scan.secret_sources,
        ));
        // Still count its dynamic-import blind spot below.
    }

    // Direct server-only case: the client file itself IS a server-only sink
    // (carries "use server", imports a server-only package, or imports a
    // server-only next/headers API). The most direct server-only leak; no
    // import hop needed. The transitive server-only emit below is gated so a
    // file that is both a direct AND a transitive sink is flagged once.
    if scan.server_only_sources.contains(&client_id) {
        findings.push(build_direct_server_only_finding(scan.graph, client_id));
    }

    // Transitive case: BFS the import cone.
    let cone = walk_client_cone(
        scan.graph,
        client_id,
        scan.modules_by_id,
        scan.secret_sources,
        scan.server_only_sources,
        scan.client_only_spans,
        scan.empty_spans,
    );

    if cone.had_unresolved_edge {
        stats.client_files_with_unresolved_edges += 1;
    }

    // Only emit a transitive secret finding when the client is not ALREADY
    // flagged by the direct case (avoid double-flagging the same file).
    if let Some(secret_id) = cone.reached_secret
        && !scan.secret_sources.contains_key(&client_id)
    {
        findings.push(build_leak_finding(
            scan.graph,
            client_id,
            secret_id,
            &cone.parent,
            scan.secret_sources,
            scan.line_offsets_by_file,
        ));
    }

    // The server-only sink is a DISTINCT category, independent of the secret
    // case: a client cone can reach BOTH a secret reader and a server-only
    // module, and a reviewer wants to see both. Only emit a transitive
    // server-only finding when the client is not ALREADY flagged by the
    // direct server-only case above (avoid double-flagging the same file).
    if let Some(server_id) = cone.reached_server_only
        && !scan.server_only_sources.contains(&client_id)
    {
        findings.push(build_server_only_finding(
            scan.graph,
            client_id,
            server_id,
            &cone.parent,
            scan.line_offsets_by_file,
        ));
    }
}

/// Walk the BFS parent map from `sink_id` back to `client_id`, building the
/// shortest-path trace. Each non-terminal hop's line is the import site in that
/// file (where it imports the NEXT hop); the terminal sink hop has no outgoing
/// edge in the chain so it anchors at line 1 with `terminal_role`. Shared by the
/// secret-leak (`SecretSource`) and server-only (`Sink`) findings.
fn build_client_server_trace(
    graph: &ModuleGraph,
    client_id: FileId,
    sink_id: FileId,
    parent: &FxHashMap<FileId, (FileId, Option<u32>)>,
    line_offsets_by_file: &LineOffsetsMap<'_>,
    terminal_role: TraceHopRole,
) -> Vec<TraceHop> {
    // Walk parent pointers from the sink back to the client, then reverse.
    let mut chain: Vec<FileId> = vec![sink_id];
    let mut cursor = sink_id;
    while let Some(&(prev, _)) = parent.get(&cursor) {
        chain.push(prev);
        cursor = prev;
        if prev == client_id {
            break;
        }
    }
    chain.reverse(); // now [client_id, ..., sink_id]

    let mut trace: Vec<TraceHop> = Vec::with_capacity(chain.len());
    for (idx, &file_id) in chain.iter().enumerate() {
        let role = if idx == 0 {
            TraceHopRole::ClientBoundary
        } else if file_id == sink_id {
            terminal_role
        } else {
            TraceHopRole::Intermediate
        };
        // The line for a non-terminal hop is where it imports the NEXT hop.
        // Member-access / import extraction does not carry a precise span for the
        // terminal sink hop, so it falls back to the source module start.
        let (line, col) = if let Some(&next) = chain.get(idx + 1) {
            parent
                .get(&next)
                .and_then(|&(_, span)| span)
                .map_or((1, 0), |s| {
                    byte_offset_to_line_col(line_offsets_by_file, file_id, s)
                })
        } else {
            (1, 0)
        };
        trace.push(TraceHop {
            path: graph.modules[file_id.0 as usize].path.clone(),
            line,
            col,
            role,
        });
    }
    trace
}

/// Reconstruct the shortest path `client -> ... -> secret` from the BFS parent
/// map and build the finding. Each non-terminal hop's line is the import site in
/// that file (where it imports the next hop); the secret-source hop has no
/// outgoing edge in the chain so it anchors at line 1.
fn build_leak_finding(
    graph: &ModuleGraph,
    client_id: FileId,
    secret_id: FileId,
    parent: &FxHashMap<FileId, (FileId, Option<u32>)>,
    secret_sources: &FxHashMap<FileId, Vec<String>>,
    line_offsets_by_file: &LineOffsetsMap<'_>,
) -> SecurityFinding {
    let trace = build_client_server_trace(
        graph,
        client_id,
        secret_id,
        parent,
        line_offsets_by_file,
        TraceHopRole::SecretSource,
    );

    let anchor = &trace[0];
    let empty = Vec::new();
    let var_list = secret_sources.get(&secret_id).unwrap_or(&empty);
    let vars = var_list.join(", ");
    let word = secret_word(var_list.len());
    // Do NOT embed the secret file path in the message: the SecretSource trace
    // hop already names it (and is root-relativized by the CLI before display),
    // so embedding the absolute path here would leak it past relativization and
    // make output environment-dependent.
    let evidence = format!(
        "This \"use client\" file transitively imports a module that reads non-public \
         env {word}: {vars} (see the secret-source hop in the trace). Candidate for \
         verification: confirm the secret value actually reaches client-bundled code."
    );

    // The client-server-leak rule is graph-structural, not catalogue-driven:
    // no source kind, no callee, no CWE. The candidate's sink slot anchors on
    // the client boundary file; the boundary slot is filled by the ranking pass.
    let candidate = client_leak_candidate(anchor.path.clone(), anchor.line, anchor.col, None);

    SecurityFinding {
        finding_id: String::new(),
        kind: SecurityFindingKind::ClientServerLeak,
        category: None,
        cwe: None,
        path: candidate.sink.path.clone(),
        line: candidate.sink.line,
        col: candidate.sink.col,
        evidence,
        // The client-server-leak rule is graph-structural, not source-to-sink;
        // source-backing is a tainted-sink concept (issue #859).
        source_backed: false,
        // client-server-leak is module-level by construction (no arg-level read).
        source_read: None,
        severity: SecuritySeverity::Low,
        trace,
        actions: build_actions(),
        dead_code: None,
        reachability: None,
        candidate,
        taint_flow: None,
        runtime: None,
        attack_surface: None,
    }
}

/// Build a finding for the SERVER-ONLY sink: a `"use client"` file whose
/// transitive static-import cone reaches a server-only module (one carrying
/// `"use server"` or importing a server-only package). Same rule, same suppress
/// kind, same `SecurityFinding` shape as the secret leak; distinguished by
/// `category: Some("server-only-import")` so consumers can tell the two apart.
/// The terminal trace hop carries the `Sink` role (the server-only module is the
/// sink of this candidate).
fn build_server_only_finding(
    graph: &ModuleGraph,
    client_id: FileId,
    server_id: FileId,
    parent: &FxHashMap<FileId, (FileId, Option<u32>)>,
    line_offsets_by_file: &LineOffsetsMap<'_>,
) -> SecurityFinding {
    let trace = build_client_server_trace(
        graph,
        client_id,
        server_id,
        parent,
        line_offsets_by_file,
        TraceHopRole::Sink,
    );

    let anchor = &trace[0];
    // Do NOT embed the server module path in the message: the Sink trace hop
    // already names it (root-relativized by the CLI before display), so embedding
    // the absolute path here would leak it past relativization and make output
    // environment-dependent.
    let evidence = "This \"use client\" file transitively imports a SERVER-ONLY module \
         (it carries a \"use server\" directive or imports server-only code such as \
         server-only, next/headers, next/server, or node:fs / node:child_process; see the \
         sink hop in the trace). Candidate for verification: confirm whether this server-only \
         code is meant to run on the client. If it is pulled in only through \
         next/dynamic(..., { ssr: false }), it is the sanctioned client-only escape hatch and \
         is a false positive."
        .to_owned();

    let candidate = client_leak_candidate(
        anchor.path.clone(),
        anchor.line,
        anchor.col,
        Some(SERVER_ONLY_CATEGORY.to_owned()),
    );

    SecurityFinding {
        finding_id: String::new(),
        kind: SecurityFindingKind::ClientServerLeak,
        category: Some(SERVER_ONLY_CATEGORY.to_owned()),
        cwe: None,
        path: candidate.sink.path.clone(),
        line: candidate.sink.line,
        col: candidate.sink.col,
        evidence,
        source_backed: false,
        source_read: None,
        severity: SecuritySeverity::Low,
        trace,
        actions: build_actions(),
        dead_code: None,
        reachability: None,
        candidate,
        taint_flow: None,
        runtime: None,
        attack_surface: None,
    }
}

/// Build the candidate record for a `client-server-leak` finding. These findings
/// carry no source kind (graph-structural, not source-to-sink) and no callee;
/// the sink slot anchors on the client boundary file. The boundary slot starts
/// at its default and is filled by the post-detection ranking pass. `category`
/// is `None` for the secret-leak finding and `Some("server-only-import")` for the
/// server-only finding, mirroring the finding's top-level `category`.
fn client_leak_candidate(
    path: std::path::PathBuf,
    line: u32,
    col: u32,
    category: Option<String>,
) -> SecurityCandidate {
    SecurityCandidate {
        source_kind: None,
        sink: SecurityCandidateSink {
            path,
            line,
            col,
            category,
            cwe: None,
            callee: None,
            url_shape: None,
        },
        boundary: SecurityCandidateBoundary::default(),
        network: None,
    }
}

/// Build a finding for the direct case: a `"use client"` file that itself reads
/// a non-public secret. Trace is a single hop on the client file, which is both
/// the boundary and the secret source.
fn build_direct_finding(
    graph: &ModuleGraph,
    client_id: FileId,
    secret_sources: &FxHashMap<FileId, Vec<String>>,
) -> SecurityFinding {
    let path = graph.modules[client_id.0 as usize].path.clone();
    let empty = Vec::new();
    let var_list = secret_sources.get(&client_id).unwrap_or(&empty);
    let vars = var_list.join(", ");
    let word = secret_word(var_list.len());
    let evidence = format!(
        "This \"use client\" file directly reads non-public env {word}: {vars}. \
         Candidate for verification: confirm the secret value actually reaches client-bundled \
         code (it may be guarded, server-only, or build-time-stripped)."
    );
    let candidate = client_leak_candidate(path.clone(), 1, 0, None);
    SecurityFinding {
        finding_id: String::new(),
        kind: SecurityFindingKind::ClientServerLeak,
        category: None,
        cwe: None,
        path: path.clone(),
        line: 1,
        col: 0,
        evidence,
        source_backed: false,
        // client-server-leak is module-level by construction (no arg-level read).
        source_read: None,
        severity: SecuritySeverity::Low,
        trace: vec![TraceHop {
            path,
            line: 1,
            col: 0,
            role: TraceHopRole::SecretSource,
        }],
        actions: build_actions(),
        dead_code: None,
        reachability: None,
        candidate,
        taint_flow: None,
        runtime: None,
        attack_surface: None,
    }
}

/// Build a finding for the direct SERVER-ONLY case: a `"use client"` file that is
/// itself a server-only sink (carries `"use server"`, imports a server-only
/// package, or imports a server-only `next/headers` API). No import hop is needed,
/// so the trace is a single self-hop on the client file, which is both the client
/// boundary and the server-only sink. Mirrors [`build_direct_finding`] for the
/// secret case; distinguished by `category: Some("server-only-import")`.
fn build_direct_server_only_finding(graph: &ModuleGraph, client_id: FileId) -> SecurityFinding {
    let path = graph.modules[client_id.0 as usize].path.clone();
    let evidence = "This \"use client\" file directly imports SERVER-ONLY code \
         (it carries a \"use server\" directive or imports server-only code such as \
         server-only, next/headers, next/server, or node:fs / node:child_process). Candidate \
         for verification: confirm whether this server-only code is meant to run on the client."
        .to_owned();
    let candidate =
        client_leak_candidate(path.clone(), 1, 0, Some(SERVER_ONLY_CATEGORY.to_owned()));
    SecurityFinding {
        finding_id: String::new(),
        kind: SecurityFindingKind::ClientServerLeak,
        category: Some(SERVER_ONLY_CATEGORY.to_owned()),
        cwe: None,
        path: path.clone(),
        line: 1,
        col: 0,
        evidence,
        source_backed: false,
        source_read: None,
        severity: SecuritySeverity::Low,
        trace: vec![TraceHop {
            path,
            line: 1,
            col: 0,
            role: TraceHopRole::Sink,
        }],
        actions: build_actions(),
        dead_code: None,
        reachability: None,
        candidate,
        taint_flow: None,
        runtime: None,
        attack_surface: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn public_env_vars_are_not_secrets() {
        assert!(is_public_env_var("NODE_ENV"));
        assert!(is_public_env_var("NEXT_PUBLIC_API_URL"));
        assert!(is_public_env_var("VITE_TITLE"));
        assert!(is_public_env_var("PUBLIC_SITE_NAME"));
        assert!(is_public_env_var("EXPO_PUBLIC_KEY"));
    }

    #[test]
    fn real_secrets_are_not_public() {
        assert!(!is_public_env_var("DATABASE_URL"));
        assert!(!is_public_env_var("STRIPE_SECRET_KEY"));
        assert!(!is_public_env_var("SESSION_SECRET"));
        // A var that merely contains a public token mid-name is still a secret.
        assert!(!is_public_env_var("MY_NEXT_PUBLIC_FAKE"));
    }
}
