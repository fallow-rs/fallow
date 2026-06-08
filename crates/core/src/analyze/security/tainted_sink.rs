//! Catalogue-driven tainted-sink candidate detector (opt-in, `fallow security`).
//!
//! Matches category-blind [`SinkSite`]s captured
//! by the extract layer against the data-driven catalogue
//! (`security_matchers.toml`). Findings are CANDIDATES for downstream agent
//! verification, NOT verified vulnerabilities: detection is deterministic and
//! syntactic, never taint-proof.
//!
//! Blind spots (sink-shaped nodes whose callee could not be flattened to a
//! static path) are surfaced in-band via [`TaintedSinkStats`], never silently
//! dropped: an empty finding set with a non-zero count is not a clean bill.

use std::sync::OnceLock;

use rustc_hash::FxHashMap;

use fallow_types::extract::{
    ModuleInfo, SanitizedSinkArg, SanitizerScope, SinkSite, TaintedBinding,
};
use fallow_types::output::{IssueAction, SuppressFileAction, SuppressFileKind};
use fallow_types::results::{
    SecurityCandidate, SecurityCandidateBoundary, SecurityCandidateSink, SecurityFinding,
    SecurityFindingKind, TraceHop, TraceHopRole,
};
use fallow_types::suppress::IssueKind;

use super::catalogue::{Matcher, catalogue};
use super::{LineOffsetsMap, byte_offset_to_line_col};
use crate::discover::FileId;
use crate::graph::ModuleGraph;
use crate::suppress::SuppressionContext;

/// The inline suppression kind token for the tainted-sink catalogue rule. ONE
/// token covers every catalogue category.
pub(super) const SUPPRESS_KIND: &str = "security-sink";

/// Include/exclude scope over catalogue category ids. Built from
/// `config.security.categories`; both unset admits every category.
#[derive(Debug, Default, Clone)]
pub struct CategoryFilter {
    include: Option<Vec<String>>,
    exclude: Option<Vec<String>>,
}

impl CategoryFilter {
    /// Build a filter from the optional config include/exclude lists.
    #[must_use]
    pub fn new(include: Option<Vec<String>>, exclude: Option<Vec<String>>) -> Self {
        Self { include, exclude }
    }

    /// Whether the given category id is admitted. When `include` is set, only
    /// listed ids are admitted; `exclude` then removes ids from the set.
    #[must_use]
    pub fn admits(&self, id: &str) -> bool {
        if let Some(include) = &self.include
            && !include.iter().any(|c| c == id)
        {
            return false;
        }
        if let Some(exclude) = &self.exclude
            && exclude.iter().any(|c| c == id)
        {
            return false;
        }
        true
    }

    /// Whether an include-required category is explicitly admitted. An absent
    /// include list does not admit these categories.
    #[must_use]
    pub fn explicitly_admits(&self, id: &str) -> bool {
        let Some(include) = &self.include else {
            return false;
        };
        if !include.iter().any(|c| c == id) {
            return false;
        }
        if let Some(exclude) = &self.exclude
            && exclude.iter().any(|c| c == id)
        {
            return false;
        }
        true
    }
}

/// In-band blind-spot accounting for the tainted-sink detector.
#[derive(Debug, Default, Clone, Copy)]
pub struct TaintedSinkStats {
    /// Sink-shaped nodes whose callee could not be flattened to a static path
    /// (dynamic dispatch, computed members, aliased bindings), summed across
    /// scanned modules. Surfaced so an empty result is never reported clean.
    pub sinks_skipped_dynamic_callee: usize,
}

/// Build the machine-actionable file-level suppress hint emitted on every
/// finding (`auto_fixable: false`: verifying the candidate is the agent's job).
pub(super) fn build_actions() -> Vec<IssueAction> {
    vec![IssueAction::SuppressFile(SuppressFileAction {
        kind: SuppressFileKind::SuppressFile,
        auto_fixable: false,
        description: "Suppress with a file-level comment at the top of the file".to_string(),
        comment: format!("// fallow-ignore-file {SUPPRESS_KIND}"),
    })]
}

