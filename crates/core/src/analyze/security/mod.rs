//! Local security candidate detection (opt-in, surfaced only by `fallow security`).
//!
//! These are CANDIDATES for downstream agent verification, NOT verified
//! vulnerabilities. The MVP ships one graph-structural rule, `client-server-leak`:
//! a `"use client"` file that transitively imports a module reading a non-public
//! env secret. fallow emits the structural import-hop trace; it does not prove
//! the path is exploitable.
//!
//! Blind spots (surfaced in-band via [`UnresolvedEdgeStats`], not silently
//! dropped): the BFS follows only resolved static import edges, so dynamic
//! `import()` and unresolved specifiers can hide a real leak.

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
use crate::graph::ModuleGraph;
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
pub use catalogue::CalleePattern;

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

    find_client_server_leaks(
        graph,
        &modules_by_id,
        &secret_sources,
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

/// For each `"use client"` file, BFS its transitive static-import cone; if it
/// reaches a secret source, emit one finding anchored on the client file with
/// the shortest-path trace. Type-only edges are skipped (erased at build, so
/// they cannot carry a secret into the client bundle).
fn find_client_server_leaks(
    graph: &ModuleGraph,
    modules_by_id: &FxHashMap<FileId, &ModuleInfo>,
    secret_sources: &FxHashMap<FileId, Vec<String>>,
    suppressions: &SuppressionContext<'_>,
    line_offsets_by_file: &LineOffsetsMap<'_>,
) -> (Vec<SecurityFinding>, UnresolvedEdgeStats) {
    let mut findings = Vec::new();
    let mut stats = UnresolvedEdgeStats::default();

    for node in &graph.modules {
        let Some(module) = modules_by_id.get(&node.file_id) else {
            continue;
        };
        if !module.directives.iter().any(|d| d == USE_CLIENT) {
            continue;
        }
        let client_id = node.file_id;
        // A file-level `// fallow-ignore-file security-client-server-leak`
        // (or a blanket file ignore) opts the whole client file out. Routed
        // through the SuppressionContext so the marker is recorded as consumed
        // (otherwise a working suppression would later be flagged stale).
        if suppressions.is_file_suppressed(client_id, IssueKind::SecurityClientServerLeak) {
            continue;
        }

        // Direct case: the client file itself reads a non-public secret. The
        // most direct leak; no import hop needed.
        if secret_sources.contains_key(&client_id) {
            findings.push(build_direct_finding(graph, client_id, secret_sources));
            // Still count its dynamic-import blind spot below.
        }

        // Transitive case: BFS the import cone. parent maps child ->
        // (parent, span_start of the parent's import).
        let mut visited: FxHashSet<FileId> = FxHashSet::default();
        visited.insert(client_id);
        let mut parent: FxHashMap<FileId, (FileId, Option<u32>)> = FxHashMap::default();
        let mut queue: VecDeque<FileId> = VecDeque::new();
        queue.push_back(client_id);
        let mut had_unresolved_edge = false;
        let mut reached_secret: Option<FileId> = None;

        // Drain the FULL cone (no early break): the first secret reached via BFS
        // is the shortest-path trace, but we keep going so the dynamic-import
        // blind-spot count reflects the whole cone, not just the path to the
        // first secret (otherwise a leak hidden behind a dynamic edge past the
        // first finding would be silently uncounted).
        while let Some(current) = queue.pop_front() {
            if let Some(current_module) = modules_by_id.get(&current)
                && !current_module.dynamic_import_patterns.is_empty()
            {
                had_unresolved_edge = true;
            }

            if current != client_id
                && reached_secret.is_none()
                && secret_sources.contains_key(&current)
            {
                reached_secret = Some(current);
            }

            for (target, all_type_only, span_start) in graph.outgoing_edge_summaries(current) {
                if all_type_only {
                    continue; // type-only imports are erased at build; cannot leak.
                }
                if visited.insert(target) {
                    parent.insert(target, (current, span_start));
                    queue.push_back(target);
                }
            }
        }

        if had_unresolved_edge {
            stats.client_files_with_unresolved_edges += 1;
        }

        // Only emit a transitive finding when the client is not ALREADY flagged
        // by the direct case (avoid double-flagging the same file).
        if let Some(secret_id) = reached_secret
            && !secret_sources.contains_key(&client_id)
        {
            findings.push(build_leak_finding(
                graph,
                client_id,
                secret_id,
                &parent,
                secret_sources,
                line_offsets_by_file,
            ));
        }
    }

    findings.sort_by(|a, b| a.path.cmp(&b.path).then(a.line.cmp(&b.line)));
    (findings, stats)
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
    // Walk parent pointers from the secret back to the client, then reverse.
    let mut chain: Vec<FileId> = vec![secret_id];
    let mut cursor = secret_id;
    while let Some(&(prev, _)) = parent.get(&cursor) {
        chain.push(prev);
        cursor = prev;
        if prev == client_id {
            break;
        }
    }
    chain.reverse(); // now [client_id, ..., secret_id]

    let mut trace: Vec<TraceHop> = Vec::with_capacity(chain.len());
    for (idx, &file_id) in chain.iter().enumerate() {
        let role = if idx == 0 {
            TraceHopRole::ClientBoundary
        } else if file_id == secret_id {
            TraceHopRole::SecretSource
        } else {
            TraceHopRole::Intermediate
        };
        // The line for a non-terminal hop is where it imports the NEXT hop.
        // Member-access extraction does not yet carry spans, so the terminal
        // secret-source hop falls back to the source module start.
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
    let candidate = client_leak_candidate(anchor.path.clone(), anchor.line, anchor.col);

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

/// Build the candidate record for a `client-server-leak` finding. These findings
/// carry no source kind (graph-structural, not source-to-sink) and no callee;
/// the sink slot anchors on the client boundary file. The boundary slot starts
/// at its default and is filled by the post-detection ranking pass.
fn client_leak_candidate(path: std::path::PathBuf, line: u32, col: u32) -> SecurityCandidate {
    SecurityCandidate {
        source_kind: None,
        sink: SecurityCandidateSink {
            path,
            line,
            col,
            category: None,
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
    let candidate = client_leak_candidate(path.clone(), 1, 0);
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
