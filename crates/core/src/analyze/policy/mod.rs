mod gdp;

use rustc_hash::FxHashMap;
use rustc_hash::FxHashSet;

use fallow_config::{EffectKind, ResolvedConfig, RulePackRule, RulePackRuleKind, Severity};
use fallow_types::extract::{ImportedName, ModuleInfo};
use fallow_types::output::IssueAction;
use fallow_types::output_dead_code::PolicyViolationFinding;
use fallow_types::results::{PolicyRuleKind, PolicyViolation, PolicyViolationSeverity};

use crate::discover::FileId;
use crate::graph::ModuleGraph;
use crate::resolve::ResolvedModule;
use crate::suppress::SuppressionContext;

use super::boundary_calls::{canonical_callee_path, display_callee};
use super::security::{CalleePattern, catalogue_matchers};
use super::{LineOffsetsMap, byte_offset_to_line_col};

/// One rule-pack rule with its matchers compiled for evaluation.
struct CompiledRule<'a> {
    pack: &'a str,
    rule: &'a RulePackRule,
    /// Parsed callee patterns (`banned-call` rules only).
    callee_patterns: Vec<CalleePattern>,
    effects: FxHashSet<EffectKind>,
    zones: FxHashSet<String>,
    files: Vec<globset::GlobMatcher>,
    exclude: Vec<globset::GlobMatcher>,
    allowed_files: Vec<globset::GlobMatcher>,
}

impl CompiledRule<'_> {
    /// Whether this rule applies to a project-root-relative file path.
    fn applies_to(&self, relative: &str, zone: Option<&str>) -> bool {
        compiled_scope_applies(&self.files, &self.exclude, &self.zones, relative, zone)
    }

    /// Per-rule severity overriding the file's effective master severity.
    fn effective_severity(&self, master: Severity) -> Severity {
        self.rule.severity.unwrap_or(master)
    }

    /// Whether any callee pattern matches the written callee path or its
    /// import-resolved canonical form (same two-pass matching as
    /// `boundaries.calls.forbidden`).
    fn matches_callee(&self, module: &ModuleInfo, callee_path: &str) -> bool {
        if self
            .callee_patterns
            .iter()
            .any(|pattern| pattern.matches(callee_path))
        {
            return true;
        }
        canonical_callee_path(module, callee_path).is_some_and(|canonical| {
            self.callee_patterns
                .iter()
                .any(|pattern| pattern.matches(&canonical))
        })
    }

    fn matches_effect(
        &self,
        module: &ModuleInfo,
        callee_path: &str,
        declared_deps: &FxHashSet<String>,
    ) -> Option<EffectKind> {
        effect_for_callee(module, callee_path, declared_deps)
            .filter(|effect| self.effects.contains(effect))
    }
}

fn compile_scope_globs(patterns: &[String]) -> Vec<globset::GlobMatcher> {
    // Rule-pack loading validates these globs. An unexpected parse failure
    // drops only that pattern, like the callee patterns in `compile_rules`.
    patterns
        .iter()
        .filter_map(|pattern| fallow_config::compile_path_glob(pattern).ok())
        .map(|glob| glob.compile_matcher())
        .collect()
}

fn compiled_scope_applies(
    files: &[globset::GlobMatcher],
    exclude: &[globset::GlobMatcher],
    zones: &FxHashSet<String>,
    relative: &str,
    zone: Option<&str>,
) -> bool {
    (files.is_empty() || files.iter().any(|m| m.is_match(relative)))
        && !exclude.iter().any(|m| m.is_match(relative))
        && (zones.is_empty() || zone.is_some_and(|zone| zones.contains(zone)))
}

/// Shared graph, extraction, configuration, and source context for policy analysis.
#[derive(Clone, Copy)]
pub(super) struct PolicyAnalysisInput<'a> {
    pub(super) graph: &'a ModuleGraph,
    pub(super) modules: &'a [ModuleInfo],
    pub(super) resolved_modules: &'a [ResolvedModule],
    pub(super) config: &'a ResolvedConfig,
    pub(super) declared_deps: &'a FxHashSet<String>,
    pub(super) suppressions: &'a SuppressionContext<'a>,
    pub(super) line_offsets_by_file: &'a LineOffsetsMap<'a>,
}

