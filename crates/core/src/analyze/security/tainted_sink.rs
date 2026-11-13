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

use rustc_hash::{FxHashMap, FxHashSet};

use fallow_extract::TEMPLATE_SINK_CALLEE_PREFIX;
use fallow_types::extract::{
    ModuleInfo, SanitizedSinkArg, SanitizerScope, SecurityUrlShape, SinkSite, TaintedBinding,
};
use fallow_types::output::{IssueAction, SuppressFileAction, SuppressFileKind};
use fallow_types::results::{
    SecurityCandidate, SecurityCandidateBoundary, SecurityCandidateSink, SecurityFinding,
    SecurityFindingKind, SecurityNetworkContext, SecuritySeverity,
    SecurityUnresolvedCalleeDiagnostic, TraceHop, TraceHopRole,
};
use fallow_types::security_stable_id::SecurityStableIdInput;
use fallow_types::suppress::IssueKind;

use super::rank::module_input_source_kinds;
use super::{LineOffsetsMap, byte_offset_to_line_col};
use crate::discover::FileId;
use crate::graph::{ModuleGraph, ModuleNode};
use crate::suppress::SuppressionContext;
use fallow_security::{CalleePattern, Matcher, ProvenanceBindings, catalogue};

/// The inline suppression kind token for the tainted-sink catalogue rule. ONE
/// token covers every catalogue category.
const SUPPRESS_KIND: &str = "security-sink";

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
#[derive(Debug, Default, Clone)]
pub struct TaintedSinkStats {
    /// Sink-shaped nodes whose callee could not be flattened to a static path
    /// (dynamic dispatch, computed members, aliased bindings), summed across
    /// scanned modules. Surfaced so an empty result is never reported clean.
    pub sinks_skipped_dynamic_callee: usize,
    /// Location and reason metadata for skipped sink-shaped callees.
    pub unresolved_callee_diagnostics: Vec<SecurityUnresolvedCalleeDiagnostic>,
    /// Findings that an inline suppression comment hides. The stable id step
    /// counts them for the tiebreak ordinal only.
    pub suppressed_findings: Vec<SecurityFinding>,
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
    declared_deps: &FxHashSet<String>,
    request_receivers: &FxHashSet<String>,
) -> FxHashMap<&'b str, (&'static str, &'static str, u32)> {
    let cat = catalogue();
    let mut out: FxHashMap<&'b str, (&'static str, &'static str, u32)> = FxHashMap::default();
    for b in bindings {
        // `matching_source_for_deps_with_receivers` borrows from the `'static` catalogue, so
        // the (id, title) pair outlives the per-module computation. The id is
        // the stable machine source kind (`http-request-input`); the title is
        // the human phrase woven into the evidence string. The third element is
        // the binding's source-read byte offset (`0` for synthetic
        // framework-param / helper-return bindings with no concrete read), used
        // to anchor an arg-level taint trace's source node (issue #1093).
        if let Some((id, title)) = cat.matching_source_for_deps_with_receivers(
            &b.source_path,
            declared_deps,
            request_receivers,
        ) {
            out.entry(b.local.as_str())
                .or_insert((id, title, b.source_span_start));
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

fn candidate_url_shape(category: &str, sink: &SinkSite) -> Option<SecurityUrlShape> {
    is_url_sanitizable_category(category)
        .then_some(sink.url_shape)
        .flatten()
}

fn tainted_sink_evidence(
    matcher: &Matcher,
    sink: &SinkSite,
    source_title: Option<&str>,
    url_shape: Option<SecurityUrlShape>,
) -> String {
    let pattern = matcher
        .first_matching_pattern(&sink.callee_path)
        .map_or("", CalleePattern::raw);
    // The `{callee}` / `{pattern}` tokens are catalogue placeholders, not Rust
    // format args; the clippy lint misreads the literal.
    #[expect(
        clippy::literal_string_with_formatting_args,
        reason = "catalogue evidence placeholders, not format args"
    )]
    let mut evidence = matcher
        .evidence_template
        .replace("{callee}", &sink.callee_path)
        .replace("{pattern}", pattern)
        .replace(
            "{regex}",
            sink.regex_pattern.as_deref().unwrap_or("unknown"),
        );
    if matches!(url_shape, Some(SecurityUrlShape::FixedOriginDynamicPath)) {
        evidence = format!(
            "Fixed-origin dynamic URL at {}. Verify the dynamic path or query encoding and destination policy. {evidence}",
            sink.callee_path
        );
    }
    match source_title {
        Some(title) => format!(
            "Untrusted source reaches this sink (an argument traces to {}). {evidence}",
            title.to_ascii_lowercase()
        ),
        None => evidence,
    }
}