/// Whether the matcher's import provenance is satisfied by the module.
///
/// `None` provenance is always satisfied. Otherwise the module must import a
/// source matching the spec (tolerant of the `node:` prefix on either side).
/// For binding-sensitive rows, the binding's `local_name` must also be the
/// leading identifier of the callee path, matching the `child_process.fork()`
/// provenance precedent.
fn provenance_satisfied(matcher: &Matcher, module: &ModuleInfo, callee_path: &str) -> bool {
    let Some(spec) = &matcher.import_provenance else {
        return true;
    };
    let leading_ident = callee_path.split('.').next().unwrap_or(callee_path);
    let want_binding_trace = matches!(
        matcher.id.as_str(),
        "command-injection"
            | "permissive-cors"
            | "electron-unsafe-webpreferences"
            | "insecure-temp-file"
            | "jwt-alg-none"
            | "jwt-verify-missing-algorithms"
            | "tls-validation-disabled"
            | "mysql-multiple-statements"
            | "world-writable-permission"
    ) || (matcher.id == "weak-crypto" && matcher.is_literal_aware());
    module.imports.iter().any(|imp| {
        let source_matches = import_source_matches(&imp.source, spec);
        if !source_matches {
            return false;
        }
        if want_binding_trace {
            imp.local_name == leading_ident
        } else {
            true
        }
    })
}

/// Compare an import source against a provenance spec, tolerant of the `node:`
/// prefix on either side (`node:child_process` matches `child_process`) and
/// package subpath imports (`mysql2/promise` matches `mysql2`).
fn import_source_matches(source: &str, spec: &str) -> bool {
    fn strip_node_prefix(value: &str) -> &str {
        value.strip_prefix("node:").unwrap_or(value)
    }

    let source = strip_node_prefix(source);
    let spec = strip_node_prefix(spec);
    source == spec
        || source
            .strip_prefix(spec)
            .is_some_and(|rest| rest.starts_with('/'))
}

/// Compiled glob set over [`PRODUCTION_EXCLUDE_PATTERNS`](crate::discover::PRODUCTION_EXCLUDE_PATTERNS),
/// built once. Used to skip security candidates anchored in test / spec / story
/// / build-config files, matching the production-mode exclusion semantics
/// (`literal_separator(true)` so `*` cannot cross a path separator).
fn production_exclude_globset() -> &'static globset::GlobSet {
    static SET: OnceLock<globset::GlobSet> = OnceLock::new();
    SET.get_or_init(|| {
        let mut builder = globset::GlobSetBuilder::new();
        for pattern in crate::discover::PRODUCTION_EXCLUDE_PATTERNS {
            if let Ok(glob) = globset::GlobBuilder::new(pattern)
                .literal_separator(true)
                .build()
            {
                builder.add(glob);
            }
        }
        builder
            .build()
            .unwrap_or_else(|_| globset::GlobSet::empty())
    })
}

/// Whether a finding's anchor path is a low-value noise location for security
/// candidates: a test / spec / story / fixture file, or a tooling config file
/// (`vite.config.ts`, `jest.config.js`, etc.). Such files are excluded from
/// candidate generation, matching how production-mode dead-code exclusion drops
/// them. The match runs on the workspace-relative path (forward-slash
/// normalized) so the `**/` globs anchor consistently across platforms; the
/// config-file predicate is filename-only and is reused verbatim.
pub(super) fn is_low_value_anchor(path: &std::path::Path) -> bool {
    let normalized = path.to_string_lossy().replace('\\', "/");
    production_exclude_globset().is_match(&normalized)
        || crate::analyze::predicates::is_config_file(path)
}

/// Map each local binding name that was sourced from a known untrusted source
/// path (issue #859) to that source's human title. Computed by matching each
/// captured `tainted_binding.source_path` against the catalogue's `[[source]]`
/// patterns. A sink whose argument references one of these names is
/// "source-backed", and the matched source title names the input class.
fn source_tainted_locals<'b>(
    bindings: &'b [TaintedBinding],
    declared_deps: &rustc_hash::FxHashSet<String>,
) -> FxHashMap<&'b str, (&'static str, &'static str)> {
    let cat = catalogue();
    let mut out: FxHashMap<&'b str, (&'static str, &'static str)> = FxHashMap::default();
    for b in bindings {
        // `matching_source_for_deps` borrows from the `'static` catalogue, so
        // the (id, title) pair outlives the per-module computation. The id is
        // the stable machine source kind (`http-request-input`); the title is
        // the human phrase woven into the evidence string.
        if let Some(found) = cat.matching_source_for_deps(&b.source_path, declared_deps) {
            out.entry(b.local.as_str()).or_insert(found);
        }
    }
    out
}