/// Detect banned calls, imports, catalogue-derived effects, and proof producer
/// ownership declared by the configured rule packs (`rulePacks`), reporting one
/// `policy-violation` finding per match.
///
/// Severity model: each file's master severity is
/// `resolve_rules_for_path(...).policy_violation` (so per-file `overrides`
/// apply); a rule-level `severity` overrides that master per finding. Master
/// `off` is a kill switch for the file (per-rule severity cannot resurrect
/// it); rule-level `off` disables only that rule. Emitted findings therefore
/// carry only `error` or `warn`.
///
/// Banned-call matching mirrors `boundaries.calls.forbidden`: the written
/// callee path and an import-resolved canonical path are both tried, so
/// `child_process.*` covers named, namespace, and default imports from
/// `child_process` / `node:child_process`. One finding is reported per unique
/// callee path per form per file (the `callee_uses` capture dedups per path
/// and per call or `new` form, first occurrence wins). Banned-effect matching
/// reads calls only. Banned-import matching is segment-aware on the RAW
/// specifier over imports and re-exports; `require()` calls and dynamic
/// `import()` are documented false negatives in v1. A rule with `allowNames`
/// reports each site that does not bind a listed name, with the binding in
/// `matched`.
///
/// Multi-rule matching is intentionally asymmetric: for `banned-call` the
/// first applicable rule in config order wins per callee (mirroring the
/// boundary forbidden-call behavior). `banned-effect` follows the same first
/// applicable rule policy over catalogue-derived effects. Every
/// `banned-import` rule that matches a specifier emits its own finding, because
/// each rule carries its own message and severity. Proof-producer rules likewise
/// report independently at every admitted factory call, including unreachable
/// analyzed modules, and follow only unambiguous ESM symbol provenance.
pub(super) fn find_policy_violations(input: PolicyAnalysisInput<'_>) -> Vec<PolicyViolation> {
    let PolicyAnalysisInput {
        graph,
        modules,
        resolved_modules,
        config,
        declared_deps,
        suppressions,
        line_offsets_by_file,
    } = input;
    if config.rule_packs.is_empty() {
        return Vec::new();
    }

    let rules = compile_rules(config);
    if rules.is_empty() {
        return Vec::new();
    }

    let modules_by_id: FxHashMap<FileId, &ModuleInfo> =
        modules.iter().map(|m| (m.file_id, m)).collect();

    // Track how many files each `files`-scoped rule applied to so a rule
    // whose globs match nothing warns instead of silently reporting zero
    // findings forever (mirrors the boundary zero-zone warn).
    let mut scoped_file_counts: Vec<usize> = vec![0; rules.len()];
    let mut zones_by_file: FxHashMap<FileId, Option<&str>> = FxHashMap::default();

    let producer_rules_enabled = rules
        .iter()
        .any(|rule| rule.rule.kind == RulePackRuleKind::GdpProofProducer);
    let mut origins = gdp::ProofOrigins::new(if producer_rules_enabled {
        resolved_modules
    } else {
        &[]
    });
    let mut violations = Vec::new();
    for node in &graph.modules {
        let zone = *zones_by_file.entry(node.file_id).or_insert_with(|| {
            node.path.strip_prefix(&config.root).ok().and_then(|path| {
                let relative = path.to_string_lossy().replace('\\', "/");
                config.boundaries.classify_zone(&relative)
            })
        });
        collect_node_policy_violations(
            &mut PolicyNodeInput {
                node,
                config,
                rules: &rules,
                zone,
                modules_by_id: &modules_by_id,
                declared_deps,
                suppressions,
                line_offsets_by_file,
                scoped_file_counts: &mut scoped_file_counts,
                violations: &mut violations,
            },
            &mut origins,
        );
    }

    for (index, rule) in rules.iter().enumerate() {
        if !rule.rule.files.is_empty() && scoped_file_counts[index] == 0 {
            tracing::warn!(
                "rule pack '{}': rule '{}' has `files` globs that matched no analyzed file; the \
                 rule currently enforces nothing",
                rule.pack,
                rule.rule.id
            );
        }
    }

    violations
}

/// Inputs threaded into the per-node policy scan: the node, the compiled rules,
/// the module lookup, the shared analysis context, and the mutable accumulators.
struct PolicyNodeInput<'a> {
    node: &'a crate::graph::ModuleNode,
    config: &'a ResolvedConfig,
    rules: &'a [CompiledRule<'a>],
    zone: Option<&'a str>,
    modules_by_id: &'a FxHashMap<FileId, &'a ModuleInfo>,
    declared_deps: &'a FxHashSet<String>,
    suppressions: &'a SuppressionContext<'a>,
    line_offsets_by_file: &'a LineOffsetsMap<'a>,
    scoped_file_counts: &'a mut [usize],
    violations: &'a mut Vec<PolicyViolation>,
}