fn is_path_sanitizable_category(id: &str) -> bool {
    matches!(id, "path-traversal" | "route-send-file" | "zip-slip")
}

fn is_sql_identifier_sanitizable_category(id: &str) -> bool {
    id == "sql-injection"
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

/// The first matched source `(id, title)` of a sink's captured arguments, else
/// `None`. See [`sink_sources`].
#[cfg(test)]
fn sink_source<'t>(
    sink: &SinkSite,
    tainted: &FxHashMap<&str, (&'t str, &'t str, u32)>,
    declared_deps: &FxHashSet<String>,
    request_receivers: &FxHashSet<String>,
) -> Option<(&'t str, &'t str, Option<u32>)> {
    sink_sources(sink, tainted, declared_deps, request_receivers)
        .into_iter()
        .next()
}

/// Every matched source `(id, title)` of a sink's captured arguments, in
/// precedence order: reads inside the argument first, then source-tainted
/// local bindings. The id is the stable catalogue source kind (slot 1 of the
/// candidate record); the title is the human phrase for the evidence string.
/// The intra-module, name-based back-trace from issue #859. Each matcher takes
/// the first source it admits, so a deployment secret in the argument does not
/// hide request input next to it.
fn sink_sources<'t>(
    sink: &SinkSite,
    tainted: &FxHashMap<&str, (&'t str, &'t str, u32)>,
    declared_deps: &FxHashSet<String>,
    request_receivers: &FxHashSet<String>,
) -> Vec<(&'t str, &'t str, Option<u32>)> {
    let cat = catalogue();
    // Direct path: the source read is inside the sink argument (same statement),
    // so there is no separate binding span; the detector anchors at the sink.
    let mut sources: Vec<(&'t str, &'t str, Option<u32>)> = sink
        .arg_source_paths
        .iter()
        .filter_map(|path| {
            cat.matching_source_for_deps_with_receivers(path, declared_deps, request_receivers)
        })
        .map(|(id, title)| (id, title, None))
        .collect();

    // Binding path: the argument references a source-tainted local; carry that
    // binding's source-read byte offset so the trace can anchor at the read.
    if !tainted.is_empty() {
        sources.extend(
            sink.arg_idents
                .iter()
                .filter_map(|name| tainted.get(name.as_str()).copied())
                .map(|(id, title, span)| (id, title, Some(span))),
        );
    }
    sources
}

type TaintedSinkSource<'a> = (&'a str, &'a str, Option<u32>);

fn matcher_admits_sink(matcher: &Matcher, sink: &SinkSite, source: Option<(&str, &str)>) -> bool {
    matcher.sink_shape == sink.sink_shape
        && matcher.admits_arg_index(sink.arg_index)
        && (sink.arg_is_non_literal || matcher.is_literal_aware())
        && matcher.admits_arg_kind(sink.arg_kind)
        && matcher.literal_value_satisfied(sink.arg_literal.as_ref())
        && matcher.object_properties_satisfied(&sink.object_properties)
        && matcher.object_missing_satisfied(
            &sink.object_property_keys,
            sink.object_property_keys_complete,
        )
        && matcher.options_may_enable_satisfied(sink.call_options.as_deref())
        && matcher.context_satisfied(&sink.arg_idents)
        && (!matcher.requires_source || source.is_some())
        // Source-KIND gate (#890): when set, the matched source's id must be one
        // of the listed kinds. Lets `secret-to-network` fire only on a SECRET
        // source, not request input.
        && (matcher.requires_source_kinds.is_empty()
            || source.is_some_and(|(id, _)| matcher.requires_source_kinds.iter().any(|k| k == id)))
        && matcher.first_matching_pattern(&sink.callee_path).is_some()
}

/// The catalogue id of the secret-to-network exfil category (issue #890). The
/// only category that carries a [`SecurityNetworkContext`] destination signal.
const NETWORK_EXFIL_CATEGORY: &str = "secret-to-network";

