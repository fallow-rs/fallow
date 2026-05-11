use std::sync::atomic::{AtomicBool, Ordering};

use rustc_hash::FxHashMap;

// Re-export types from fallow-types
pub use fallow_types::suppress::{IssueKind, Suppression};

// Re-export parsing functions from fallow-extract
pub use fallow_extract::suppress::parse_suppressions_from_source;

use crate::discover::FileId;
use crate::extract::ModuleInfo;
use crate::graph::ModuleGraph;
use crate::results::{StaleSuppression, SuppressionOrigin};

/// Issue kinds whose suppression is not checked via `SuppressionContext`
/// in `find_dead_code_full`. Excludes CLI-side kinds (checked in health/flags
/// commands) and dependency-level kinds (not file-scoped, suppression never
/// consumed by core detectors). Without this exclusion, these suppressions
/// would always appear stale since no core detector checks them.
const NON_CORE_KINDS: &[IssueKind] = &[
    // CLI-side: checked in health/flags/dupes commands, not in find_dead_code_full
    IssueKind::Complexity,
    IssueKind::CoverageGaps,
    IssueKind::FeatureFlag,
    IssueKind::CodeDuplication,
    // Dep-level: not file-scoped, suppression path is via config ignoreDependencies
    IssueKind::UnusedDependency,
    IssueKind::UnusedDevDependency,
    IssueKind::UnlistedDependency,
    IssueKind::TypeOnlyDependency,
    IssueKind::TestOnlyDependency,
    // Meta: stale-suppression itself is never consumed by any detector,
    // so a `// fallow-ignore-next-line stale-suppression` comment would
    // always appear stale. Exclude to prevent recursive confusion.
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

        Self { by_file, used }
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
        Self { by_file, used }
    }

    /// Build an empty suppression context (for testing).
    #[cfg(test)]
    pub fn empty() -> Self {
        Self {
            by_file: FxHashMap::default(),
            used: FxHashMap::default(),
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
            let matched = if s.line == 0 {
                s.kind.is_none() || s.kind == Some(kind)
            } else {
                s.line == line && (s.kind.is_none() || s.kind == Some(kind))
            };
            if matched {
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
            if s.line == 0 && (s.kind.is_none() || s.kind == Some(kind)) {
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
    /// to avoid false positives.
    pub fn find_stale(&self, graph: &ModuleGraph) -> Vec<StaleSuppression> {
        let mut stale = Vec::new();

        for (&file_id, supps) in &self.by_file {
            let used = &self.used[&file_id];
            let path = &graph.modules[file_id.0 as usize].path;

            for (i, s) in supps.iter().enumerate() {
                if used[i].load(Ordering::Relaxed) {
                    continue;
                }

                // Skip suppression kinds that are only checked in the CLI layer.
                // These were never presented to the core detectors, so they
                // appear unconsumed, but are not actually stale.
                if let Some(kind) = s.kind
                    && NON_CORE_KINDS.contains(&kind)
                {
                    continue;
                }

                let is_file_level = s.line == 0;
                let issue_kind_str = s.kind.map(|k| {
                    // Convert back to the kebab-case string for output
                    match k {
                        IssueKind::UnusedFile => "unused-file",
                        IssueKind::UnusedExport => "unused-export",
                        IssueKind::UnusedType => "unused-type",
                        IssueKind::PrivateTypeLeak => "private-type-leak",
                        IssueKind::UnusedDependency => "unused-dependency",
                        IssueKind::UnusedDevDependency => "unused-dev-dependency",
                        IssueKind::UnusedEnumMember => "unused-enum-member",
                        IssueKind::UnusedClassMember => "unused-class-member",
                        IssueKind::UnresolvedImport => "unresolved-import",
                        IssueKind::UnlistedDependency => "unlisted-dependency",
                        IssueKind::DuplicateExport => "duplicate-export",
                        IssueKind::CodeDuplication => "code-duplication",
                        IssueKind::CircularDependency => "circular-dependency",
                        IssueKind::TypeOnlyDependency => "type-only-dependency",
                        IssueKind::TestOnlyDependency => "test-only-dependency",
                        IssueKind::BoundaryViolation => "boundary-violation",
                        IssueKind::CoverageGaps => "coverage-gaps",
                        IssueKind::FeatureFlag => "feature-flag",
                        IssueKind::Complexity => "complexity",
                        IssueKind::StaleSuppression => "stale-suppression",
                        IssueKind::PnpmCatalogEntry => "unused-catalog-entry",
                    }
                    .to_string()
                });

                stale.push(StaleSuppression {
                    path: path.clone(),
                    line: s.comment_line,
                    col: 0,
                    origin: SuppressionOrigin::Comment {
                        issue_kind: issue_kind_str,
                        is_file_level,
                    },
                });
            }
        }

        stale
    }
}

/// Check if a specific issue at a given line should be suppressed.
///
/// Standalone predicate for callers outside `find_dead_code_full`
/// (e.g., CLI health/flags commands) that don't need tracking.
#[must_use]
pub fn is_suppressed(suppressions: &[Suppression], line: u32, kind: IssueKind) -> bool {
    suppressions.iter().any(|s| {
        // File-wide suppression
        if s.line == 0 {
            return s.kind.is_none() || s.kind == Some(kind);
        }
        // Line-specific suppression
        s.line == line && (s.kind.is_none() || s.kind == Some(kind))
    })
}

/// Check if the entire file is suppressed (for issue types that don't have line numbers).
///
/// Standalone predicate for callers outside `find_dead_code_full`.
#[must_use]
pub fn is_file_suppressed(suppressions: &[Suppression], kind: IssueKind) -> bool {
    suppressions
        .iter()
        .any(|s| s.line == 0 && (s.kind.is_none() || s.kind == Some(kind)))
}

#[cfg(test)]
mod tests {
    use super::*;

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
        ] {
            assert_eq!(
                IssueKind::from_discriminant(kind.to_discriminant()),
                Some(kind)
            );
        }
        assert_eq!(IssueKind::from_discriminant(0), None);
        assert_eq!(IssueKind::from_discriminant(22), None);
    }

    #[test]
    fn parse_file_wide_suppression() {
        let source = "// fallow-ignore-file\nexport const foo = 1;\n";
        let suppressions = parse_suppressions_from_source(source);
        assert_eq!(suppressions.len(), 1);
        assert_eq!(suppressions[0].line, 0);
        assert!(suppressions[0].kind.is_none());
    }

    #[test]
    fn parse_file_wide_suppression_with_kind() {
        let source = "// fallow-ignore-file unused-export\nexport const foo = 1;\n";
        let suppressions = parse_suppressions_from_source(source);
        assert_eq!(suppressions.len(), 1);
        assert_eq!(suppressions[0].line, 0);
        assert_eq!(suppressions[0].kind, Some(IssueKind::UnusedExport));
    }

    #[test]
    fn parse_next_line_suppression() {
        let source =
            "import { x } from './x';\n// fallow-ignore-next-line\nexport const foo = 1;\n";
        let suppressions = parse_suppressions_from_source(source);
        assert_eq!(suppressions.len(), 1);
        assert_eq!(suppressions[0].line, 3); // suppresses line 3 (the export)
        assert!(suppressions[0].kind.is_none());
    }

    #[test]
    fn parse_next_line_suppression_with_kind() {
        let source = "// fallow-ignore-next-line unused-export\nexport const foo = 1;\n";
        let suppressions = parse_suppressions_from_source(source);
        assert_eq!(suppressions.len(), 1);
        assert_eq!(suppressions[0].line, 2);
        assert_eq!(suppressions[0].kind, Some(IssueKind::UnusedExport));
    }

    #[test]
    fn parse_unknown_kind_ignored() {
        let source = "// fallow-ignore-next-line typo-kind\nexport const foo = 1;\n";
        let suppressions = parse_suppressions_from_source(source);
        assert!(suppressions.is_empty());
    }

    #[test]
    fn is_suppressed_file_wide() {
        let suppressions = vec![Suppression {
            line: 0,
            comment_line: 1,
            kind: None,
        }];
        assert!(is_suppressed(&suppressions, 5, IssueKind::UnusedExport));
        assert!(is_suppressed(&suppressions, 10, IssueKind::UnusedFile));
    }

    #[test]
    fn is_suppressed_file_wide_specific_kind() {
        let suppressions = vec![Suppression {
            line: 0,
            comment_line: 1,
            kind: Some(IssueKind::UnusedExport),
        }];
        assert!(is_suppressed(&suppressions, 5, IssueKind::UnusedExport));
        assert!(!is_suppressed(&suppressions, 5, IssueKind::UnusedType));
    }

    #[test]
    fn is_suppressed_line_specific() {
        let suppressions = vec![Suppression {
            line: 5,
            comment_line: 4,
            kind: None,
        }];
        assert!(is_suppressed(&suppressions, 5, IssueKind::UnusedExport));
        assert!(!is_suppressed(&suppressions, 6, IssueKind::UnusedExport));
    }

    #[test]
    fn is_suppressed_line_and_kind() {
        let suppressions = vec![Suppression {
            line: 5,
            comment_line: 4,
            kind: Some(IssueKind::UnusedExport),
        }];
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
        let suppressions = vec![Suppression {
            line: 0,
            comment_line: 1,
            kind: None,
        }];
        assert!(is_file_suppressed(&suppressions, IssueKind::UnusedFile));

        let suppressions = vec![Suppression {
            line: 0,
            comment_line: 1,
            kind: Some(IssueKind::UnusedFile),
        }];
        assert!(is_file_suppressed(&suppressions, IssueKind::UnusedFile));
        assert!(!is_file_suppressed(&suppressions, IssueKind::UnusedExport));

        // Line-specific suppression should not count as file-wide
        let suppressions = vec![Suppression {
            line: 5,
            comment_line: 4,
            kind: None,
        }];
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

        let suppressions = parse_suppressions(&parser_return.program.comments, source);
        assert_eq!(suppressions.len(), 2);

        // File-wide suppression
        assert_eq!(suppressions[0].line, 0);
        assert!(suppressions[0].kind.is_none());

        // Next-line suppression with kind
        assert_eq!(suppressions[1].line, 3); // suppresses line 3 (export const foo)
        assert_eq!(suppressions[1].kind, Some(IssueKind::UnusedExport));
    }

    #[test]
    fn parse_block_comment_suppression() {
        let source = "/* fallow-ignore-file */\nexport const foo = 1;\n";
        let suppressions = parse_suppressions_from_source(source);
        assert_eq!(suppressions.len(), 1);
        assert_eq!(suppressions[0].line, 0);
        assert!(suppressions[0].kind.is_none());
    }

    #[test]
    fn is_suppressed_multiple_suppressions_different_kinds() {
        let suppressions = vec![
            Suppression {
                line: 5,
                comment_line: 4,
                kind: Some(IssueKind::UnusedExport),
            },
            Suppression {
                line: 5,
                comment_line: 4,
                kind: Some(IssueKind::UnusedType),
            },
        ];
        assert!(is_suppressed(&suppressions, 5, IssueKind::UnusedExport));
        assert!(is_suppressed(&suppressions, 5, IssueKind::UnusedType));
        assert!(!is_suppressed(&suppressions, 5, IssueKind::UnusedFile));
    }

    #[test]
    fn is_suppressed_file_wide_blanket_and_specific_coexist() {
        let suppressions = vec![
            Suppression {
                line: 0,
                comment_line: 1,
                kind: Some(IssueKind::UnusedExport),
            },
            Suppression {
                line: 5,
                comment_line: 4,
                kind: None, // blanket suppress on line 5
            },
        ];
        // File-wide suppression only covers UnusedExport
        assert!(is_suppressed(&suppressions, 10, IssueKind::UnusedExport));
        assert!(!is_suppressed(&suppressions, 10, IssueKind::UnusedType));

        // Line 5 blanket suppression covers everything on line 5
        assert!(is_suppressed(&suppressions, 5, IssueKind::UnusedType));
        assert!(is_suppressed(&suppressions, 5, IssueKind::UnusedExport));
    }

    #[test]
    fn is_file_suppressed_blanket_suppresses_all_kinds() {
        let suppressions = vec![Suppression {
            line: 0,
            comment_line: 1,
            kind: None, // blanket file-wide
        }];
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
    fn parse_multiple_next_line_suppressions() {
        let source = "// fallow-ignore-next-line unused-export\nexport const foo = 1;\n// fallow-ignore-next-line unused-type\nexport type Bar = string;\n";
        let suppressions = parse_suppressions_from_source(source);
        assert_eq!(suppressions.len(), 2);
        assert_eq!(suppressions[0].line, 2);
        assert_eq!(suppressions[0].kind, Some(IssueKind::UnusedExport));
        assert_eq!(suppressions[1].line, 4);
        assert_eq!(suppressions[1].kind, Some(IssueKind::UnusedType));
    }

    #[test]
    fn parse_code_duplication_suppression() {
        let source = "// fallow-ignore-file code-duplication\nexport const foo = 1;\n";
        let suppressions = parse_suppressions_from_source(source);
        assert_eq!(suppressions.len(), 1);
        assert_eq!(suppressions[0].line, 0);
        assert_eq!(suppressions[0].kind, Some(IssueKind::CodeDuplication));
    }

    #[test]
    fn parse_circular_dependency_suppression() {
        let source = "// fallow-ignore-file circular-dependency\nimport { x } from './x';\n";
        let suppressions = parse_suppressions_from_source(source);
        assert_eq!(suppressions.len(), 1);
        assert_eq!(suppressions[0].line, 0);
        assert_eq!(suppressions[0].kind, Some(IssueKind::CircularDependency));
    }

    /// Every `IssueKind` must be explicitly placed in either `NON_CORE_KINDS`
    /// (not checked by core detectors) or handled by a core detector that
    /// calls `SuppressionContext::is_suppressed` / `is_file_suppressed`.
    /// This test fails when a new `IssueKind` variant is added without
    /// being classified, preventing silent false-positive stale reports.
    #[test]
    fn all_issue_kinds_classified_for_stale_detection() {
        // Kinds checked by core detectors via SuppressionContext
        let core_kinds = [
            IssueKind::UnusedFile,
            IssueKind::UnusedExport,
            IssueKind::UnusedType,
            IssueKind::UnusedEnumMember,
            IssueKind::UnusedClassMember,
            IssueKind::UnresolvedImport,
            IssueKind::DuplicateExport,
            IssueKind::CircularDependency,
            IssueKind::BoundaryViolation,
        ];

        // All variants that exist
        let all_kinds = [
            IssueKind::UnusedFile,
            IssueKind::UnusedExport,
            IssueKind::UnusedType,
            IssueKind::UnusedDependency,
            IssueKind::UnusedDevDependency,
            IssueKind::UnusedEnumMember,
            IssueKind::UnusedClassMember,
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