fn is_html_sanitizable_category(id: &str) -> bool {
    matches!(id, "dangerous-html" | "dom-document-write" | "jquery-html")
}

fn is_url_sanitizable_category(id: &str) -> bool {
    matches!(id, "open-redirect" | "nextjs-open-redirect" | "ssrf")
}

fn is_path_sanitizable_category(id: &str) -> bool {
    matches!(id, "path-traversal" | "route-send-file" | "zip-slip")
}

fn has_direct_sanitizer(sink: &SinkSite, args: &[SanitizedSinkArg], scope: SanitizerScope) -> bool {
    args.iter().any(|arg| {
        arg.span_start == sink.span_start && arg.arg_index == sink.arg_index && arg.scope == scope
    })
}

fn sink_has_sanitizer(module: &ModuleInfo, sink: &SinkSite, scope: SanitizerScope) -> bool {
    has_direct_sanitizer(sink, &module.sanitized_sink_args, scope)
}

fn has_direct_html_sanitizer(sink: &SinkSite, args: &[SanitizedSinkArg]) -> bool {
    args.iter().any(|arg| {
        arg.span_start == sink.span_start
            && arg.arg_index == sink.arg_index
            && arg.scope == SanitizerScope::Html
    })
}

fn sink_has_html_sanitizer(module: &ModuleInfo, sink: &SinkSite) -> bool {
    has_direct_html_sanitizer(sink, &module.sanitized_sink_args)
}

/// The matched source `(id, title)` if any of a sink's captured argument
/// identifiers trace to a source-tainted local binding, else `None`. The id is
/// the stable catalogue source kind (slot 1 of the candidate record); the title
/// is the human phrase for the evidence string. The intra-module, name-based
/// back-trace from issue #859.
fn sink_source<'t>(
    sink: &SinkSite,
    tainted: &FxHashMap<&str, (&'t str, &'t str)>,
    declared_deps: &rustc_hash::FxHashSet<String>,
) -> Option<(&'t str, &'t str)> {
    let cat = catalogue();
    if let Some(found) = sink
        .arg_source_paths
        .iter()
        .find_map(|path| cat.matching_source_for_deps(path, declared_deps))
    {
        return Some(found);
    }

    if !tainted.is_empty()
        && let Some(found) = sink
            .arg_idents
            .iter()
            .find_map(|name| tainted.get(name.as_str()).copied())
    {
        return Some(found);
    }

    None
}

fn matcher_admits_sink(matcher: &Matcher, sink: &SinkSite, source_title: Option<&str>) -> bool {
    matcher.sink_shape == sink.sink_shape
        && matcher.arg_index == sink.arg_index
        && (sink.arg_is_non_literal || matcher.is_literal_aware())
        && matcher.admits_arg_kind(sink.arg_kind)
        && matcher.literal_value_satisfied(sink.arg_literal.as_ref())
        && matcher.object_properties_satisfied(&sink.object_properties)
        && matcher.object_missing_satisfied(
            &sink.object_property_keys,
            sink.object_property_keys_complete,
        )
        && matcher.context_satisfied(&sink.arg_idents)
        && (!matcher.requires_source || source_title.is_some())
        && matcher.first_matching_pattern(&sink.callee_path).is_some()
}