/// Catalogue categories admitted ONLY when explicitly listed in
/// `security.categories.include` (issue #890). A `secret-to-network` candidate
/// fires on intended auth as often as on exfil, so it must be opt-in; an absent
/// include list never admits it.
const INCLUDE_REQUIRED_CATEGORIES: &[&str] = &[NETWORK_EXFIL_CATEGORY];

/// Shared options for the catalogue-driven tainted-sink detector.
pub struct TaintedSinkContext<'a> {
    pub category_filter: &'a CategoryFilter,
    pub request_receivers: &'a FxHashSet<String>,
    pub root: &'a std::path::Path,
}

/// Whether a catalogue category is include-required (opt-in only).
fn is_include_required_category(id: &str) -> bool {
    INCLUDE_REQUIRED_CATEGORIES.contains(&id)
}

fn active_matchers(
    category_filter: &CategoryFilter,
    declared_deps: &FxHashSet<String>,
) -> Vec<&'static Matcher> {
    catalogue()
        .matchers()
        .iter()
        .filter(|m| {
            // Include-required categories (#890) are admitted only when explicitly
            // listed in `security.categories.include`; everything else uses the
            // normal include/exclude scope.
            let admitted = if is_include_required_category(&m.id) {
                category_filter.explicitly_admits(&m.id)
            } else {
                category_filter.admits(&m.id)
            };
            admitted && m.enabler_satisfied(declared_deps)
        })
        .collect()
}

fn record_unresolved_callee_diagnostics(
    stats: &mut TaintedSinkStats,
    module: &ModuleInfo,
    node: &ModuleNode,
    line_offsets_by_file: &LineOffsetsMap<'_>,
) {
    stats.sinks_skipped_dynamic_callee += module.security_sinks_skipped as usize;
    stats
        .unresolved_callee_diagnostics
        .extend(module.security_unresolved_callee_sites.iter().map(|site| {
            let (line, col) =
                byte_offset_to_line_col(line_offsets_by_file, node.file_id, site.span_start);
            SecurityUnresolvedCalleeDiagnostic {
                path: node.path.clone(),
                line,
                col,
                reason: site.reason,
                expression_kind: site.expression_kind,
            }
        }));
}

fn sink_is_sanitized_for_matcher(matcher: &Matcher, module: &ModuleInfo, sink: &SinkSite) -> bool {
    (is_html_sanitizable_category(&matcher.id) && sink_has_html_sanitizer(module, sink))
        || (is_url_sanitizable_category(&matcher.id)
            && sink_has_sanitizer(module, sink, SanitizerScope::Url))
        || (is_path_sanitizable_category(&matcher.id)
            && sink_has_sanitizer(module, sink, SanitizerScope::Path))
        || (is_sql_identifier_sanitizable_category(&matcher.id)
            && sink_has_sanitizer(module, sink, SanitizerScope::SqlIdentifier))
}

fn source_read_location(
    span: Option<u32>,
    line_offsets_by_file: &LineOffsetsMap<'_>,
    file_id: FileId,
    sink_line_col: (u32, u32),
) -> (u32, u32) {
    match span {
        Some(offset) if offset != 0 => {
            byte_offset_to_line_col(line_offsets_by_file, file_id, offset)
        }
        _ => sink_line_col,
    }
}

fn build_tainted_sink_candidate(
    matcher: &Matcher,
    sink: &SinkSite,
    node: &ModuleNode,
    source: Option<(&str, &str, Option<u32>)>,
    sink_line_col: (u32, u32),
    url_shape: Option<SecurityUrlShape>,
) -> SecurityCandidate {
    let (line, col) = sink_line_col;
    let network = (matcher.id == NETWORK_EXFIL_CATEGORY).then(|| SecurityNetworkContext {
        destination: sink.url_arg_literal.clone(),
    });

    SecurityCandidate {
        source_kind: source.map(|(id, _, _)| id.to_string()),
        sink: SecurityCandidateSink {
            path: node.path.clone(),
            line,
            col,
            category: Some(matcher.id.clone()),
            cwe: Some(matcher.cwe),
            callee: Some(sink.callee_path.clone()),
            url_shape,
        },
        boundary: SecurityCandidateBoundary::default(),
        network,
    }
}

