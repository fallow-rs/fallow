//! Inline suppression comment types and issue kind definitions.

/// Issue kind for suppression matching.
///
/// # Examples
///
/// ```
/// use fallow_types::suppress::IssueKind;
///
/// let kind = IssueKind::parse("unused-export");
/// assert_eq!(kind, Some(IssueKind::UnusedExport));
///
/// // Round-trip through discriminant
/// let d = IssueKind::UnusedFile.to_discriminant();
/// assert_eq!(IssueKind::from_discriminant(d), Some(IssueKind::UnusedFile));
///
/// // Unknown strings return None
/// assert_eq!(IssueKind::parse("not-a-kind"), None);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IssueKind {
    /// An unused file.
    UnusedFile,
    /// An unused export.
    UnusedExport,
    /// An unused type export.
    UnusedType,
    /// An exported signature that references a same-file private type.
    PrivateTypeLeak,
    /// An unused dependency.
    UnusedDependency,
    /// An unused dev dependency.
    UnusedDevDependency,
    /// An unused enum member.
    UnusedEnumMember,
    /// An unused class member.
    UnusedClassMember,
    /// An unresolved import.
    UnresolvedImport,
    /// An unlisted dependency.
    UnlistedDependency,
    /// A duplicate export name across modules.
    DuplicateExport,
    /// Code duplication.
    CodeDuplication,
    /// A circular dependency chain.
    CircularDependency,
    /// A production dependency only imported via type-only imports.
    TypeOnlyDependency,
    /// A production dependency only imported by test files.
    TestOnlyDependency,
    /// An import that crosses an architecture boundary.
    BoundaryViolation,
    /// A runtime file or export with no test dependency path.
    CoverageGaps,
    /// A detected feature flag pattern.
    FeatureFlag,
    /// A function exceeding complexity thresholds (health command).
    Complexity,
    /// A suppression comment or JSDoc tag that no longer matches any issue.
    StaleSuppression,
    /// A pnpm catalog entry in pnpm-workspace.yaml not referenced by any workspace package.
    PnpmCatalogEntry,
}

impl IssueKind {
    /// Parse an issue kind from the string tokens used in CLI output and suppression comments.
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "unused-file" => Some(Self::UnusedFile),
            "unused-export" => Some(Self::UnusedExport),
            "unused-type" => Some(Self::UnusedType),
            "private-type-leak" => Some(Self::PrivateTypeLeak),
            "unused-dependency" => Some(Self::UnusedDependency),
            "unused-dev-dependency" => Some(Self::UnusedDevDependency),
            "unused-enum-member" => Some(Self::UnusedEnumMember),
            "unused-class-member" => Some(Self::UnusedClassMember),
            "unresolved-import" => Some(Self::UnresolvedImport),
            "unlisted-dependency" => Some(Self::UnlistedDependency),
            "duplicate-export" => Some(Self::DuplicateExport),
            "code-duplication" => Some(Self::CodeDuplication),
            "circular-dependency" | "circular-dependencies" => Some(Self::CircularDependency),
            "type-only-dependency" => Some(Self::TypeOnlyDependency),
            "test-only-dependency" => Some(Self::TestOnlyDependency),
            "boundary-violation" => Some(Self::BoundaryViolation),
            "coverage-gaps" => Some(Self::CoverageGaps),
            "feature-flag" => Some(Self::FeatureFlag),
            "complexity" => Some(Self::Complexity),
            "stale-suppression" => Some(Self::StaleSuppression),
            "unused-catalog-entry" | "unused-catalog-entries" => Some(Self::PnpmCatalogEntry),
            _ => None,
        }
    }

    /// Convert to a u8 discriminant for compact cache storage.
    #[must_use]
    pub const fn to_discriminant(self) -> u8 {
        match self {
            Self::UnusedFile => 1,
            Self::UnusedExport => 2,
            Self::UnusedType => 3,
            Self::PrivateTypeLeak => 4,
            Self::UnusedDependency => 5,
            Self::UnusedDevDependency => 6,
            Self::UnusedEnumMember => 7,
            Self::UnusedClassMember => 8,
            Self::UnresolvedImport => 9,
            Self::UnlistedDependency => 10,
            Self::DuplicateExport => 11,
            Self::CodeDuplication => 12,
            Self::CircularDependency => 13,
            Self::TypeOnlyDependency => 14,
            Self::TestOnlyDependency => 15,
            Self::BoundaryViolation => 16,
            Self::CoverageGaps => 17,
            Self::FeatureFlag => 18,
            Self::Complexity => 19,
            Self::StaleSuppression => 20,
            Self::PnpmCatalogEntry => 21,
        }
    }

    /// Reconstruct from a cache discriminant.
    #[must_use]
    pub const fn from_discriminant(d: u8) -> Option<Self> {
        match d {
            1 => Some(Self::UnusedFile),
            2 => Some(Self::UnusedExport),
            3 => Some(Self::UnusedType),
            4 => Some(Self::PrivateTypeLeak),
            5 => Some(Self::UnusedDependency),
            6 => Some(Self::UnusedDevDependency),
            7 => Some(Self::UnusedEnumMember),
            8 => Some(Self::UnusedClassMember),
            9 => Some(Self::UnresolvedImport),
            10 => Some(Self::UnlistedDependency),
            11 => Some(Self::DuplicateExport),
            12 => Some(Self::CodeDuplication),
            13 => Some(Self::CircularDependency),
            14 => Some(Self::TypeOnlyDependency),
            15 => Some(Self::TestOnlyDependency),
            16 => Some(Self::BoundaryViolation),
            17 => Some(Self::CoverageGaps),
            18 => Some(Self::FeatureFlag),
            19 => Some(Self::Complexity),
            20 => Some(Self::StaleSuppression),
            21 => Some(Self::PnpmCatalogEntry),
            _ => None,
        }
    }
}

