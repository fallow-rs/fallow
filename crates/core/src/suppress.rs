use std::sync::atomic::{AtomicBool, Ordering};

use fallow_config::{ResolvedConfig, RulesConfig, Severity};
use rustc_hash::{FxHashMap, FxHashSet};

pub use fallow_types::suppress::{
    IssueKind, PolicyRuleSuppression, Suppression, UnknownSuppressionKind, is_file_suppressed,
    is_suppressed, issue_kind_to_kebab,
};

pub use fallow_extract::suppress::parse_suppressions_from_source;

use crate::discover::FileId;
use crate::extract::ModuleInfo;
use crate::graph::ModuleGraph;
use crate::results::{ActiveSuppression, StaleSuppression, SuppressionOrigin};

/// Issue kinds whose suppression is not checked via `SuppressionContext`
/// in `find_dead_code_full`. Excludes CLI-side kinds (checked in health/flags
/// commands) and dependency-level kinds (not file-scoped, suppression never
/// consumed by core detectors). Without this exclusion, these suppressions
/// would always appear stale since no core detector checks them.
const NON_CORE_KINDS: &[IssueKind] = &[
    IssueKind::Complexity,
    IssueKind::CoverageGaps,
    IssueKind::FeatureFlag,
    IssueKind::CodeDuplication,
    IssueKind::CssTokenDrift,
    IssueKind::CssDuplicateBlock,
    IssueKind::CssSelectorComplexity,
    IssueKind::CssDeadSurface,
    IssueKind::CssBrokenReference,
    IssueKind::UnusedDependency,
    IssueKind::UnusedDevDependency,
    IssueKind::UnlistedDependency,
    IssueKind::TypeOnlyDependency,
    IssueKind::TestOnlyDependency,
    IssueKind::DevDependencyInProduction,
    IssueKind::PnpmCatalogEntry,
    IssueKind::EmptyCatalogGroup,
    IssueKind::UnresolvedCatalogReference,
    IssueKind::UnusedDependencyOverride,
    IssueKind::MisconfiguredDependencyOverride,
    IssueKind::StaleSuppression,
];

/// Suppression context that tracks which suppressions are consumed by detectors.
///
/// Wraps the per-file suppression map and records, via `AtomicBool` flags,
/// which suppression entries actually matched an issue during detection.
/// After all detectors run, `find_stale()` returns unmatched suppressions.
///
/// Uses `AtomicBool` (not `Cell<bool>`) so the context can be shared
/// across threads if detectors ever use `rayon` internally.
pub struct SuppressionContext<'a> {
    by_file: FxHashMap<FileId, &'a [Suppression]>,
    used: FxHashMap<FileId, Vec<AtomicBool>>,
    /// Suppression tokens that did not parse to any known `IssueKind`.
    /// Emitted as `StaleSuppression` with `kind_known: false` in `find_stale`.
    /// See issue #449.
    unknown_kinds: FxHashMap<FileId, &'a [UnknownSuppressionKind]>,
}

impl<'a> SuppressionContext<'a> {
    /// Build a suppression context from parsed modules.
    pub(crate) fn new(modules: &'a [ModuleInfo]) -> Self {
        let by_file: FxHashMap<FileId, &[Suppression]> = modules
            .iter()
            .filter(|m| !m.suppressions.is_empty())
            .map(|m| (m.file_id, m.suppressions.as_slice()))
            .collect();

        let used = by_file
            .iter()
            .map(|(&fid, supps)| {
                (
                    fid,
                    std::iter::repeat_with(|| AtomicBool::new(false))
                        .take(supps.len())
                        .collect(),
                )
            })
            .collect();

        let unknown_kinds: FxHashMap<FileId, &[UnknownSuppressionKind]> = modules
            .iter()
            .filter(|m| !m.unknown_suppression_kinds.is_empty())
            .map(|m| (m.file_id, m.unknown_suppression_kinds.as_slice()))
            .collect();

        Self {
            by_file,
            used,
            unknown_kinds,
        }
    }

    /// Build a suppression context from a pre-built map (for testing).
    #[cfg(test)]
    pub(crate) fn from_map(by_file: FxHashMap<FileId, &'a [Suppression]>) -> Self {
        let used = by_file
            .iter()
            .map(|(&fid, supps)| {
                (
                    fid,
                    std::iter::repeat_with(|| AtomicBool::new(false))
                        .take(supps.len())
                        .collect(),
                )
            })
            .collect();
        Self {
            by_file,
            used,
            unknown_kinds: FxHashMap::default(),
        }
    }

