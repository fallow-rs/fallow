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
}