struct TaintedSinkFindingInput<'a> {
    matcher: &'a Matcher,
    sink: &'a SinkSite,
    node: &'a ModuleNode,
    source: Option<(&'a str, &'a str, Option<u32>)>,
    sink_line_col: (u32, u32),
    line_offsets_by_file: &'a LineOffsetsMap<'a>,
    file_id: FileId,
}

/// Build a `SecurityFinding` for one matched sink: evidence, source-read anchor,
/// network destination, candidate slots, and the single sink trace hop.
fn build_tainted_sink_finding(input: &TaintedSinkFindingInput<'_>) -> SecurityFinding {
    let matcher = input.matcher;
    let sink = input.sink;
    let node = input.node;
    let source = input.source;
    let (line, col) = input.sink_line_col;
    let url_shape = candidate_url_shape(&matcher.id, sink);
    let source_backed = source.is_some();
    let evidence =
        tainted_sink_evidence(matcher, sink, source.map(|(_, title, _)| title), url_shape);
    let source_read = tainted_sink_source_read(input);

    // Slot 1 (source kind) is the stable catalogue source id; slot 2
    // (sink) carries the callee path the evidence already names. The
    // boundary slot is filled by the post-detection ranking pass once
    // reachability is known. See issue #900.
    let candidate =
        build_tainted_sink_candidate(matcher, sink, node, source, (line, col), url_shape);

    let path = node.path.clone();
    SecurityFinding {
        finding_id: String::new(),
        stable_finding_id: None,
        stable_id_input: sink_stable_id_input(sink),
        kind: SecurityFindingKind::TaintedSink,
        category: Some(matcher.id.clone()),
        cwe: Some(matcher.cwe),
        path: path.clone(),
        line,
        col,
        evidence,
        source_backed,
        source_read,
        severity: SecuritySeverity::Low,
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
    }
}

/// The exact span of the sink expression for the stable id. A template sink
/// spans its tag or attribute, not the sink expression, so it gets no stable
/// id.
fn sink_stable_id_input(sink: &SinkSite) -> Option<SecurityStableIdInput> {
    if sink.callee_path.starts_with(TEMPLATE_SINK_CALLEE_PREFIX) || sink.span_end <= sink.span_start
    {
        return None;
    }
    Some(SecurityStableIdInput::SinkSpan {
        start: sink.span_start,
        end: sink.span_end,
    })
}

fn tainted_sink_source_read(input: &TaintedSinkFindingInput<'_>) -> Option<(u32, u32)> {
    // Arg-level source-read anchor (issue #1093): for a source-backed finding,
    // point the trace's source node at the real read. The binding path carries
    // the read's byte offset (`Some(span)`); a `0` span (synthetic
    // framework-param / helper-return source) and the direct path (`None`, the
    // read sits inside the sink statement) both fall back to the sink line/col
    // rather than a spurious line. `None` for module-level findings keeps the
    // trace honest (role `ModuleSource`, set by the ranking pass).
    input.source.map(|(_, _, span)| {
        source_read_location(
            span,
            input.line_offsets_by_file,
            input.file_id,
            input.sink_line_col,
        )
    })
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
    declared_deps: &FxHashSet<String>,
    context: &TaintedSinkContext<'_>,
) -> (Vec<SecurityFinding>, TaintedSinkStats) {
    let mut stats = TaintedSinkStats::default();

    // Pre-filter the catalogue by the category scope AND the framework enabler
    // gate (#861). `enabler_satisfied` depends only on the project's declared
    // dependency set, not the per-module state, so it is hoisted here: a
    // framework-scoped row whose enabler package is absent never participates.
    // Empty -> nothing to do.
    let active = active_matchers(context.category_filter, declared_deps);
    if active.is_empty() {
        return (Vec::new(), stats);
    }

    let modules_by_id: FxHashMap<FileId, &ModuleInfo> =
        modules.iter().map(|m| (m.file_id, m)).collect();

    let run = TaintedSinkRun {
        active: &active,
        graph,
        modules_by_id: &modules_by_id,
        suppressions,
        line_offsets_by_file,
        declared_deps,
        context,
    };

    let mut findings = Vec::new();
    for node in &graph.modules {
        let Some(module) = modules_by_id.get(&node.file_id) else {
            continue;
        };
        // Always count the module's blind spots, even when it has no sinks.
        record_unresolved_callee_diagnostics(&mut stats, module, node, line_offsets_by_file);
        collect_module_tainted_sinks(
            &run,
            node,
            module,
            &mut findings,
            &mut stats.suppressed_findings,
        );
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

/// Shared immutable inputs threaded through the per-module tainted-sink scan.
struct TaintedSinkRun<'a> {
    active: &'a [&'static Matcher],
    graph: &'a ModuleGraph,
    modules_by_id: &'a FxHashMap<FileId, &'a ModuleInfo>,
    suppressions: &'a SuppressionContext<'a>,
    line_offsets_by_file: &'a LineOffsetsMap<'a>,
    declared_deps: &'a FxHashSet<String>,
    context: &'a TaintedSinkContext<'a>,
}