    /// Build an empty suppression context (for testing).
    #[cfg(test)]
    pub(crate) fn empty() -> Self {
        Self {
            by_file: FxHashMap::default(),
            used: FxHashMap::default(),
            unknown_kinds: FxHashMap::default(),
        }
    }

    /// Check if a specific issue at a given line should be suppressed,
    /// and mark the matching suppression as consumed.
    #[must_use]
    pub(crate) fn is_suppressed(&self, file_id: FileId, line: u32, kind: IssueKind) -> bool {
        let Some(supps) = self.by_file.get(&file_id) else {
            return false;
        };
        let Some(used) = self.used.get(&file_id) else {
            return false;
        };
        for (i, s) in supps.iter().enumerate() {
            if s.matches_issue_kind(line, kind) {
                used[i].store(true, Ordering::Relaxed);
                return true;
            }
        }
        false
    }

    /// Check if the entire file is suppressed for the given kind,
    /// and mark the matching suppression as consumed.
    #[must_use]
    pub(crate) fn is_file_suppressed(&self, file_id: FileId, kind: IssueKind) -> bool {
        let Some(supps) = self.by_file.get(&file_id) else {
            return false;
        };
        let Some(used) = self.used.get(&file_id) else {
            return false;
        };
        for (i, s) in supps.iter().enumerate() {
            if s.line == 0 && s.matches_issue_kind(0, kind) {
                used[i].store(true, Ordering::Relaxed);
                return true;
            }
        }
        false
    }

    /// Check if a policy finding at a given line should be suppressed.
    #[must_use]
    pub(crate) fn is_policy_suppressed(
        &self,
        file_id: FileId,
        line: u32,
        pack: &str,
        rule_id: &str,
    ) -> bool {
        let Some(supps) = self.by_file.get(&file_id) else {
            return false;
        };
        let Some(used) = self.used.get(&file_id) else {
            return false;
        };
        for (i, s) in supps.iter().enumerate() {
            if s.matches_policy_rule(line, pack, rule_id) {
                used[i].store(true, Ordering::Relaxed);
                return true;
            }
        }
        false
    }

    /// Get the raw suppressions for a file (for detectors that need direct access).
    pub fn get(&self, file_id: FileId) -> Option<&[Suppression]> {
        self.by_file.get(&file_id).copied()
    }

    /// Count suppression entries that matched at least one issue.
    #[must_use]
    pub(crate) fn used_count(&self) -> usize {
        self.used
            .values()
            .flat_map(|used| used.iter())
            .filter(|used| used.load(Ordering::Relaxed))
            .count()
    }

    /// Collect all suppressions that were never consumed by any detector.
    ///
    /// Skips suppression kinds that are checked in the CLI layer
    /// (complexity, coverage gaps, feature flags, code duplication)
    /// to avoid false positives. Also skips suppressions whose target kind
    /// is disabled (`Severity::Off`) under the resolved rules for the
    /// suppression's file, including per-file `overrides.rules`: the
    /// detector never ran, so the suppression appears unconsumed, but is
    /// not actually stale (it documents intentional dormancy and becomes
    /// valid again the moment the rule is re-enabled). See issue #482.
    pub(crate) fn find_stale(
        &self,
        graph: &ModuleGraph,
        config: &ResolvedConfig,
    ) -> Vec<StaleSuppression> {
        let mut stale = Vec::new();
        let mut warned_unknown_policy_targets: FxHashSet<(String, String)> = FxHashSet::default();

        for (&file_id, supps) in &self.by_file {
            let used = &self.used[&file_id];
            let path = &graph.modules[file_id.0 as usize].path;
            let file_rules = config.resolve_rules_for_path(path);

            for (i, s) in supps.iter().enumerate() {
                if used[i].load(Ordering::Relaxed) {
                    continue;
                }
                if let Some(entry) = stale_entry_for_suppression(
                    config,
                    &file_rules,
                    path,
                    s,
                    &mut warned_unknown_policy_targets,
                ) {
                    stale.push(entry);
                }
            }
        }

        for (&file_id, unknowns) in &self.unknown_kinds {
            let path = &graph.modules[file_id.0 as usize].path;
            for u in *unknowns {
                stale.push(unknown_kind_stale_entry(path, u, false));
            }
        }

        stale
    }

