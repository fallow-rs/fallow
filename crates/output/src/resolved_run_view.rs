//! The serializable view of one resolved run: each decision with its source.

use std::path::PathBuf;

use fallow_config::{CommandKind, Surface};
use fallow_types::sourced::Sourced;
use serde::Serialize;

/// The schema version of [`ResolvedRunView`].
pub const RESOLVED_RUN_VIEW_SCHEMA_VERSION: u32 = 1;

/// Every decision of one run, with the input that gave it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ResolvedRunView {
    /// The schema version of this view.
    pub schema_version: u32,
    /// The surface that started the run.
    pub surface: Surface,
    /// The command of the run.
    pub command: CommandKind,
    /// The project root.
    pub root: Sourced<PathBuf>,
    /// The config file. `null` when the run uses the default config.
    pub config_file: Sourced<Option<PathBuf>>,
    /// The decisions of each analysis.
    pub analyses: AnalysesView,
    /// The settings of the run.
    pub settings: RunSettingsView,
    /// The environment variables of the run, by name. The view never holds a
    /// value.
    pub env: EnvView,
    /// Whether the run uses the default config because the discovered config
    /// failed.
    pub fell_back: bool,
}

/// The decisions of each analysis.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AnalysesView {
    /// The dead-code analysis.
    pub dead_code: AnalysisView,
    /// The duplication analysis.
    pub duplication: AnalysisView,
    /// The health analysis.
    pub health: AnalysisView,
}

/// The decisions of one analysis.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AnalysisView {
    /// Whether the analysis runs.
    pub selection: Sourced<SelectionView>,
    /// The production mode of the analysis.
    pub production: Sourced<bool>,
}

/// Whether an analysis runs, and why it does not.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum SelectionView {
    /// The analysis runs.
    Run,
    /// The request excludes the analysis.
    NotRequested,
    /// A config key turns the analysis off.
    OffByConfig,
}

/// The settings of one run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RunSettingsView {
    /// The number of worker threads.
    pub threads: Sourced<usize>,
    /// Whether the run bypasses the parse cache.
    pub no_cache: Sourced<bool>,
    /// Whether the `extends` chain can name `https://` entries.
    pub allow_remote_extends: Sourced<bool>,
    /// The largest file to analyze, in bytes. `null` means no limit.
    pub max_file_size_bytes: Sourced<Option<u64>>,
    /// The cache directory.
    pub cache_dir: Sourced<PathBuf>,
    /// The size limit of the parse cache, in megabytes.
    pub cache_max_size_mb: Sourced<Option<u32>>,
    /// Whether the run reads `workspaces.changedSince`.
    pub package_baselines: Sourced<bool>,
    /// Whether the run reports unused exports of entry files.
    pub include_entry_exports: Sourced<bool>,
}

/// The environment variables of one run, by name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EnvView {
    /// The set variables that the run reads.
    pub read: Vec<&'static str>,
    /// The set variables that the run ignores on its surface and command.
    pub ignored: Vec<&'static str>,
}

#[cfg(test)]
mod tests {
    use fallow_types::sourced::{Scope, Source};

    use super::*;

    #[test]
    fn analysis_view_serializes_the_value_and_the_source() {
        let view = AnalysisView {
            selection: Sourced::new(
                SelectionView::OffByConfig,
                Source::ConfigFile {
                    file: PathBuf::from("project/.fallowrc.json"),
                    key: "duplicates.enabled",
                },
                Scope::Analysis,
            ),
            production: Sourced::new(
                true,
                Source::Environment {
                    name: "FALLOW_PRODUCTION",
                },
                Scope::Global,
            ),
        };
        assert_eq!(
            serde_json::to_value(&view).expect("view serializes"),
            serde_json::json!({
                "selection": {
                    "value": "off-by-config",
                    "source": {
                        "kind": "config-file",
                        "file": "project/.fallowrc.json",
                        "key": "duplicates.enabled",
                    },
                    "scope": "analysis",
                },
                "production": {
                    "value": true,
                    "source": { "kind": "environment", "name": "FALLOW_PRODUCTION" },
                    "scope": "global",
                },
            })
        );
    }
}
