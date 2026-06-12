use rustc_hash::FxHashMap;

use fallow_config::{ResolvedConfig, RulePackRule, RulePackRuleKind, Severity};
use fallow_types::extract::ModuleInfo;
use fallow_types::results::{PolicyRuleKind, PolicyViolation, PolicyViolationSeverity};

use crate::discover::FileId;
use crate::graph::ModuleGraph;
use crate::suppress::SuppressionContext;

use super::boundary_calls::canonical_callee_path;
use super::security::CalleePattern;
use super::{LineOffsetsMap, byte_offset_to_line_col};

/// One rule-pack rule with its matchers compiled for evaluation.
struct CompiledRule<'a> {
    pack: &'a str,
    rule: &'a RulePackRule,
    /// Parsed callee patterns (`banned-call` rules only).
    callee_patterns: Vec<CalleePattern>,
    files: Vec<globset::GlobMatcher>,
    exclude: Vec<globset::GlobMatcher>,
}

impl CompiledRule<'_> {
    /// Whether this rule applies to a project-root-relative file path.
    fn applies_to(&self, relative: &str) -> bool {
        (self.files.is_empty() || self.files.iter().any(|m| m.is_match(relative)))
            && !self.exclude.iter().any(|m| m.is_match(relative))
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
}

/// Detect banned calls and banned imports declared by the configured rule
/// packs (`rulePacks`), reporting one `policy-violation` finding per match.
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
/// callee path per file (the `callee_uses` capture dedups per path, first
/// occurrence wins). Banned-import matching is segment-aware on the RAW
/// specifier over imports and re-exports; `require()` calls and dynamic
/// `import()` are documented false negatives in v1.
///
/// Multi-rule matching is intentionally asymmetric: for `banned-call` the
/// first applicable rule in config order wins per callee (mirroring the
/// boundary forbidden-call behavior), while every `banned-import` rule that
/// matches a specifier emits its own finding, because each rule carries its
/// own message and severity.
pub fn find_policy_violations(
    graph: &ModuleGraph,
    modules: &[ModuleInfo],
    config: &ResolvedConfig,
    suppressions: &SuppressionContext<'_>,
    line_offsets_by_file: &LineOffsetsMap<'_>,
) -> Vec<PolicyViolation> {
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

    let mut violations = Vec::new();
    for node in &graph.modules {
        if !node.is_reachable() && !node.is_entry_point() {
            continue;
        }
        let Ok(relative) = node.path.strip_prefix(&config.root) else {
            continue;
        };
        let relative = relative.to_string_lossy().replace('\\', "/");

        let master = config.resolve_rules_for_path(&node.path).policy_violation;
        if master == Severity::Off {
            continue;
        }

        let in_scope: Vec<(usize, &CompiledRule<'_>)> = rules
            .iter()
            .enumerate()
            .filter(|(_, rule)| rule.applies_to(&relative))
            .collect();
        if in_scope.is_empty() {
            continue;
        }
        for (index, _) in &in_scope {
            scoped_file_counts[*index] += 1;
        }

        let Some(module) = modules_by_id.get(&node.file_id) else {
            continue;
        };

        collect_banned_imports(
            &in_scope,
            module,
            node,
            master,
            suppressions,
            line_offsets_by_file,
            &mut violations,
        );
        collect_banned_calls(
            &in_scope,
            module,
            node,
            master,
            suppressions,
            line_offsets_by_file,
            &mut violations,
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
            let compile = |patterns: &[String]| {
                patterns
                    .iter()
                    .filter_map(|pattern| globset::Glob::new(pattern).ok())
                    .map(|glob| glob.compile_matcher())
                    .collect::<Vec<_>>()
            };
            rules.push(CompiledRule {
                pack: pack.name.as_str(),
                rule,
                callee_patterns,
                files: compile(&rule.files),
                exclude: compile(&rule.exclude),
            });
        }
    }
    rules
}

/// Emit one finding per `banned-import` rule match over the module's imports
/// and re-exports.
fn collect_banned_imports(
    in_scope: &[(usize, &CompiledRule<'_>)],
    module: &ModuleInfo,
    node: &crate::graph::ModuleNode,
    master: Severity,
    suppressions: &SuppressionContext<'_>,
    line_offsets_by_file: &LineOffsetsMap<'_>,
    violations: &mut Vec<PolicyViolation>,
) {
    for (_, rule) in in_scope {
        if rule.rule.kind != RulePackRuleKind::BannedImport {
            continue;
        }
        let Some(severity) = wire_severity(rule.effective_severity(master)) else {
            continue;
        };
        let sites = module
            .imports
            .iter()
            .map(|import| {
                (
                    import.source.as_str(),
                    import.is_type_only,
                    import.span.start,
                )
            })
            .chain(module.re_exports.iter().map(|re_export| {
                (
                    re_export.source.as_str(),
                    re_export.is_type_only,
                    re_export.span.start,
                )
            }));
        for (source, is_type_only, span_start) in sites {
            if rule.rule.ignore_type_only && is_type_only {
                continue;
            }
            if !rule
                .rule
                .specifiers
                .iter()
                .any(|specifier| specifier_matches(source, specifier))
            {
                continue;
            }
            let (line, col) =
                byte_offset_to_line_col(line_offsets_by_file, node.file_id, span_start);
            if suppressions.is_policy_suppressed(node.file_id, line, rule.pack, &rule.rule.id) {
                continue;
            }
            violations.push(PolicyViolation {
                path: node.path.clone(),
                line,
                col,
                pack: rule.pack.to_owned(),
                rule_id: rule.rule.id.clone(),
                kind: PolicyRuleKind::BannedImport,
                matched: source.to_owned(),
                severity,
                message: rule.rule.message.clone(),
            });
        }
    }
}

/// Emit one finding per unique callee path matched by the first applicable
/// `banned-call` rule (config order), mirroring the boundary forbidden-call
/// first-pattern-wins behavior.
fn collect_banned_calls(
    in_scope: &[(usize, &CompiledRule<'_>)],
    module: &ModuleInfo,
    node: &crate::graph::ModuleNode,
    master: Severity,
    suppressions: &SuppressionContext<'_>,
    line_offsets_by_file: &LineOffsetsMap<'_>,
    violations: &mut Vec<PolicyViolation>,
) {
    for callee_use in &module.callee_uses {
        let matched = in_scope.iter().find_map(|(_, rule)| {
            if rule.rule.kind != RulePackRuleKind::BannedCall {
                return None;
            }
            let severity = wire_severity(rule.effective_severity(master))?;
            rule.matches_callee(module, &callee_use.callee_path)
                .then_some((rule, severity))
        });
        let Some((rule, severity)) = matched else {
            continue;
        };
        let (line, col) =
            byte_offset_to_line_col(line_offsets_by_file, node.file_id, callee_use.span_start);
        if suppressions.is_policy_suppressed(node.file_id, line, rule.pack, &rule.rule.id) {
            continue;
        }
        violations.push(PolicyViolation {
            path: node.path.clone(),
            line,
            col,
            pack: rule.pack.to_owned(),
            rule_id: rule.rule.id.clone(),
            kind: PolicyRuleKind::BannedCall,
            matched: callee_use.callee_path.clone(),
            severity,
            message: rule.rule.message.clone(),
        });
    }
}

/// Segment-aware raw-specifier match: the pattern matches exactly or at a
/// `/` boundary, so `moment` covers `moment/locale/nl` but never
/// `moment-timezone`.
fn specifier_matches(raw: &str, pattern: &str) -> bool {
    raw == pattern
        || raw
            .strip_prefix(pattern)
            .is_some_and(|rest| rest.starts_with('/'))
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