/// A suppression directive parsed from a source comment.
///
/// # Examples
///
/// ```
/// use fallow_types::suppress::{Suppression, IssueKind};
///
/// // File-wide suppression (line 0, no specific kind)
/// let file_wide = Suppression { line: 0, comment_line: 1, kind: None };
/// assert_eq!(file_wide.line, 0);
///
/// // Line-specific suppression for unused exports
/// let line_suppress = Suppression {
///     line: 42,
///     comment_line: 41,
///     kind: Some(IssueKind::UnusedExport),
/// };
/// assert_eq!(line_suppress.kind, Some(IssueKind::UnusedExport));
/// ```
#[derive(Debug, Clone)]
pub struct Suppression {
    /// 1-based line this suppression applies to. 0 = file-wide suppression.
    pub line: u32,
    /// 1-based line where the suppression comment itself appears.
    /// For `fallow-ignore-next-line`, this is `line - 1`.
    /// For `fallow-ignore-file`, this is the actual line of the comment in the source.
    pub comment_line: u32,
    /// None = suppress all issue kinds on this line.
    pub kind: Option<IssueKind>,
}

// Size assertions to prevent memory regressions.
// `Suppression` is stored in a Vec per file; `IssueKind` appears in every suppression.
const _: () = assert!(std::mem::size_of::<Suppression>() == 12);
const _: () = assert!(std::mem::size_of::<IssueKind>() == 1);

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
            IssueKind::parse("private-type-leak"),
            Some(IssueKind::PrivateTypeLeak)
        );
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
        assert_eq!(
            IssueKind::parse("code-duplication"),
            Some(IssueKind::CodeDuplication)
        );
        assert_eq!(
            IssueKind::parse("circular-dependency"),
            Some(IssueKind::CircularDependency)
        );
        assert_eq!(
            IssueKind::parse("circular-dependencies"),
            Some(IssueKind::CircularDependency)
        );
        assert_eq!(
            IssueKind::parse("type-only-dependency"),
            Some(IssueKind::TypeOnlyDependency)
        );
        assert_eq!(
            IssueKind::parse("test-only-dependency"),
            Some(IssueKind::TestOnlyDependency)
        );
        assert_eq!(
            IssueKind::parse("boundary-violation"),
            Some(IssueKind::BoundaryViolation)
        );
        assert_eq!(
            IssueKind::parse("coverage-gaps"),
            Some(IssueKind::CoverageGaps)
        );
        assert_eq!(
            IssueKind::parse("feature-flag"),
            Some(IssueKind::FeatureFlag)
        );
        assert_eq!(IssueKind::parse("complexity"), Some(IssueKind::Complexity));
        assert_eq!(
            IssueKind::parse("stale-suppression"),
            Some(IssueKind::StaleSuppression)
        );
        assert_eq!(
            IssueKind::parse("unused-catalog-entry"),
            Some(IssueKind::PnpmCatalogEntry)
        );
        assert_eq!(
            IssueKind::parse("unused-catalog-entries"),
            Some(IssueKind::PnpmCatalogEntry)
        );
    }

    #[test]
    fn issue_kind_from_str_unknown() {
        assert_eq!(IssueKind::parse("foo"), None);
        assert_eq!(IssueKind::parse(""), None);
    }

    #[test]
    fn issue_kind_from_str_near_misses() {
        // Case sensitivity — these should NOT match
        assert_eq!(IssueKind::parse("Unused-File"), None);
        assert_eq!(IssueKind::parse("UNUSED-EXPORT"), None);
        // Typos / near-misses
        assert_eq!(IssueKind::parse("unused_file"), None);
        assert_eq!(IssueKind::parse("unused-files"), None);
    }

    #[test]
    fn discriminant_out_of_range() {
        assert_eq!(IssueKind::from_discriminant(0), None);
        assert_eq!(IssueKind::from_discriminant(22), None);
        assert_eq!(IssueKind::from_discriminant(u8::MAX), None);
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
            IssueKind::TypeOnlyDependency,
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

    // ── Discriminant uniqueness ─────────────────────────────────

    #[test]
    fn discriminant_values_are_unique() {
        let all_kinds = [
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
            IssueKind::TypeOnlyDependency,
            IssueKind::TestOnlyDependency,
            IssueKind::BoundaryViolation,
            IssueKind::CoverageGaps,
            IssueKind::FeatureFlag,
            IssueKind::Complexity,
            IssueKind::StaleSuppression,
            IssueKind::PnpmCatalogEntry,
        ];
        let discriminants: Vec<u8> = all_kinds.iter().map(|k| k.to_discriminant()).collect();
        let mut sorted = discriminants.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(
            discriminants.len(),
            sorted.len(),
            "discriminant values must be unique"
        );
    }

    // ── Discriminant starts at 1 ────────────────────────────────

    #[test]
    fn discriminant_starts_at_one() {
        assert_eq!(IssueKind::UnusedFile.to_discriminant(), 1);
    }

    // ── Suppression struct ──────────────────────────────────────

    #[test]
    fn suppression_line_zero_is_file_wide() {
        let s = Suppression {
            line: 0,
            comment_line: 1,
            kind: None,
        };
        assert_eq!(s.line, 0);
        assert!(s.kind.is_none());
    }

    #[test]
    fn suppression_with_specific_kind_and_line() {
        let s = Suppression {
            line: 42,
            comment_line: 41,
            kind: Some(IssueKind::UnusedExport),
        };
        assert_eq!(s.line, 42);
        assert_eq!(s.comment_line, 41);
        assert_eq!(s.kind, Some(IssueKind::UnusedExport));
    }
}