/// Evaluate applicable rules for one analyzed module, bumping scope counts and
/// appending findings. Only proof-producer rules apply to unreachable modules;
/// off-master and out-of-scope nodes remain skipped.
fn collect_node_policy_violations(
    input: &mut PolicyNodeInput<'_>,
    origins: &mut gdp::ProofOrigins<'_>,
) {
    let node = input.node;
    let Some(scope) = scoped_policy_rules(
        node,
        input.config,
        input.rules,
        input.zone,
        input.scoped_file_counts,
    ) else {
        return;
    };

    let Some(module) = input.modules_by_id.get(&node.file_id) else {
        return;
    };

    gdp::collect_producer_violations(
        &mut PolicyCollectionInput {
            in_scope: &scope.in_scope,
            module,
            node,
            master: scope.master,
            declared_deps: input.declared_deps,
            suppressions: input.suppressions,
            line_offsets_by_file: input.line_offsets_by_file,
            violations: input.violations,
        },
        origins,
        &input.config.root,
    );
    collect_banned_imports(&mut PolicyCollectionInput {
        in_scope: &scope.in_scope,
        module,
        node,
        master: scope.master,
        declared_deps: input.declared_deps,
        suppressions: input.suppressions,
        line_offsets_by_file: input.line_offsets_by_file,
        violations: input.violations,
    });
    collect_banned_exports(&mut PolicyCollectionInput {
        in_scope: &scope.in_scope,
        module,
        node,
        master: scope.master,
        declared_deps: input.declared_deps,
        suppressions: input.suppressions,
        line_offsets_by_file: input.line_offsets_by_file,
        violations: input.violations,
    });
    collect_banned_effects(&mut PolicyCollectionInput {
        in_scope: &scope.in_scope,
        module,
        node,
        master: scope.master,
        declared_deps: input.declared_deps,
        suppressions: input.suppressions,
        line_offsets_by_file: input.line_offsets_by_file,
        violations: input.violations,
    });
    collect_banned_calls(&mut PolicyCollectionInput {
        in_scope: &scope.in_scope,
        module,
        node,
        master: scope.master,
        declared_deps: input.declared_deps,
        suppressions: input.suppressions,
        line_offsets_by_file: input.line_offsets_by_file,
        violations: input.violations,
    });
}

struct ScopedPolicyRules<'a> {
    master: Severity,
    in_scope: Vec<(usize, &'a CompiledRule<'a>)>,
}

fn scoped_policy_rules<'a>(
    node: &crate::graph::ModuleNode,
    config: &ResolvedConfig,
    rules: &'a [CompiledRule<'a>],
    zone: Option<&str>,
    scoped_file_counts: &mut [usize],
) -> Option<ScopedPolicyRules<'a>> {
    let Ok(relative) = node.path.strip_prefix(&config.root) else {
        return None;
    };
    let relative = relative.to_string_lossy().replace('\\', "/");

    let master = config.resolve_rules_for_path(&node.path).policy_violation;
    if master == Severity::Off {
        return None;
    }

    let in_scope: Vec<(usize, &CompiledRule<'_>)> = rules
        .iter()
        .enumerate()
        .filter(|(_, rule)| {
            (node.is_reachable()
                || node.is_entry_point()
                || rule.rule.kind == RulePackRuleKind::GdpProofProducer)
                && rule.applies_to(&relative, zone)
        })
        .collect();
    if in_scope.is_empty() {
        return None;
    }
    for (index, _) in &in_scope {
        scoped_file_counts[*index] += 1;
    }

    Some(ScopedPolicyRules { master, in_scope })
}

/// Compile every loaded pack rule. Rules pinned to `severity: "off"` are
/// dropped here: they are disabled regardless of the master severity.
fn compile_rules(config: &ResolvedConfig) -> Vec<CompiledRule<'_>> {
    let mut rules = Vec::new();
    for pack in &config.rule_packs {
        for rule in &pack.rules {
            if rule.severity == Some(Severity::Off) {
                continue;
            }
            // Patterns and globs are validated at config load; a parse
            // failure here only drops that single pattern (defensive).
            let callee_patterns = rule
                .callees
                .iter()
                .filter_map(|raw| CalleePattern::parse(raw))
                .collect();
            let effects = rule.effects.iter().copied().collect();
            let zones = rule.zones.iter().cloned().collect();
            rules.push(CompiledRule {
                pack: pack.name.as_str(),
                rule,
                callee_patterns,
                effects,
                zones,
                files: compile_scope_globs(&rule.files),
                exclude: compile_scope_globs(&rule.exclude),
                allowed_files: compile_scope_globs(&rule.allowed_files),
            });
        }
    }
    rules
}

