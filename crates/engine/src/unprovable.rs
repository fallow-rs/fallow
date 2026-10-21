//! Finding classes that one run cannot prove.
//!
//! Some detectors do not report anything when the run cannot see all of the
//! code that can disprove a finding. The audit reads
//! [`unprovable_dead_code_collections`] on the base run: a head finding of a
//! class that the base could not prove is inherited, not introduced, because
//! the base did not measure that class.

use fallow_config::{ResolvedConfig, Severity};

/// JSON collection key of the unused-catalog-entries findings.
const UNUSED_CATALOG_ENTRIES: &str = "unused_catalog_entries";

/// The dead-code collections, by JSON key, that a run over `config` does not
/// report because it cannot prove their findings.
///
/// The catalog analysis skips unused catalog entries on the same predicate,
/// [`fallow_config::has_missing_workspace_pattern_dirs`]. A collection whose
/// rule is `off` is not in the list: the run does not report it for a
/// different reason.
#[must_use]
pub fn unprovable_dead_code_collections(config: &ResolvedConfig) -> Vec<&'static str> {
    let mut collections = Vec::new();
    if config.rules.unused_catalog_entries != Severity::Off
        && fallow_config::has_missing_workspace_pattern_dirs(
            &config.root,
            &config.workspace_patterns,
        )
    {
        collections.push(UNUSED_CATALOG_ENTRIES);
    }
    collections
}