    /// Collect suppression comments that are missing `-- <reason>`.
    #[must_use]
    pub(crate) fn find_missing_reasons(&self, graph: &ModuleGraph) -> Vec<StaleSuppression> {
        let mut findings = Vec::new();
        let mut seen: FxHashSet<(FileId, u32)> = FxHashSet::default();

        for (&file_id, supps) in &self.by_file {
            let path = &graph.modules[file_id.0 as usize].path;
            for s in *supps {
                if s.reason.is_some() || !seen.insert((file_id, s.comment_line)) {
                    continue;
                }

                findings.push(StaleSuppression {
                    path: path.clone(),
                    line: s.comment_line,
                    col: 0,
                    origin: SuppressionOrigin::Comment {
                        issue_kind: s.target_token(),
                        reason: None,
                        is_file_level: s.line == 0,
                        kind_known: true,
                    },
                    missing_reason: true,
                    actions: StaleSuppression::actions_for(true),
                    effective_severity: None,
                });
            }
        }

        for (&file_id, unknowns) in &self.unknown_kinds {
            let path = &graph.modules[file_id.0 as usize].path;
            for u in *unknowns {
                if u.reason.is_some() || !seen.insert((file_id, u.comment_line)) {
                    continue;
                }

                findings.push(StaleSuppression {
                    path: path.clone(),
                    line: u.comment_line,
                    col: 0,
                    origin: SuppressionOrigin::Comment {
                        issue_kind: Some(u.token.clone()),
                        reason: None,
                        is_file_level: u.is_file_level,
                        kind_known: false,
                    },
                    missing_reason: true,
                    actions: StaleSuppression::actions_for(true),
                    effective_severity: None,
                });
            }
        }

        findings
    }

    /// Collect every suppression comment present in the analyzed files this run,
    /// keyed by file path and kind.
    ///
    /// This is the "active-suppression state" the Fallow Impact value report
    /// needs (issue: v1.5 attribution): to tell a genuinely resolved finding
    /// (code removed) from one merely silenced by a newly-added `fallow-ignore`,
    /// impact records which suppressions are in play each run and looks for ones
    /// that newly appeared covering a disappeared finding's kind.
    ///
    /// Unlike [`Self::find_stale`], this returns ALL present suppressions
    /// regardless of whether a core detector consumed them, and across every
    /// kind (dead-code, complexity, code-duplication, ...). Impact only needs to
    /// know a suppression for `(file, kind)` exists; a present-but-stale entry is
    /// harmless because impact's discriminator keys on a suppression that newly
    /// appeared between two recorded runs, and a finding silenced by a present
    /// suppression was never reported (so it never enters the resolved tally).
    /// Complexity and code-duplication suppressions are consumed in the CLI
    /// layer rather than through this context, so capturing presence here is the
    /// single uniform mechanism that covers all three impact categories.
    #[must_use]
    pub(crate) fn all_suppressions(&self, graph: &ModuleGraph) -> Vec<ActiveSuppression> {
        let mut active = Vec::new();
        for (&file_id, supps) in &self.by_file {
            let path = &graph.modules[file_id.0 as usize].path;
            for s in *supps {
                active.push(ActiveSuppression {
                    path: path.clone(),
                    kind: s.target_token(),
                    is_file_level: s.line == 0,
                    reason: s.reason.clone(),
                    comment_line: s.comment_line,
                });
            }
        }
        active
    }
}