/// Emit one finding per `banned-import` rule match over the module's imports
/// and re-exports.
struct PolicyCollectionInput<'a> {
    in_scope: &'a [(usize, &'a CompiledRule<'a>)],
    module: &'a ModuleInfo,
    node: &'a crate::graph::ModuleNode,
    master: Severity,
    declared_deps: &'a FxHashSet<String>,
    suppressions: &'a SuppressionContext<'a>,
    line_offsets_by_file: &'a LineOffsetsMap<'a>,
    violations: &'a mut Vec<PolicyViolation>,
}

fn collect_banned_imports(input: &mut PolicyCollectionInput<'_>) {
    let ctx = BannedImportCtx {
        node: input.node,
        suppressions: input.suppressions,
        line_offsets_by_file: input.line_offsets_by_file,
    };
    for (_, rule) in input.in_scope {
        if rule.rule.kind != RulePackRuleKind::BannedImport {
            continue;
        }
        let Some(severity) = wire_severity(rule.effective_severity(input.master)) else {
            continue;
        };
        let sites = input
            .module
            .imports
            .iter()
            .map(|import| {
                (
                    import.source.as_str(),
                    import_binding(&import.imported_name),
                    import.is_type_only,
                    import.span.start,
                )
            })
            .chain(input.module.re_exports.iter().map(|re_export| {
                (
                    re_export.source.as_str(),
                    re_export_binding(&re_export.imported_name),
                    re_export.is_type_only,
                    re_export.span.start,
                )
            }));
        for (source, binding, is_type_only, span_start) in sites {
            push_banned_import_if_matched(
                &ctx,
                rule,
                severity,
                &BannedImportSite {
                    source,
                    binding,
                    is_type_only,
                    span_start,
                },
                input.violations,
            );
        }
    }
}

/// A single import / re-export specifier site evaluated by `banned-import`.
struct BannedImportSite<'a> {
    source: &'a str,
    binding: ImportBinding<'a>,
    is_type_only: bool,
    span_start: u32,
}

/// What one import or re-export site binds, as `allowNames` sees it.
#[derive(Clone, Copy)]
enum ImportBinding<'a> {
    /// A source-side name. A default import is the name `default`.
    Name(&'a str),
    /// A namespace import or a star re-export.
    Star,
    /// An import that binds nothing.
    SideEffect,
}

const fn import_binding(imported_name: &ImportedName) -> ImportBinding<'_> {
    match imported_name {
        ImportedName::Named(name) => ImportBinding::Name(name.as_str()),
        ImportedName::Default => ImportBinding::Name("default"),
        ImportedName::Namespace => ImportBinding::Star,
        ImportedName::SideEffect => ImportBinding::SideEffect,
    }
}

fn re_export_binding(imported_name: &str) -> ImportBinding<'_> {
    if imported_name == "*" {
        ImportBinding::Star
    } else {
        ImportBinding::Name(imported_name)
    }
}

/// The `matched` value of a site whose specifier matches `rule`, or `None` when
/// `allowNames` allows the site. Only a name can be in the list: a star or
/// side-effect form reports even when a list holds the text `*`.
fn banned_import_matched(rule: &RulePackRule, site: &BannedImportSite<'_>) -> Option<String> {
    if rule.allow_names.is_empty() {
        return Some(site.source.to_owned());
    }
    let binding = match site.binding {
        ImportBinding::Name(name) if rule.allow_names.iter().any(|allowed| allowed == name) => {
            return None;
        }
        ImportBinding::Name(name) => name,
        ImportBinding::Star => "*",
        ImportBinding::SideEffect => "side-effect",
    };
    Some(format!("{}: {binding}", site.source))
}