/// Skip modules that cannot produce useful tainted-sink candidates. Low-value
/// anchors use project-relative paths, and suppression lookup goes through
/// `SuppressionContext` so file-level markers are recorded as consumed.
fn module_allows_tainted_sink_scan(
    run: &TaintedSinkRun<'_>,
    node: &ModuleNode,
    module: &ModuleInfo,
) -> bool {
    if module.security_sinks.is_empty() {
        return false;
    }

    let rel_path = node
        .path
        .strip_prefix(run.context.root)
        .unwrap_or(&node.path);
    if is_low_value_anchor(rel_path) {
        return false;
    }

    !run.suppressions
        .is_file_suppressed(node.file_id, IssueKind::SecuritySink)
}

/// Whether a matcher may use a source of kind `id`. A deployment secret backs
/// only the rows that name its kind in `requires_source_kinds` or set
/// `secret_sources`; for every other row it is not attacker input. A kind in
/// `ignored_source_kinds` never backs the row.
fn matcher_accepts_source_kind(matcher: &Matcher, id: &str) -> bool {
    !matcher.ignored_source_kinds.iter().any(|kind| kind == id)
        && (matcher.secret_sources
            || !matcher.requires_source_kinds.is_empty()
            || !catalogue().is_secret_source(id))
}

/// The source kinds of the untrusted input that one module and each of its
/// direct importers read. Computed on first use, because only a
/// `requires_module_source` row without an argument source needs them.
struct ModuleSourceKinds<'m> {
    module: &'m ModuleInfo,
    kinds: std::cell::OnceCell<FxHashSet<&'static str>>,
    importer_kinds: std::cell::OnceCell<Vec<FxHashSet<&'static str>>>,
}

impl<'m> ModuleSourceKinds<'m> {
    const fn new(module: &'m ModuleInfo) -> Self {
        Self {
            module,
            kinds: std::cell::OnceCell::new(),
            importer_kinds: std::cell::OnceCell::new(),
        }
    }

    fn get(&self, run: &TaintedSinkRun<'_>) -> &FxHashSet<&'static str> {
        self.kinds.get_or_init(|| {
            module_input_source_kinds(
                self.module,
                run.declared_deps,
                run.context.request_receivers,
            )
        })
    }

    /// The input kinds of each direct runtime importer of the module, one set
    /// per importer. A test, story or tooling-config importer is left out.
    fn importers(&self, run: &TaintedSinkRun<'_>) -> &[FxHashSet<&'static str>] {
        self.importer_kinds.get_or_init(|| {
            let target = self.module.file_id;
            let mut kinds = Vec::new();
            let Some(importers) = run.graph.reverse_deps.get(target.0 as usize) else {
                return kinds;
            };
            for &importer in importers {
                let Some(node) = run.graph.modules.get(importer.0 as usize) else {
                    continue;
                };
                let Some(module) = run.modules_by_id.get(&importer) else {
                    continue;
                };
                let rel_path = node
                    .path
                    .strip_prefix(run.context.root)
                    .unwrap_or(&node.path);
                let runtime_edge = run
                    .graph
                    .outgoing_edge_summaries(importer)
                    .any(|edge| edge.target == target && !edge.all_type_only);
                if is_low_value_anchor(rel_path) || !runtime_edge {
                    continue;
                }
                kinds.push(module_input_source_kinds(
                    module,
                    run.declared_deps,
                    run.context.request_receivers,
                ));
            }
            kinds
        })
    }
}

/// Whether a `requires_module_source` row may fire: the argument traces to a
/// source, the sink module reads input of a kind the row accepts, or every
/// direct runtime importer does.
///
/// A request handler often passes its input to a helper in another file, so
/// a helper that only handlers import is armed. A module that other code also
/// imports is not: a dev server imports its config and build pipeline, and
/// one request read there must not arm every path join in them. Only one
/// import hop counts for the same reason.
fn module_source_satisfied(
    run: &TaintedSinkRun<'_>,
    matcher: &Matcher,
    source: Option<TaintedSinkSource<'_>>,
    module_kinds: &ModuleSourceKinds<'_>,
) -> bool {
    let accepts = |kinds: &FxHashSet<&'static str>| {
        kinds
            .iter()
            .any(|kind| matcher_accepts_source_kind(matcher, kind))
    };
    !matcher.requires_module_source || source.is_some() || accepts(module_kinds.get(run)) || {
        let importers = module_kinds.importers(run);
        !importers.is_empty() && importers.iter().all(accepts)
    }
}

