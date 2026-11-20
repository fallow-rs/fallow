//! The duplication options of a surface over the `duplicates` config
//! section: one merge for the CLI flags, the options of the Rust API and the
//! Node bindings, and the duplication settings of the editor.

use crate::{DetectionMode, DuplicatesConfig};

/// The duplication options that a surface gives. `None` keeps the value of
/// the config. A given value wins over the config for every key, also
/// `false` over a config `true`.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct DupesInputs {
    /// The detection mode.
    pub mode: Option<DetectionMode>,
    /// Detect function-scoped near-miss clones.
    pub near: Option<bool>,
    /// The minimum number of tokens of a clone.
    pub min_tokens: Option<usize>,
    /// The minimum number of lines of a clone.
    pub min_lines: Option<usize>,
    /// The minimum number of instances of a reported clone group.
    pub min_occurrences: Option<usize>,
    /// The duplication percentage above which the gate fails.
    pub threshold: Option<f64>,
    /// Report only clones across directories.
    pub skip_local: Option<bool>,
    /// Match clones across languages.
    pub cross_language: Option<bool>,
    /// Leave import declarations out of clone detection.
    pub ignore_imports: Option<bool>,
    /// Leave symlinked clone instances out.
    pub ignore_symlinks: Option<bool>,
}

impl DuplicatesConfig {
    /// The config with each option of `inputs` over the value of the config.
    /// The `enabled` key and the keys that no surface gives keep the value
    /// of the config.
    #[must_use]
    pub fn merged(&self, inputs: &DupesInputs) -> Self {
        Self {
            mode: inputs.mode.unwrap_or(self.mode),
            near: inputs.near.unwrap_or(self.near),
            min_tokens: inputs.min_tokens.unwrap_or(self.min_tokens),
            min_lines: inputs.min_lines.unwrap_or(self.min_lines),
            min_occurrences: inputs.min_occurrences.unwrap_or(self.min_occurrences),
            threshold: inputs.threshold.unwrap_or(self.threshold),
            skip_local: inputs.skip_local.unwrap_or(self.skip_local),
            cross_language: inputs.cross_language.unwrap_or(self.cross_language),
            ignore_imports: inputs.ignore_imports.unwrap_or(self.ignore_imports),
            ignore_symlinks: inputs.ignore_symlinks.unwrap_or(self.ignore_symlinks),
            ..self.clone()
        }
    }

    /// [`Self::merged`] for a runner whose run plan already selected the
    /// duplication analysis, so the config section does not turn it off.
    #[must_use]
    pub fn merged_for_selected_run(&self, inputs: &DupesInputs) -> Self {
        Self {
            enabled: true,
            ..self.merged(inputs)
        }
    }
}

#[cfg(test)]
#[path = "dupes_tests.rs"]
mod tests;