/// Wrap a raw violation with its actions. A finding of a rule with `allowNames`
/// gets a note that explains the list.
pub(super) fn policy_finding(
    violation: PolicyViolation,
    config: &ResolvedConfig,
) -> PolicyViolationFinding {
    let has_allow_names = config
        .rule_packs
        .iter()
        .filter(|pack| pack.name == violation.pack)
        .flat_map(|pack| &pack.rules)
        .any(|rule| rule.id == violation.rule_id && !rule.allow_names.is_empty());
    let note = has_allow_names.then(|| {
        format!(
            "Rule `{}/{}` allows only listed names from this specifier. A namespace import, a star re-export and a side-effect import always report, because fallow cannot prove that they stay in the list. Import a listed name by name.",
            violation.pack, violation.rule_id,
        )
    });
    let mut finding = PolicyViolationFinding::with_actions(violation);
    if let Some(note) = note
        && let Some(IssueAction::Fix(fix)) = finding.actions.first_mut()
    {
        fix.note = Some(note);
    }
    finding
}

/// Per-module emission context shared across every `banned-import` site check.
struct BannedImportCtx<'a> {
    node: &'a crate::graph::ModuleNode,
    suppressions: &'a SuppressionContext<'a>,
    line_offsets_by_file: &'a LineOffsetsMap<'a>,
}

/// Push a `banned-import` violation when `site`'s specifier matches the rule and
/// is neither type-only-skipped nor suppressed.
fn push_banned_import_if_matched(
    ctx: &BannedImportCtx<'_>,
    rule: &CompiledRule<'_>,
    severity: PolicyViolationSeverity,
    site: &BannedImportSite<'_>,
    violations: &mut Vec<PolicyViolation>,
) {
    if rule.rule.ignore_type_only && site.is_type_only {
        return;
    }
    if !rule
        .rule
        .specifiers
        .iter()
        .any(|specifier| specifier_matches(site.source, specifier))
    {
        return;
    }
    let Some(matched) = banned_import_matched(rule.rule, site) else {
        return;
    };
    let (line, col) =
        byte_offset_to_line_col(ctx.line_offsets_by_file, ctx.node.file_id, site.span_start);
    if ctx
        .suppressions
        .is_policy_suppressed(ctx.node.file_id, line, rule.pack, &rule.rule.id)
    {
        return;
    }
    violations.push(PolicyViolation {
        path: ctx.node.path.clone(),
        line,
        col,
        pack: rule.pack.to_owned(),
        rule_id: rule.rule.id.clone(),
        kind: PolicyRuleKind::BannedImport,
        matched,
        severity,
        message: rule.rule.message.clone(),
    });
}

/// Emit one finding per `banned-export` rule match over the module's direct
/// exports. Re-exports are intentionally handled by `banned-import`.
fn collect_banned_exports(input: &mut PolicyCollectionInput<'_>) {
    for (_, rule) in input.in_scope {
        if rule.rule.kind != RulePackRuleKind::BannedExport {
            continue;
        }
        let Some(severity) = wire_severity(rule.effective_severity(input.master)) else {
            continue;
        };
        for export in input.module.exports.iter() {
            if rule.rule.ignore_type_only && export.is_type_only {
                continue;
            }
            if !rule
                .rule
                .exports
                .iter()
                .any(|pattern| export_pattern_matches(&export.name, pattern))
            {
                continue;
            }
            let (line, col) = byte_offset_to_line_col(
                input.line_offsets_by_file,
                input.node.file_id,
                export.span.start,
            );
            if input.suppressions.is_policy_suppressed(
                input.node.file_id,
                line,
                rule.pack,
                &rule.rule.id,
            ) {
                continue;
            }
            input.violations.push(PolicyViolation {
                path: input.node.path.clone(),
                line,
                col,
                pack: rule.pack.to_owned(),
                rule_id: rule.rule.id.clone(),
                kind: PolicyRuleKind::BannedExport,
                matched: export.name.to_string(),
                severity,
                message: rule.rule.message.clone(),
            });
        }
    }
}

