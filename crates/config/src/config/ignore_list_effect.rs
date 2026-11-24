//! Which entries of `ignoreExports`, `duplicates.ignore` and `health.ignore`
//! matched a file or an export in the latest dead-code pass (issue #5167).

use super::{EntryEffectRecord, ResolvedConfig};

/// The export name of an `ignoreExports` rule that covers every export of
/// its files. It names no single export, so it can only miss as a file glob.
const ALL_EXPORTS: &str = "*";

/// One answer per entry of each list. The dead-code pass writes them from
/// the module graph, and the dead-code envelope reads them.
#[derive(Debug, Clone, Default)]
pub struct IgnoreListEffect {
    /// One flag per `ignoreExports` rule: its `file` glob matched a file.
    pub ignore_export_files: EntryEffectRecord,
    /// One flag per name of [`ResolvedConfig::ignore_export_names`]: a file
    /// of its rule declares the name.
    pub ignore_export_names: EntryEffectRecord,
    /// One flag per `duplicates.ignore` glob: it matched a file.
    pub duplicates_ignore: EntryEffectRecord,
    /// One flag per `health.ignore` glob: it matched a file.
    pub health_ignore: EntryEffectRecord,
}

/// An export name of an `ignoreExports` rule, with the `file` glob of the
/// rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IgnoreExportName<'a> {
    /// The index of the rule in `ignoreExports`.
    pub rule: usize,
    /// The `file` glob of the rule, as written in the config.
    pub file: &'a str,
    /// The export name, as written in the config.
    pub export: &'a str,
}

impl ResolvedConfig {
    /// The export names of every `ignoreExports` rule in config order,
    /// without `*`. [`IgnoreListEffect::ignore_export_names`] has one flag
    /// per entry of this list.
    #[must_use]
    pub fn ignore_export_names(&self) -> Vec<IgnoreExportName<'_>> {
        self.ignore_export_rules
            .iter()
            .enumerate()
            .flat_map(|(rule, entry)| {
                entry
                    .exports
                    .iter()
                    .filter(|export| export.as_str() != ALL_EXPORTS)
                    .map(move |export| IgnoreExportName {
                        rule,
                        file: entry.file.as_str(),
                        export: export.as_str(),
                    })
            })
            .collect()
    }
}
