//! Audit review-brief ownership output contracts.

use serde::Serialize;

/// Maximum number of owner groups listed in [`OwnershipFacts::groups`]. The
/// groups beyond the cap are the lightest ones and are counted in
/// [`OwnershipFacts::groups_omitted`].
pub const OWNER_GROUP_CAP: usize = 10;

/// How far a changeset reaches across CODEOWNERS owner groups.
///
/// Computed from the CODEOWNERS file alone: no git history is read, so the
/// section is present whenever a CODEOWNERS file is found, also when the churn
/// walk behind `routing` finds nothing. Each file maps to its primary owner
/// (the first owner of the last matching rule). A file that no rule matches,
/// or that a GitLab negation rule matches, belongs to the `(unowned)` group.
/// The owner strings use the same vocabulary as `routing.units[].expert`.
///
/// Absent from the brief when no CODEOWNERS file is found, or when the file
/// cannot be read or does not parse. A configured `codeowners` path that
/// fails also prints a warning on stderr.
///
/// `groups[].direct_count` counts all changed files, source or not, so the
/// sum over all groups is the number of changed files, not the size of
/// `impact_closure.in_diff`. Slice owners count only the files of the
/// partition units, which are source files.
#[derive(Debug, Clone, Default, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct OwnershipFacts {
    /// Distinct owner groups across the changed files and the impact closure.
    /// The `(unowned)` group counts as one group. Exact, never capped.
    pub group_count: usize,
    /// Owner groups that own no changed file and appear only through the
    /// impact closure. Exact, never capped.
    pub transitive_only_count: usize,
    /// Changed files that belong to the `(unowned)` group.
    pub unowned_direct_count: usize,
    /// The owner groups, sorted by `direct_count` descending, then
    /// `affected_count` descending, then `owner`. At most [`OWNER_GROUP_CAP`]
    /// entries. The counts in each entry are exact.
    pub groups: Vec<OwnerGroupFact>,
    /// How many owner groups did not fit within [`OWNER_GROUP_CAP`] and are
    /// absent from `groups`. Zero when nothing was omitted.
    pub groups_omitted: usize,
    /// The owner set of each independent slice, aligned by index with
    /// `partition.independent_slices`. Present only when that list is present
    /// (two or more slices). A fact for the reviewer, never a demand to split.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub slices: Vec<OwnershipSliceFact>,
}

/// One owner group and how many files of the changeset it owns.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct OwnerGroupFact {
    /// The CODEOWNERS owner (`@user`, `@org/team`, or an email), or
    /// `(unowned)`.
    pub owner: String,
    /// Changed files this group owns.
    pub direct_count: usize,
    /// Files of the impact closure (affected, not in the diff) this group owns.
    pub affected_count: usize,
}

/// The owners of one independent slice of the partition.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct OwnershipSliceFact {
    /// The module directories of the slice, as in
    /// `partition.independent_slices`.
    pub module_dirs: Vec<String>,
    /// The distinct owners of the changed files in the slice, sorted. The
    /// `(unowned)` group is a distinct owner. Never empty.
    pub owners: Vec<String>,
    /// True when the slice has exactly one owner, so one owner group can
    /// review it on its own.
    pub separable: bool,
}
