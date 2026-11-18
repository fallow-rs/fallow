//! The sub-analyses of one `fallow audit` run.

use crate::check::ArchitectureSelection;

/// The sub-analyses one audit runs, resolved from `--only`, `--skip` and the
/// `audit.deadCode` / `audit.health` / `audit.dupes` config keys.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AuditAnalysisSelection {
    pub dead_code: bool,
    pub health: bool,
    pub dupes: bool,
    /// Which dead-code findings stay in the report (`--only architecture`,
    /// `--skip architecture`).
    pub architecture: ArchitectureSelection,
    /// `duplicates.enabled` is false, whatever the flags select.
    pub duplicates_off_in_config: bool,
}

impl AuditAnalysisSelection {
    /// Every sub-analysis with every dead-code finding: the default.
    pub const ALL: Self = Self {
        dead_code: true,
        health: true,
        dupes: true,
        architecture: ArchitectureSelection::All,
        duplicates_off_in_config: false,
    };

    /// The cache-key label of the architecture selection.
    pub const fn architecture_label(self) -> &'static str {
        match self.architecture {
            ArchitectureSelection::All => "all",
            ArchitectureSelection::Only => "only",
            ArchitectureSelection::Exclude => "exclude",
        }
    }

    /// Whether the dead-code and health sub-analyses run with every dead-code
    /// finding. The duplication sub-analysis can run or not.
    pub const fn is_full_without_dupes(self) -> bool {
        self.dead_code && self.health && matches!(self.architecture, ArchitectureSelection::All)
    }

    /// Whether a run with this selection writes an Impact record. The audit
    /// and the bare `fallow` run both use this rule.
    ///
    /// A skipped analysis reports no findings, which Impact would credit as
    /// resolved. So only a full selection writes a record. Duplication that
    /// `duplicates.enabled: false` turned off for the project is no skip of
    /// one run: the other findings still need a history. A record whose run
    /// did not execute duplication credits no clone group as resolved and
    /// keeps the clone groups that an earlier run stored.
    pub const fn records_impact(self) -> bool {
        self.is_full_without_dupes() && (self.dupes || self.duplicates_off_in_config)
    }
}