/// Build the [`StaleSuppression`] entry for an unconsumed suppression, or `None`
/// if the suppression is exempt (non-core kind, disabled rule, disabled policy).
///
/// Records unknown policy-rule targets in `warned` and emits a one-time warning.
fn stale_entry_for_suppression(
    config: &ResolvedConfig,
    file_rules: &RulesConfig,
    path: &std::path::Path,
    s: &Suppression,
    warned: &mut FxHashSet<(String, String)>,
) -> Option<StaleSuppression> {
    if let Some(kind) = s.issue_kind_target()
        && NON_CORE_KINDS.contains(&kind)
    {
        return None;
    }

    if let Some(kind) = s.issue_kind_target()
        && file_rules.severity_for_kind(kind) == Severity::Off
    {
        return None;
    }

    if let Some(target) = s.policy_rule_target() {
        if file_rules.policy_violation == Severity::Off || policy_rule_is_disabled(config, target) {
            return None;
        }

        if !policy_rule_exists(config, target) {
            let token = target.token();
            let key = (path.to_string_lossy().to_string(), token.clone());
            if warned.insert(key) {
                tracing::warn!(
                    "{}:{}: suppression '{}' names no loaded rule-pack rule",
                    path.display(),
                    s.comment_line,
                    token
                );
            }
        }
    }

    Some(StaleSuppression {
        path: path.to_path_buf(),
        line: s.comment_line,
        col: 0,
        origin: SuppressionOrigin::Comment {
            issue_kind: s.target_token(),
            reason: s.reason.clone(),
            is_file_level: s.line == 0,
            kind_known: true,
        },
        missing_reason: false,
        actions: StaleSuppression::actions_for(false),
        effective_severity: None,
    })
}

/// Build a [`StaleSuppression`] entry for a suppression token that parsed to no
/// known [`IssueKind`]. `missing_reason` controls the corresponding flags.
fn unknown_kind_stale_entry(
    path: &std::path::Path,
    u: &UnknownSuppressionKind,
    missing_reason: bool,
) -> StaleSuppression {
    StaleSuppression {
        path: path.to_path_buf(),
        line: u.comment_line,
        col: 0,
        origin: SuppressionOrigin::Comment {
            issue_kind: Some(u.token.clone()),
            reason: if missing_reason {
                None
            } else {
                u.reason.clone()
            },
            is_file_level: u.is_file_level,
            kind_known: false,
        },
        missing_reason,
        actions: StaleSuppression::actions_for(missing_reason),
        effective_severity: None,
    }
}

fn policy_rule_exists(config: &ResolvedConfig, target: &PolicyRuleSuppression) -> bool {
    config.rule_packs.iter().any(|pack| {
        pack.name == target.pack && pack.rules.iter().any(|rule| rule.id == target.rule_id)
    })
}

