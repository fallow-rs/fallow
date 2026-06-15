use std::sync::atomic::{AtomicBool, Ordering};

use fallow_config::{ResolvedConfig, RulesConfig, Severity};
use rustc_hash::{FxHashMap, FxHashSet};

pub use fallow_types::suppress::{
    IssueKind, PolicyRuleSuppression, Suppression, UnknownSuppressionKind, issue_kind_to_kebab,
};

pub use fallow_extract::suppress::parse_suppressions_from_source;

use crate::discover::FileId;
use crate::extract::ModuleInfo;
use crate::graph::ModuleGraph;
use crate::results::{ActiveSuppression, StaleSuppression, SuppressionOrigin};

/// Convert an [`IssueKind`] to its canonical kebab-case wire string.
///
/// Single source of truth for the kind-to-string mapping, shared by stale
/// detection and active-suppression capture so the two never drift.
#[must_use]
pub fn kind_to_kebab(kind: IssueKind) -> &'static str {
    issue_kind_to_kebab(kind)
}

/// Map an `IssueKind` to its corresponding severity in `RulesConfig`.
///
/// Exhaustive match by design: a new `IssueKind` variant triggers a compile
/// error here, forcing the implementer to decide which `RulesConfig` field
/// (if any) gates emission. Kinds that have no matching field
/// (`CodeDuplication`, gated by the dupes command itself) return the
/// non-Off value `Severity::Error`; in practice these kinds short-circuit
/// earlier via `NON_CORE_KINDS` so the returned value is unobservable.
fn severity_for_kind(rules: &RulesConfig, kind: IssueKind) -> Severity {
    match kind {
        IssueKind::UnusedFile => rules.unused_files,
        IssueKind::UnusedExport => rules.unused_exports,
        IssueKind::UnusedType => rules.unused_types,
        IssueKind::PrivateTypeLeak => rules.private_type_leaks,
        IssueKind::UnusedDependency => rules.unused_dependencies,
        IssueKind::UnusedDevDependency => rules.unused_dev_dependencies,
        IssueKind::UnusedEnumMember => rules.unused_enum_members,
        IssueKind::UnusedClassMember => rules.unused_class_members,
        IssueKind::UnusedStoreMember => rules.unused_store_members,
        IssueKind::UnprovidedInject => rules.unprovided_injects,
        IssueKind::UnresolvedImport => rules.unresolved_imports,
        IssueKind::UnlistedDependency => rules.unlisted_dependencies,
        IssueKind::DuplicateExport => rules.duplicate_exports,
        IssueKind::CircularDependency => rules.circular_dependencies,
        IssueKind::ReExportCycle => rules.re_export_cycle,
        IssueKind::TypeOnlyDependency => rules.type_only_dependencies,
        IssueKind::TestOnlyDependency => rules.test_only_dependencies,
        IssueKind::BoundaryViolation => rules.boundary_violation,
        IssueKind::CoverageGaps => rules.coverage_gaps,
        IssueKind::FeatureFlag => rules.feature_flags,
        IssueKind::StaleSuppression => rules.stale_suppressions,
        IssueKind::PnpmCatalogEntry => rules.unused_catalog_entries,
        IssueKind::EmptyCatalogGroup => rules.empty_catalog_groups,
        IssueKind::UnresolvedCatalogReference => rules.unresolved_catalog_references,
        IssueKind::UnusedDependencyOverride => rules.unused_dependency_overrides,
        IssueKind::MisconfiguredDependencyOverride => rules.misconfigured_dependency_overrides,
        IssueKind::SecurityClientServerLeak => rules.security_client_server_leak,
        IssueKind::SecuritySink => rules.security_sink,
        IssueKind::PolicyViolation => rules.policy_violation,
        IssueKind::InvalidClientExport => rules.invalid_client_export,
        IssueKind::MixedClientServerBarrel => rules.mixed_client_server_barrel,
        IssueKind::MisplacedDirective => rules.misplaced_directive,
        IssueKind::RouteCollision => rules.route_collision,
        IssueKind::DynamicSegmentNameConflict => rules.dynamic_segment_name_conflict,
        IssueKind::UnrenderedComponent => rules.unrendered_components,
        IssueKind::UnusedComponentProp => rules.unused_component_props,
        IssueKind::UnusedComponentEmit => rules.unused_component_emits,
        IssueKind::UnusedServerAction => rules.unused_server_actions,
        IssueKind::UnusedLoadDataKey => rules.unused_load_data_keys,
        IssueKind::Complexity | IssueKind::CodeDuplication => Severity::Error,
    }
}

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
    IssueKind::UnusedDependency,
    IssueKind::UnusedDevDependency,
    IssueKind::UnlistedDependency,
    IssueKind::TypeOnlyDependency,
    IssueKind::TestOnlyDependency,
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
    pub fn new(modules: &'a [ModuleInfo]) -> Self {
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
    pub fn from_map(by_file: FxHashMap<FileId, &'a [Suppression]>) -> Self {
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
    pub fn empty() -> Self {
        Self {
            by_file: FxHashMap::default(),
            used: FxHashMap::default(),
            unknown_kinds: FxHashMap::default(),
        }
    }

    /// Check if a specific issue at a given line should be suppressed,
    /// and mark the matching suppression as consumed.
    #[must_use]
    pub fn is_suppressed(&self, file_id: FileId, line: u32, kind: IssueKind) -> bool {
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
    pub fn is_file_suppressed(&self, file_id: FileId, kind: IssueKind) -> bool {
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
    pub fn is_policy_suppressed(
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
    pub fn used_count(&self) -> usize {
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
    pub fn find_stale(
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

                if let Some(kind) = s.issue_kind_target()
                    && NON_CORE_KINDS.contains(&kind)
                {
                    continue;
                }

                if let Some(kind) = s.issue_kind_target()
                    && severity_for_kind(&file_rules, kind) == Severity::Off
                {
                    continue;
                }

                if let Some(target) = s.policy_rule_target() {
                    if file_rules.policy_violation == Severity::Off
                        || policy_rule_is_disabled(config, target)
                    {
                        continue;
                    }

                    if !policy_rule_exists(config, target) {
                        let token = target.token();
                        let key = (path.to_string_lossy().to_string(), token.clone());
                        if warned_unknown_policy_targets.insert(key) {
                            tracing::warn!(
                                "{}:{}: suppression '{}' names no loaded rule-pack rule",
                                path.display(),
                                s.comment_line,
                                token
                            );
                        }
                    }
                }

                let is_file_level = s.line == 0;
                let issue_kind_str = s.target_token();

                stale.push(StaleSuppression {
                    path: path.clone(),
                    line: s.comment_line,
                    col: 0,
                    origin: SuppressionOrigin::Comment {
                        issue_kind: issue_kind_str,
                        is_file_level,
                        kind_known: true,
                    },
                });
            }
        }

        for (&file_id, unknowns) in &self.unknown_kinds {
            let path = &graph.modules[file_id.0 as usize].path;
            for u in *unknowns {
                stale.push(StaleSuppression {
                    path: path.clone(),
                    line: u.comment_line,
                    col: 0,
                    origin: SuppressionOrigin::Comment {
                        issue_kind: Some(u.token.clone()),
                        is_file_level: u.is_file_level,
                        kind_known: false,
                    },
                });
            }
        }

        stale
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
    pub fn all_suppressions(&self, graph: &ModuleGraph) -> Vec<ActiveSuppression> {
        let mut active = Vec::new();
        for (&file_id, supps) in &self.by_file {
            let path = &graph.modules[file_id.0 as usize].path;
            for s in *supps {
                active.push(ActiveSuppression {
                    path: path.clone(),
                    kind: s.target_token(),
                    is_file_level: s.line == 0,
                });
            }
        }
        active
    }
}

/// Check if a specific issue at a given line should be suppressed.
///
/// Standalone predicate for callers outside `find_dead_code_full`
/// (e.g., CLI health/flags commands) that don't need tracking.
#[must_use]
pub fn is_suppressed(suppressions: &[Suppression], line: u32, kind: IssueKind) -> bool {
    suppressions
        .iter()
        .any(|s| s.matches_issue_kind(line, kind))
}

/// Check if the entire file is suppressed (for issue types that don't have line numbers).
///
/// Standalone predicate for callers outside `find_dead_code_full`.
#[must_use]
pub fn is_file_suppressed(suppressions: &[Suppression], kind: IssueKind) -> bool {
    suppressions
        .iter()
        .any(|s| s.line == 0 && s.matches_issue_kind(0, kind))
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

    #[test]
    fn severity_for_kind_maps_every_core_kind_to_its_field() {
        let rules = RulesConfig {
            unused_exports: Severity::Warn,
            unused_types: Severity::Off,
            unresolved_imports: Severity::Error,
            boundary_violation: Severity::Off,
            ..RulesConfig::default()
        };

        assert_eq!(
            severity_for_kind(&rules, IssueKind::UnusedExport),
            Severity::Warn
        );
        assert_eq!(
            severity_for_kind(&rules, IssueKind::UnusedType),
            Severity::Off
        );
        assert_eq!(
            severity_for_kind(&rules, IssueKind::UnresolvedImport),
            Severity::Error
        );
        assert_eq!(
            severity_for_kind(&rules, IssueKind::BoundaryViolation),
            Severity::Off
        );
        assert_eq!(
            severity_for_kind(&rules, IssueKind::PrivateTypeLeak),
            Severity::Off
        );
    }

    #[test]
    fn issue_kind_from_str_all_variants() {
        assert_eq!(IssueKind::parse("unused-file"), Some(IssueKind::UnusedFile));
        assert_eq!(
            IssueKind::parse("unused-export"),
            Some(IssueKind::UnusedExport)
        );
        assert_eq!(IssueKind::parse("unused-type"), Some(IssueKind::UnusedType));
        assert_eq!(
            IssueKind::parse("unused-dependency"),
            Some(IssueKind::UnusedDependency)
        );
        assert_eq!(
            IssueKind::parse("unused-dev-dependency"),
            Some(IssueKind::UnusedDevDependency)
        );
        assert_eq!(
            IssueKind::parse("unused-enum-member"),
            Some(IssueKind::UnusedEnumMember)
        );
        assert_eq!(
            IssueKind::parse("unused-class-member"),
            Some(IssueKind::UnusedClassMember)
        );
        assert_eq!(
            IssueKind::parse("unresolved-import"),
            Some(IssueKind::UnresolvedImport)
        );
        assert_eq!(
            IssueKind::parse("unlisted-dependency"),
            Some(IssueKind::UnlistedDependency)
        );
        assert_eq!(
            IssueKind::parse("duplicate-export"),
            Some(IssueKind::DuplicateExport)
        );
    }

    #[test]
    fn issue_kind_from_str_unknown() {
        assert_eq!(IssueKind::parse("foo"), None);
        assert_eq!(IssueKind::parse(""), None);
    }

    #[test]
    fn discriminant_roundtrip() {
        for kind in [
            IssueKind::UnusedFile,
            IssueKind::UnusedExport,
            IssueKind::UnusedType,
            IssueKind::PrivateTypeLeak,
            IssueKind::UnusedDependency,
            IssueKind::UnusedDevDependency,
            IssueKind::UnusedEnumMember,
            IssueKind::UnusedClassMember,
            IssueKind::UnresolvedImport,
            IssueKind::UnlistedDependency,
            IssueKind::DuplicateExport,
            IssueKind::CodeDuplication,
            IssueKind::CircularDependency,
            IssueKind::TestOnlyDependency,
            IssueKind::BoundaryViolation,
            IssueKind::CoverageGaps,
            IssueKind::FeatureFlag,
            IssueKind::Complexity,
            IssueKind::StaleSuppression,
            IssueKind::PnpmCatalogEntry,
            IssueKind::EmptyCatalogGroup,
            IssueKind::UnresolvedCatalogReference,
            IssueKind::UnusedDependencyOverride,
            IssueKind::MisconfiguredDependencyOverride,
            IssueKind::ReExportCycle,
            IssueKind::SecurityClientServerLeak,
            IssueKind::SecuritySink,
            IssueKind::PolicyViolation,
            IssueKind::InvalidClientExport,
            IssueKind::MixedClientServerBarrel,
            IssueKind::MisplacedDirective,
            IssueKind::UnusedStoreMember,
            IssueKind::UnprovidedInject,
            IssueKind::RouteCollision,
            IssueKind::DynamicSegmentNameConflict,
            IssueKind::UnrenderedComponent,
            IssueKind::UnusedComponentProp,
            IssueKind::UnusedComponentEmit,
            IssueKind::UnusedServerAction,
            IssueKind::UnusedLoadDataKey,
        ] {
            assert_eq!(
                IssueKind::from_discriminant(kind.to_discriminant()),
                Some(kind)
            );
        }
        assert_eq!(IssueKind::from_discriminant(0), None);
        assert_eq!(IssueKind::from_discriminant(42), None);
    }

    #[test]
    fn parse_file_wide_suppression() {
        let source = "// fallow-ignore-file\nexport const foo = 1;\n";
        let suppressions = parse_suppressions_from_source(source).suppressions;
        assert_eq!(suppressions.len(), 1);
        assert_eq!(suppressions[0].line, 0);
        assert!(suppressions[0].issue_kind_target().is_none());
    }

    #[test]
    fn parse_file_wide_suppression_with_kind() {
        let source = "// fallow-ignore-file unused-export\nexport const foo = 1;\n";
        let suppressions = parse_suppressions_from_source(source).suppressions;
        assert_eq!(suppressions.len(), 1);
        assert_eq!(suppressions[0].line, 0);
        assert_eq!(
            suppressions[0].issue_kind_target(),
            Some(IssueKind::UnusedExport)
        );
    }

    #[test]
    fn parse_next_line_suppression() {
        let source =
            "import { x } from './x';\n// fallow-ignore-next-line\nexport const foo = 1;\n";
        let suppressions = parse_suppressions_from_source(source).suppressions;
        assert_eq!(suppressions.len(), 1);
        assert_eq!(suppressions[0].line, 3); // suppresses line 3 (the export)
        assert!(suppressions[0].issue_kind_target().is_none());
    }

    #[test]
    fn parse_next_line_suppression_with_kind() {
        let source = "// fallow-ignore-next-line unused-export\nexport const foo = 1;\n";
        let suppressions = parse_suppressions_from_source(source).suppressions;
        assert_eq!(suppressions.len(), 1);
        assert_eq!(suppressions[0].line, 2);
        assert_eq!(
            suppressions[0].issue_kind_target(),
            Some(IssueKind::UnusedExport)
        );
    }

    #[test]
    fn parse_unknown_kind_surfaces_as_unknown() {
        let source = "// fallow-ignore-next-line typo-kind\nexport const foo = 1;\n";
        let parsed = parse_suppressions_from_source(source);
        assert!(parsed.suppressions.is_empty());
        assert_eq!(parsed.unknown_kinds.len(), 1);
        assert_eq!(parsed.unknown_kinds[0].token, "typo-kind");
    }

    #[test]
    fn is_suppressed_file_wide() {
        let suppressions = vec![Suppression::all(0, 1)];
        assert!(is_suppressed(&suppressions, 5, IssueKind::UnusedExport));
        assert!(is_suppressed(&suppressions, 10, IssueKind::UnusedFile));
    }

    #[test]
    fn is_suppressed_file_wide_specific_kind() {
        let suppressions = vec![Suppression::issue(0, 1, IssueKind::UnusedExport)];
        assert!(is_suppressed(&suppressions, 5, IssueKind::UnusedExport));
        assert!(!is_suppressed(&suppressions, 5, IssueKind::UnusedType));
    }

    #[test]
    fn is_suppressed_line_specific() {
        let suppressions = vec![Suppression::all(5, 4)];
        assert!(is_suppressed(&suppressions, 5, IssueKind::UnusedExport));
        assert!(!is_suppressed(&suppressions, 6, IssueKind::UnusedExport));
    }

    #[test]
    fn is_suppressed_line_and_kind() {
        let suppressions = vec![Suppression::issue(5, 4, IssueKind::UnusedExport)];
        assert!(is_suppressed(&suppressions, 5, IssueKind::UnusedExport));
        assert!(!is_suppressed(&suppressions, 5, IssueKind::UnusedType));
        assert!(!is_suppressed(&suppressions, 6, IssueKind::UnusedExport));
    }

    #[test]
    fn is_suppressed_empty() {
        assert!(!is_suppressed(&[], 5, IssueKind::UnusedExport));
    }

    #[test]
    fn is_file_suppressed_works() {
        let suppressions = vec![Suppression::all(0, 1)];
        assert!(is_file_suppressed(&suppressions, IssueKind::UnusedFile));

        let suppressions = vec![Suppression::issue(0, 1, IssueKind::UnusedFile)];
        assert!(is_file_suppressed(&suppressions, IssueKind::UnusedFile));
        assert!(!is_file_suppressed(&suppressions, IssueKind::UnusedExport));

        let suppressions = vec![Suppression::all(5, 4)];
        assert!(!is_file_suppressed(&suppressions, IssueKind::UnusedFile));
    }

    #[test]
    fn parse_oxc_comments() {
        use fallow_extract::suppress::parse_suppressions;
        use oxc_allocator::Allocator;
        use oxc_parser::Parser;
        use oxc_span::SourceType;

        let source = "// fallow-ignore-file\n// fallow-ignore-next-line unused-export\nexport const foo = 1;\nexport const bar = 2;\n";
        let allocator = Allocator::default();
        let parser_return = Parser::new(&allocator, source, SourceType::mjs()).parse();

        let suppressions = parse_suppressions(&parser_return.program.comments, source).suppressions;
        assert_eq!(suppressions.len(), 2);

        assert_eq!(suppressions[0].line, 0);
        assert!(suppressions[0].issue_kind_target().is_none());

        assert_eq!(suppressions[1].line, 3); // suppresses line 3 (export const foo)
        assert_eq!(
            suppressions[1].issue_kind_target(),
            Some(IssueKind::UnusedExport)
        );
    }

    #[test]
    fn parse_block_comment_suppression() {
        let source = "/* fallow-ignore-file */\nexport const foo = 1;\n";
        let suppressions = parse_suppressions_from_source(source).suppressions;
        assert_eq!(suppressions.len(), 1);
        assert_eq!(suppressions[0].line, 0);
        assert!(suppressions[0].issue_kind_target().is_none());
    }

    #[test]
    fn is_suppressed_multiple_suppressions_different_kinds() {
        let suppressions = vec![
            Suppression::issue(5, 4, IssueKind::UnusedExport),
            Suppression::issue(5, 4, IssueKind::UnusedType),
        ];
        assert!(is_suppressed(&suppressions, 5, IssueKind::UnusedExport));
        assert!(is_suppressed(&suppressions, 5, IssueKind::UnusedType));
        assert!(!is_suppressed(&suppressions, 5, IssueKind::UnusedFile));
    }

    #[test]
    fn is_suppressed_file_wide_blanket_and_specific_coexist() {
        let suppressions = vec![
            Suppression::issue(0, 1, IssueKind::UnusedExport),
            Suppression::all(5, 4),
        ];
        assert!(is_suppressed(&suppressions, 10, IssueKind::UnusedExport));
        assert!(!is_suppressed(&suppressions, 10, IssueKind::UnusedType));

        assert!(is_suppressed(&suppressions, 5, IssueKind::UnusedType));
        assert!(is_suppressed(&suppressions, 5, IssueKind::UnusedExport));
    }

    #[test]
    fn is_file_suppressed_blanket_suppresses_all_kinds() {
        let suppressions = vec![Suppression::all(0, 1)];
        assert!(is_file_suppressed(&suppressions, IssueKind::UnusedFile));
        assert!(is_file_suppressed(&suppressions, IssueKind::UnusedExport));
        assert!(is_file_suppressed(&suppressions, IssueKind::UnusedType));
        assert!(is_file_suppressed(
            &suppressions,
            IssueKind::CircularDependency
        ));
        assert!(is_file_suppressed(
            &suppressions,
            IssueKind::CodeDuplication
        ));
    }

    #[test]
    fn is_file_suppressed_empty_list() {
        assert!(!is_file_suppressed(&[], IssueKind::UnusedFile));
    }

    #[test]
    fn scoped_policy_suppression_does_not_match_generic_policy_kind() {
        let suppressions = vec![Suppression::policy_rule(5, 4, "team-policy", "no-fs")];
        assert!(!is_suppressed(&suppressions, 5, IssueKind::PolicyViolation));
    }

    #[test]
    fn parse_multiple_next_line_suppressions() {
        let source = "// fallow-ignore-next-line unused-export\nexport const foo = 1;\n// fallow-ignore-next-line unused-type\nexport type Bar = string;\n";
        let suppressions = parse_suppressions_from_source(source).suppressions;
        assert_eq!(suppressions.len(), 2);
        assert_eq!(suppressions[0].line, 2);
        assert_eq!(
            suppressions[0].issue_kind_target(),
            Some(IssueKind::UnusedExport)
        );
        assert_eq!(suppressions[1].line, 4);
        assert_eq!(
            suppressions[1].issue_kind_target(),
            Some(IssueKind::UnusedType)
        );
    }

    #[test]
    fn parse_code_duplication_suppression() {
        let source = "// fallow-ignore-file code-duplication\nexport const foo = 1;\n";
        let suppressions = parse_suppressions_from_source(source).suppressions;
        assert_eq!(suppressions.len(), 1);
        assert_eq!(suppressions[0].line, 0);
        assert_eq!(
            suppressions[0].issue_kind_target(),
            Some(IssueKind::CodeDuplication)
        );
    }

    #[test]
    fn parse_circular_dependency_suppression() {
        let source = "// fallow-ignore-file circular-dependency\nimport { x } from './x';\n";
        let suppressions = parse_suppressions_from_source(source).suppressions;
        assert_eq!(suppressions.len(), 1);
        assert_eq!(suppressions[0].line, 0);
        assert_eq!(
            suppressions[0].issue_kind_target(),
            Some(IssueKind::CircularDependency)
        );
    }

    /// Every `IssueKind` must be explicitly placed in either `NON_CORE_KINDS`
    /// (not checked by core detectors) or handled by a core detector that
    /// calls `SuppressionContext::is_suppressed` / `is_file_suppressed`.
    /// This test fails when a new `IssueKind` variant is added without
    /// being classified, preventing silent false-positive stale reports.
    #[test]
    fn all_issue_kinds_classified_for_stale_detection() {
        let core_kinds = [
            IssueKind::UnusedFile,
            IssueKind::UnusedExport,
            IssueKind::UnusedType,
            IssueKind::UnusedEnumMember,
            IssueKind::UnusedClassMember,
            IssueKind::UnusedStoreMember,
            IssueKind::UnprovidedInject,
            IssueKind::UnresolvedImport,
            IssueKind::DuplicateExport,
            IssueKind::CircularDependency,
            IssueKind::BoundaryViolation,
            IssueKind::InvalidClientExport,
            IssueKind::RouteCollision,
            IssueKind::DynamicSegmentNameConflict,
            IssueKind::UnrenderedComponent,
            IssueKind::UnusedServerAction,
            IssueKind::UnusedLoadDataKey,
        ];

        let all_kinds = [
            IssueKind::UnusedFile,
            IssueKind::UnusedExport,
            IssueKind::UnusedType,
            IssueKind::UnusedDependency,
            IssueKind::UnusedDevDependency,
            IssueKind::UnusedEnumMember,
            IssueKind::UnusedClassMember,
            IssueKind::UnusedStoreMember,
            IssueKind::UnprovidedInject,
            IssueKind::UnresolvedImport,
            IssueKind::UnlistedDependency,
            IssueKind::DuplicateExport,
            IssueKind::CodeDuplication,
            IssueKind::CircularDependency,
            IssueKind::TypeOnlyDependency,
            IssueKind::TestOnlyDependency,
            IssueKind::BoundaryViolation,
            IssueKind::CoverageGaps,
            IssueKind::FeatureFlag,
            IssueKind::Complexity,
            IssueKind::StaleSuppression,
            IssueKind::PnpmCatalogEntry,
            IssueKind::EmptyCatalogGroup,
            IssueKind::UnresolvedCatalogReference,
            IssueKind::UnusedDependencyOverride,
            IssueKind::MisconfiguredDependencyOverride,
            IssueKind::InvalidClientExport,
            IssueKind::RouteCollision,
            IssueKind::DynamicSegmentNameConflict,
            IssueKind::UnrenderedComponent,
            IssueKind::UnusedServerAction,
            IssueKind::UnusedLoadDataKey,
        ];

        for kind in all_kinds {
            let in_core = core_kinds.contains(&kind);
            let in_non_core = NON_CORE_KINDS.contains(&kind);
            assert!(
                in_core || in_non_core,
                "IssueKind::{kind:?} is not classified in either core_kinds or NON_CORE_KINDS. \
                 Add it to NON_CORE_KINDS if it is checked outside find_dead_code_full, \
                 or to core_kinds in this test if a core detector checks it."
            );
            assert!(
                !(in_core && in_non_core),
                "IssueKind::{kind:?} is in BOTH core_kinds and NON_CORE_KINDS. Pick one."
            );
        }
    }
}