fn collect_banned_effects(input: &mut PolicyCollectionInput<'_>) {
    for callee_use in input.module.callee_uses.iter().filter(|use_| !use_.is_new) {
        let matched = input.in_scope.iter().find_map(|(_, rule)| {
            if rule.rule.kind != RulePackRuleKind::BannedEffect {
                return None;
            }
            let severity = wire_severity(rule.effective_severity(input.master))?;
            rule.matches_effect(input.module, &callee_use.callee_path, input.declared_deps)
                .map(|effect| (rule, severity, effect))
        });
        let Some((rule, severity, effect)) = matched else {
            continue;
        };
        let (line, col) = byte_offset_to_line_col(
            input.line_offsets_by_file,
            input.node.file_id,
            callee_use.span_start,
        );
        if input.suppressions.is_policy_suppressed(
            input.node.file_id,
            line,
            rule.pack,
            &rule.rule.id,
        ) {
            continue;
        }
        input.violations.push(PolicyViolation {
            path: input.node.path.clone(),
            line,
            col,
            pack: rule.pack.to_owned(),
            rule_id: rule.rule.id.clone(),
            kind: PolicyRuleKind::BannedEffect,
            matched: format!("{}: {}", effect.as_str(), callee_use.callee_path),
            severity,
            message: rule.rule.message.clone(),
        });
    }
}

/// Emit one finding per unique callee path per form matched by the first applicable
/// `banned-call` rule (config order), mirroring the boundary forbidden-call
/// first-pattern-wins behavior.
fn collect_banned_calls(input: &mut PolicyCollectionInput<'_>) {
    for callee_use in &input.module.callee_uses {
        let matched = input.in_scope.iter().find_map(|(_, rule)| {
            if rule.rule.kind != RulePackRuleKind::BannedCall {
                return None;
            }
            let severity = wire_severity(rule.effective_severity(input.master))?;
            rule.matches_callee(input.module, &callee_use.callee_path)
                .then_some((rule, severity))
        });
        let Some((rule, severity)) = matched else {
            continue;
        };
        let (line, col) = byte_offset_to_line_col(
            input.line_offsets_by_file,
            input.node.file_id,
            callee_use.span_start,
        );
        if input.suppressions.is_policy_suppressed(
            input.node.file_id,
            line,
            rule.pack,
            &rule.rule.id,
        ) {
            continue;
        }
        input.violations.push(PolicyViolation {
            path: input.node.path.clone(),
            line,
            col,
            pack: rule.pack.to_owned(),
            rule_id: rule.rule.id.clone(),
            kind: PolicyRuleKind::BannedCall,
            matched: display_callee(callee_use),
            severity,
            message: rule.rule.message.clone(),
        });
    }
}

fn effect_for_callee(
    module: &ModuleInfo,
    callee_path: &str,
    declared_deps: &FxHashSet<String>,
) -> Option<EffectKind> {
    let written = catalogue_matchers()
        .iter()
        .find(|matcher| matcher_matches_callee(matcher, module, callee_path, declared_deps))
        .map(|matcher| matcher.effect);
    if written.is_some() {
        return written;
    }
    let canonical = canonical_callee_path(module, callee_path)?;
    catalogue_matchers()
        .iter()
        .find(|matcher| matcher_matches_callee(matcher, module, &canonical, declared_deps))
        .map(|matcher| matcher.effect)
}

fn matcher_matches_callee(
    matcher: &super::security::Matcher,
    module: &ModuleInfo,
    callee_path: &str,
    declared_deps: &FxHashSet<String>,
) -> bool {
    matcher.enabler_satisfied(declared_deps)
        && matcher.provenance_satisfied(
            module,
            callee_path,
            fallow_security::ProvenanceBindings::ImportsAndRequires,
        )
        && matcher.first_matching_pattern(callee_path).is_some()
}

/// Segment-aware raw-specifier match: the pattern matches exactly or at a
/// `/` boundary, so `moment` covers `moment/locale/nl` but never
/// `moment-timezone`.
fn specifier_matches(raw: &str, pattern: &str) -> bool {
    if let Some(prefix) = pattern.strip_suffix("/*") {
        return raw
            .strip_prefix(prefix)
            .is_some_and(|rest| rest.starts_with('/'));
    }
    raw == pattern
        || raw
            .strip_prefix(pattern)
            .is_some_and(|rest| rest.starts_with('/'))
}

fn export_pattern_matches(name: &fallow_types::extract::ExportName, pattern: &str) -> bool {
    pattern.strip_suffix('*').map_or_else(
        || name.matches_str(pattern),
        |prefix| name.to_string().starts_with(prefix),
    )
}

/// Map an effective config severity onto the wire enum. `Off` yields `None`
/// (the rule emits nothing).
const fn wire_severity(severity: Severity) -> Option<PolicyViolationSeverity> {
    match severity {
        Severity::Error => Some(PolicyViolationSeverity::Error),
        Severity::Warn => Some(PolicyViolationSeverity::Warn),
        Severity::Off => None,
    }
}

#[cfg(test)]
mod tests;