fn policy_rule_is_disabled(config: &ResolvedConfig, target: &PolicyRuleSuppression) -> bool {
    config.rule_packs.iter().any(|pack| {
        pack.name == target.pack
            && pack
                .rules
                .iter()
                .any(|rule| rule.id == target.rule_id && rule.severity == Some(Severity::Off))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Run one check against a context that holds `supps` for one file, and
    /// return whether it matched plus how many entries it marked as consumed.
    fn run_check(supps: &[Suppression], line: Option<u32>, kind: IssueKind) -> (bool, usize) {
        let file = FileId(0);
        let mut by_file = FxHashMap::default();
        by_file.insert(file, supps);
        let ctx = SuppressionContext::from_map(by_file);
        let matched = match line {
            Some(line) => ctx.is_suppressed(file, line, kind),
            None => ctx.is_file_suppressed(file, kind),
        };
        (matched, ctx.used_count())
    }

    /// A line check matches on line and kind, and marks only a matching entry
    /// as consumed.
    #[test]
    fn context_line_check_matches_line_and_kind() {
        let export = IssueKind::UnusedExport;
        let cases = [
            (
                "blanket file-wide, any line",
                vec![Suppression::all(0, 1)],
                10,
                IssueKind::UnusedFile,
                true,
            ),
            (
                "file-wide kind, same kind",
                vec![Suppression::issue(0, 1, export)],
                5,
                export,
                true,
            ),
            (
                "file-wide kind, other kind",
                vec![Suppression::issue(0, 1, export)],
                5,
                IssueKind::UnusedType,
                false,
            ),
            (
                "blanket next-line, target line",
                vec![Suppression::all(5, 4)],
                5,
                export,
                true,
            ),
            (
                "blanket next-line, other line",
                vec![Suppression::all(5, 4)],
                6,
                export,
                false,
            ),
            (
                "kind next-line, other kind",
                vec![Suppression::issue(5, 4, export)],
                5,
                IssueKind::UnusedType,
                false,
            ),
            (
                "kind next-line, other line",
                vec![Suppression::issue(5, 4, export)],
                6,
                export,
                false,
            ),
            (
                "two kinds on one line, second kind",
                vec![
                    Suppression::issue(5, 4, export),
                    Suppression::issue(5, 4, IssueKind::UnusedType),
                ],
                5,
                IssueKind::UnusedType,
                true,
            ),
            (
                "two kinds on one line, third kind",
                vec![
                    Suppression::issue(5, 4, export),
                    Suppression::issue(5, 4, IssueKind::UnusedType),
                ],
                5,
                IssueKind::UnusedFile,
                false,
            ),
            (
                "scoped policy rule, generic policy kind",
                vec![Suppression::policy_rule(5, 4, "team-policy", "no-fs")],
                5,
                IssueKind::PolicyViolation,
                false,
            ),
        ];
        for (name, supps, line, kind, expected) in cases {
            let (matched, consumed) = run_check(&supps, Some(line), kind);
            assert_eq!(matched, expected, "{name}");
            assert_eq!(consumed, usize::from(expected), "{name}: consumed count");
        }
    }

    /// A file check matches only file-wide entries.
    #[test]
    fn context_file_check_matches_file_wide_entries_only() {
        let cases = [
            (
                "blanket file-wide",
                vec![Suppression::all(0, 1)],
                IssueKind::CodeDuplication,
                true,
            ),
            (
                "file-wide kind, other kind",
                vec![Suppression::issue(0, 1, IssueKind::UnusedFile)],
                IssueKind::UnusedExport,
                false,
            ),
            (
                "blanket next-line is not file-wide",
                vec![Suppression::all(5, 4)],
                IssueKind::UnusedFile,
                false,
            ),
        ];
        for (name, supps, kind, expected) in cases {
            let (matched, consumed) = run_check(&supps, None, kind);
            assert_eq!(matched, expected, "{name}");
            assert_eq!(consumed, usize::from(expected), "{name}: consumed count");
        }
    }

    /// Every `IssueKind` must be in exactly one of two sets. `core_kinds` holds
    /// the kinds whose suppressions a core detector consumes through
    /// `SuppressionContext`. `NON_CORE_KINDS` holds the kinds that are checked
    /// outside `find_dead_code_full` or that have no file-scoped suppression.
    /// An unclassified kind makes `find_stale` report every suppression of
    /// that kind as stale.
    #[test]
    fn all_issue_kinds_classified_for_stale_detection() {
        let core_kinds = [
            IssueKind::UnusedFile,
            IssueKind::UnusedExport,
            IssueKind::UnusedType,
            IssueKind::PrivateTypeLeak,
            IssueKind::DeprecatedExportInUse,
            IssueKind::UnusedEnumMember,
            IssueKind::UnusedClassMember,
            IssueKind::UnusedStoreMember,
            IssueKind::UnprovidedInject,
            IssueKind::UnresolvedImport,
            IssueKind::DuplicateExport,
            IssueKind::CircularDependency,
            IssueKind::ReExportCycle,
            IssueKind::BoundaryViolation,
            IssueKind::SecurityClientServerLeak,
            IssueKind::SecuritySink,
            IssueKind::PolicyViolation,
            IssueKind::InvalidClientExport,
            IssueKind::MixedClientServerBarrel,
            IssueKind::MisplacedDirective,
            IssueKind::RouteCollision,
            IssueKind::DynamicSegmentNameConflict,
            IssueKind::UnrenderedComponent,
            IssueKind::UnusedComponentProp,
            IssueKind::UnusedComponentEmit,
            IssueKind::UnusedComponentInput,
            IssueKind::UnusedComponentOutput,
            IssueKind::UnusedSvelteEvent,
            IssueKind::UnusedServerAction,
            IssueKind::UnusedLoadDataKey,
            IssueKind::PropDrilling,
            IssueKind::ThinWrapper,
            IssueKind::DuplicatePropShape,
        ];

        for &kind in IssueKind::ALL {
            let in_core = core_kinds.contains(&kind);
            let in_non_core = NON_CORE_KINDS.contains(&kind);
            assert!(
                in_core != in_non_core,
                "IssueKind::{kind:?} must be in exactly one of core_kinds and NON_CORE_KINDS \
                 (core: {in_core}, non-core: {in_non_core}). Use NON_CORE_KINDS when no core \
                 detector consumes its suppressions through SuppressionContext."
            );
        }
    }
}