/// Run the catalogue-driven tainted-sink detector. Returns the findings plus the
/// in-band blind-spot stats. Callers gate this on the `security_sink` rule
/// severity; it never runs under bare `fallow` or the `audit` gate.
#[must_use]
pub fn find_tainted_sinks(
    graph: &ModuleGraph,
    modules: &[ModuleInfo],
    suppressions: &SuppressionContext<'_>,
    line_offsets_by_file: &LineOffsetsMap<'_>,
    category_filter: &CategoryFilter,
    declared_deps: &rustc_hash::FxHashSet<String>,
    root: &std::path::Path,
) -> (Vec<SecurityFinding>, TaintedSinkStats) {
    let mut stats = TaintedSinkStats::default();

    // Pre-filter the catalogue by the category scope AND the framework enabler
    // gate (#861). `enabler_satisfied` depends only on the project's declared
    // dependency set, not the per-module state, so it is hoisted here: a
    // framework-scoped row whose enabler package is absent never participates.
    // Empty -> nothing to do.
    let active: Vec<&Matcher> = catalogue()
        .matchers()
        .iter()
        .filter(|m| category_filter.admits(&m.id) && m.enabler_satisfied(declared_deps))
        .collect();
    if active.is_empty() {
        return (Vec::new(), stats);
    }

    let modules_by_id: FxHashMap<FileId, &ModuleInfo> =
        modules.iter().map(|m| (m.file_id, m)).collect();

    let mut findings = Vec::new();
    for node in &graph.modules {
        let Some(module) = modules_by_id.get(&node.file_id) else {
            continue;
        };
        // Always count the module's blind spots, even when it has no sinks.
        stats.sinks_skipped_dynamic_callee += module.security_sinks_skipped as usize;
        if module.security_sinks.is_empty() {
            continue;
        }
        // Skip test / spec / story / fixture files and tooling config files
        // (`vite.config.ts`, `jest.config.js`, etc.). A sink there is low-value
        // noise: build configs run at build time and test files exercise code
        // with synthetic inputs, neither is an attacker-reachable surface. This
        // mirrors the production-mode dead-code exclusion. Matching runs on the
        // PROJECT-RELATIVE path so the `**/tests/**` glob does not catch every
        // file when the project itself lives under a `tests/` directory.
        let rel_path = node.path.strip_prefix(root).unwrap_or(&node.path);
        if is_low_value_anchor(rel_path) {
            continue;
        }
        let file_id = node.file_id;
        // File-level suppression opts the whole file out. Routed through the
        // SuppressionContext so the marker is recorded as consumed (otherwise a
        // working suppression would later be flagged stale).
        if suppressions.is_file_suppressed(file_id, IssueKind::SecuritySink) {
            continue;
        }

        // Source-tainted local names for this module (issue #859). Computed once
        // per module; empty for modules with no source-shaped bindings.
        let tainted_locals = source_tainted_locals(&module.tainted_bindings, declared_deps);

        for sink in &module.security_sinks {
            let source = sink_source(sink, &tainted_locals, declared_deps);
            let Some(matcher) = active.iter().copied().find(|m| {
                matcher_admits_sink(m, sink, source.map(|(_, title)| title))
                    && provenance_satisfied(m, module, &sink.callee_path)
            }) else {
                continue;
            };

            if is_html_sanitizable_category(&matcher.id) && sink_has_html_sanitizer(module, sink) {
                continue;
            }
            if is_url_sanitizable_category(&matcher.id)
                && sink_has_sanitizer(module, sink, SanitizerScope::Url)
            {
                continue;
            }
            if is_path_sanitizable_category(&matcher.id)
                && sink_has_sanitizer(module, sink, SanitizerScope::Path)
            {
                continue;
            }

            let (line, col) =
                byte_offset_to_line_col(line_offsets_by_file, file_id, sink.span_start);
            if suppressions.is_suppressed(file_id, line, IssueKind::SecuritySink) {
                continue;
            }

            let pattern = matcher
                .first_matching_pattern(&sink.callee_path)
                .map_or("", super::catalogue::CalleePattern::raw);
            // The `{callee}` / `{pattern}` tokens are catalogue placeholders, not
            // Rust format args; the clippy lint misreads the literal.
            #[expect(
                clippy::literal_string_with_formatting_args,
                reason = "catalogue evidence placeholders, not format args"
            )]
            let base_evidence = matcher
                .evidence_template
                .replace("{callee}", &sink.callee_path)
                .replace("{pattern}", pattern)
                .replace(
                    "{regex}",
                    sink.regex_pattern.as_deref().unwrap_or("unknown"),
                );

            let source_backed = source.is_some();
            // Annotate the evidence when source-backed so the ranking signal is
            // visible in every output format (the boolean drives ordering; the
            // prefix names the matched untrusted-input class as the rationale).
            let evidence = match source {
                Some((_, title)) => format!(
                    "Untrusted source reaches this sink (an argument traces to {}). {base_evidence}",
                    title.to_ascii_lowercase()
                ),
                None => base_evidence,
            };

            // Slot 1 (source kind) is the stable catalogue source id; slot 2
            // (sink) carries the callee path the evidence already names. The
            // boundary slot is filled by the post-detection ranking pass once
            // reachability is known. See issue #900.
            let candidate = SecurityCandidate {
                source_kind: source.map(|(id, _)| id.to_string()),
                sink: SecurityCandidateSink {
                    path: node.path.clone(),
                    line,
                    col,
                    category: Some(matcher.id.clone()),
                    cwe: Some(matcher.cwe),
                    callee: Some(sink.callee_path.clone()),
                },
                boundary: SecurityCandidateBoundary::default(),
            };

            let path = node.path.clone();
            findings.push(SecurityFinding {
                finding_id: String::new(),
                kind: SecurityFindingKind::TaintedSink,
                category: Some(matcher.id.clone()),
                cwe: Some(matcher.cwe),
                path: path.clone(),
                line,
                col,
                evidence,
                source_backed,
                trace: vec![TraceHop {
                    path,
                    line,
                    col,
                    role: TraceHopRole::Sink,
                }],
                actions: build_actions(),
                dead_code: None,
                reachability: None,
                candidate,
                taint_flow: None,
                runtime: None,
                attack_surface: None,
            });
        }
    }

    // Rank source-backed candidates first (issue #859): a sink whose argument
    // traces to a known untrusted source is the higher-precision lead. Ties fall
    // back to the stable path/line/col/category order so output is deterministic.
    findings.sort_by(|a, b| {
        b.source_backed
            .cmp(&a.source_backed)
            .then(a.path.cmp(&b.path))
            .then(a.line.cmp(&b.line))
            .then(a.col.cmp(&b.col))
            .then(a.category.cmp(&b.category))
    });
    (findings, stats)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustc_hash::FxHashSet;

    #[test]
    fn category_filter_default_admits_all() {
        let f = CategoryFilter::default();
        assert!(f.admits("dangerous-html"));
        assert!(f.admits("anything"));
    }

    #[test]
    fn category_filter_include_scopes() {
        let f = CategoryFilter::new(Some(vec!["dangerous-html".to_string()]), None);
        assert!(f.admits("dangerous-html"));
        assert!(!f.admits("sql-injection"));
    }

    #[test]
    fn category_filter_exclude_removes() {
        let f = CategoryFilter::new(None, Some(vec!["sql-injection".to_string()]));
        assert!(f.admits("dangerous-html"));
        assert!(!f.admits("sql-injection"));
    }

    #[test]
    fn import_source_matches_node_prefix() {
        assert!(import_source_matches("node:child_process", "child_process"));
        assert!(import_source_matches("child_process", "node:child_process"));
        assert!(!import_source_matches("child_process", "node:vm"));
    }

    #[test]
    fn import_source_matches_package_subpath() {
        assert!(import_source_matches("mysql2/promise", "mysql2"));
        assert!(import_source_matches("@scope/pkg/subpath", "@scope/pkg"));
        assert!(!import_source_matches("mysql2-promise", "mysql2"));
    }

    fn binding(local: &str, source_path: &str) -> TaintedBinding {
        TaintedBinding {
            local: local.to_string(),
            source_path: source_path.to_string(),
        }
    }

    fn sink_with_idents_and_sources(idents: &[&str], source_paths: &[&str]) -> SinkSite {
        SinkSite {
            sink_shape: fallow_types::extract::SinkShape::Call,
            callee_path: "eval".to_string(),
            arg_index: 0,
            arg_is_non_literal: true,
            arg_kind: fallow_types::extract::SinkArgKind::Other,
            arg_literal: None,
            regex_pattern: None,
            object_properties: Vec::new(),
            object_property_keys: Vec::new(),
            object_property_keys_complete: false,
            arg_idents: idents.iter().map(|s| (*s).to_string()).collect(),
            arg_source_paths: source_paths.iter().map(|s| (*s).to_string()).collect(),
            span_start: 0,
            span_end: 1,
        }
    }

    fn sink_with_idents(idents: &[&str]) -> SinkSite {
        sink_with_idents_and_sources(idents, &[])
    }

    #[test]
    fn source_tainted_locals_match_catalogue_sources() {
        // `const id = req.query.id` is source-tainted; `const cfg = config.value`
        // is not (config.value is not an untrusted-source path).
        let bindings = vec![binding("id", "req.query"), binding("cfg", "config.value")];
        let tainted = source_tainted_locals(&bindings, &FxHashSet::default());
        assert!(tainted.contains_key("id"));
        assert!(!tainted.contains_key("cfg"));
        // The matched local carries the source's (id, title) pair: the stable
        // catalogue source kind plus the human title.
        assert_eq!(
            tainted.get("id").copied(),
            Some(("http-request-input", "HTTP request input"))
        );
    }

    #[test]
    fn sink_is_source_backed_when_arg_traces_to_source() {
        let bindings = vec![binding("id", "req.query")];
        let tainted = source_tainted_locals(&bindings, &FxHashSet::default());
        // `eval(id)` traces to the source-tainted `id`; the returned pair carries
        // both the catalogue source id (slot 1) and the human title.
        assert_eq!(
            sink_source(&sink_with_idents(&["id"]), &tainted, &FxHashSet::default()),
            Some(("http-request-input", "HTTP request input"))
        );
        // `eval(other)` does not.
        assert_eq!(
            sink_source(
                &sink_with_idents(&["other"]),
                &tainted,
                &FxHashSet::default(),
            ),
            None
        );
    }

    #[test]
    fn sink_not_source_backed_with_no_tainted_locals() {
        let tainted = source_tainted_locals(&[], &FxHashSet::default());
        assert_eq!(
            sink_source(&sink_with_idents(&["id"]), &tainted, &FxHashSet::default()),
            None
        );
    }

    #[test]
    fn sink_is_source_backed_when_arg_source_path_matches_catalogue() {
        let tainted = source_tainted_locals(&[], &FxHashSet::default());
        assert_eq!(
            sink_source(
                &sink_with_idents_and_sources(&["process"], &["process.env.SECRET", "process.env"]),
                &tainted,
                &FxHashSet::default(),
            )
            .map(|(_, title)| title),
            Some("Environment secret")
        );
    }

    #[test]
    fn direct_source_path_precedes_broader_tainted_local_source() {
        let bindings = vec![binding("req", "framework.request")];
        let mut deps = FxHashSet::default();
        deps.insert("express".to_string());
        let tainted = source_tainted_locals(&bindings, &deps);

        assert_eq!(
            sink_source(
                &sink_with_idents_and_sources(&["req"], &["req.body"]),
                &tainted,
                &deps,
            )
            .map(|(_, title)| title),
            Some("HTTP request input")
        );
    }

    #[test]
    fn low_value_anchor_excludes_tests_and_configs() {
        use std::path::Path;
        // Test / spec / story / fixture files are excluded.
        assert!(is_low_value_anchor(Path::new("src/foo.test.ts")));
        assert!(is_low_value_anchor(Path::new("src/foo.spec.ts")));
        assert!(is_low_value_anchor(Path::new("src/Button.stories.tsx")));
        assert!(is_low_value_anchor(Path::new("test/helper.ts")));
        assert!(is_low_value_anchor(Path::new(
            "packages/app/__tests__/x.ts"
        )));
        // Tooling config files are excluded (filename predicate).
        assert!(is_low_value_anchor(Path::new("vite.config.ts")));
        assert!(is_low_value_anchor(Path::new(
            "packages/app/vite.config.ts"
        )));
        assert!(is_low_value_anchor(Path::new("jest.config.js")));
        // Ordinary source files are NOT excluded.
        assert!(!is_low_value_anchor(Path::new("src/sink.ts")));
        assert!(!is_low_value_anchor(Path::new("src/db/query.ts")));
        // An app-level config that is not a tooling config is NOT excluded.
        assert!(!is_low_value_anchor(Path::new("src/app/app.config.ts")));
    }
}