fn match_tainted_sink<'a>(
    run: &TaintedSinkRun<'_>,
    module_kinds: &ModuleSourceKinds<'_>,
    sink: &SinkSite,
    tainted_locals: &FxHashMap<&str, (&'a str, &'a str, u32)>,
) -> Option<(&'static Matcher, Option<TaintedSinkSource<'a>>)> {
    let module = module_kinds.module;
    let sources = sink_sources(
        sink,
        tainted_locals,
        run.declared_deps,
        run.context.request_receivers,
    );
    run.active.iter().copied().find_map(|m| {
        let source = sources
            .iter()
            .copied()
            .find(|(id, _, _)| matcher_accepts_source_kind(m, id));
        let source_id = source.map(|(id, title, _)| (id, title));
        (matcher_admits_sink(m, sink, source_id)
            && module_source_satisfied(run, m, source, module_kinds)
            && m.provenance_satisfied(module, &sink.callee_path, ProvenanceBindings::Imports))
        .then_some((m, source))
    })
}

/// Match every sink in one module against the active catalogue and push a
/// finding per admitted, non-sanitized sink. A sink that an inline comment
/// suppresses goes to `suppressed` instead. Low-value-anchor and
/// file-level-suppressed modules are skipped wholesale.
fn collect_module_tainted_sinks(
    run: &TaintedSinkRun<'_>,
    node: &ModuleNode,
    module: &ModuleInfo,
    findings: &mut Vec<SecurityFinding>,
    suppressed: &mut Vec<SecurityFinding>,
) {
    let file_id = node.file_id;
    if !module_allows_tainted_sink_scan(run, node, module) {
        return;
    }

    // Source-tainted local names for this module (issue #859). Computed once
    // per module; empty for modules with no source-shaped bindings.
    let tainted_locals = source_tainted_locals(
        &module.tainted_bindings,
        run.declared_deps,
        run.context.request_receivers,
    );

    // A variadic row (`path.join` segments, #3712) can admit several arguments
    // of one call. Keep one candidate per call and row, and prefer a
    // source-backed argument over a source-free one.
    let mut variadic_calls: FxHashMap<(u32, *const Matcher), usize> = FxHashMap::default();
    let mut suppressed_variadic_calls: FxHashSet<(u32, *const Matcher)> = FxHashSet::default();

    let module_kinds = ModuleSourceKinds::new(module);

    for sink in &module.security_sinks {
        let Some((matcher, source)) = match_tainted_sink(run, &module_kinds, sink, &tainted_locals)
        else {
            continue;
        };

        if sink_is_sanitized_for_matcher(matcher, module, sink) {
            continue;
        }

        let (line, col) =
            byte_offset_to_line_col(run.line_offsets_by_file, file_id, sink.span_start);
        let finding = build_tainted_sink_finding(&TaintedSinkFindingInput {
            matcher,
            sink,
            node,
            source,
            sink_line_col: (line, col),
            line_offsets_by_file: run.line_offsets_by_file,
            file_id,
        });
        if run
            .suppressions
            .is_suppressed(file_id, line, IssueKind::SecuritySink)
        {
            if !matcher.variadic
                || suppressed_variadic_calls.insert((sink.span_start, std::ptr::from_ref(matcher)))
            {
                suppressed.push(finding);
            }
            continue;
        }
        if !matcher.variadic {
            findings.push(finding);
            continue;
        }
        match variadic_calls.entry((sink.span_start, std::ptr::from_ref(matcher))) {
            std::collections::hash_map::Entry::Occupied(slot) => {
                let existing = &mut findings[*slot.get()];
                if finding.source_backed && !existing.source_backed {
                    *existing = finding;
                }
            }
            std::collections::hash_map::Entry::Vacant(slot) => {
                slot.insert(findings.len());
                findings.push(finding);
            }
        }
    }
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

    fn binding(local: &str, source_path: &str) -> TaintedBinding {
        TaintedBinding {
            local: local.to_string(),
            source_path: source_path.to_string(),
            source_span_start: 0,
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
            url_arg_literal: None,
            url_shape: None,
            call_options: None,
        }
    }

    fn sink_with_idents(idents: &[&str]) -> SinkSite {
        sink_with_idents_and_sources(idents, &[])
    }

    fn empty_receivers() -> FxHashSet<String> {
        FxHashSet::default()
    }

    #[test]
    fn source_tainted_locals_match_catalogue_sources() {
        // `const id = req.query.id` is source-tainted; `const cfg = config.value`
        // is not (config.value is not an untrusted-source path).
        let bindings = vec![binding("id", "req.query"), binding("cfg", "config.value")];
        let tainted = source_tainted_locals(&bindings, &FxHashSet::default(), &empty_receivers());
        assert!(tainted.contains_key("id"));
        assert!(!tainted.contains_key("cfg"));
        // The matched local carries the source's (id, title) pair: the stable
        // catalogue source kind plus the human title.
        assert_eq!(
            tainted.get("id").copied(),
            // Third element is the binding's source-read byte offset; the test
            // `binding` helper sets it to 0 (no concrete read span).
            Some(("http-request-input", "HTTP request input", 0))
        );
    }

    #[test]
    fn sink_is_source_backed_when_arg_traces_to_source() {
        let bindings = vec![binding("id", "req.query")];
        let tainted = source_tainted_locals(&bindings, &FxHashSet::default(), &empty_receivers());
        // `eval(id)` traces to the source-tainted `id`; the returned pair carries
        // both the catalogue source id (slot 1) and the human title.
        assert_eq!(
            sink_source(
                &sink_with_idents(&["id"]),
                &tainted,
                &FxHashSet::default(),
                &empty_receivers()
            ),
            // Binding path: carries the binding's source-read span (`Some(0)`
            // here, since the test `binding` helper sets the offset to 0).
            Some(("http-request-input", "HTTP request input", Some(0)))
        );
        // `eval(other)` does not.
        assert_eq!(
            sink_source(
                &sink_with_idents(&["other"]),
                &tainted,
                &FxHashSet::default(),
                &empty_receivers(),
            ),
            None
        );
    }

    #[test]
    fn sink_not_source_backed_with_no_tainted_locals() {
        let tainted = source_tainted_locals(&[], &FxHashSet::default(), &empty_receivers());
        assert_eq!(
            sink_source(
                &sink_with_idents(&["id"]),
                &tainted,
                &FxHashSet::default(),
                &empty_receivers()
            ),
            None
        );
    }

    #[test]
    fn sink_is_source_backed_when_arg_source_path_matches_catalogue() {
        let tainted = source_tainted_locals(&[], &FxHashSet::default(), &empty_receivers());
        assert_eq!(
            sink_source(
                &sink_with_idents_and_sources(&["process"], &["process.env.SECRET", "process.env"]),
                &tainted,
                &FxHashSet::default(),
                &empty_receivers(),
            )
            .map(|(_, title, _)| title),
            Some("Environment secret")
        );
    }

    #[test]
    fn direct_source_path_precedes_broader_tainted_local_source() {
        let bindings = vec![binding("req", "framework.request")];
        let mut deps = FxHashSet::default();
        deps.insert("express".to_string());
        let tainted = source_tainted_locals(&bindings, &deps, &empty_receivers());

        assert_eq!(
            sink_source(
                &sink_with_idents_and_sources(&["req"], &["req.body"]),
                &tainted,
                &deps,
                &empty_receivers(),
            )
            .map(|(_, title, _)| title),
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
