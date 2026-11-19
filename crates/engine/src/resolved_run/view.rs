//! The serializable view of a resolved run.

use fallow_config::{AnalysisSelection, SkipReason};
use fallow_output::{
    AnalysesView, AnalysisView, EnvView, RESOLVED_RUN_VIEW_SCHEMA_VERSION, ResolvedRunView,
    RunSettingsView, SelectionView,
};
use fallow_types::sourced::Sourced;

use super::{AnalysisDecision, ResolvedRun};

const fn selection_view(selection: AnalysisSelection) -> SelectionView {
    match selection {
        AnalysisSelection::Run => SelectionView::Run,
        AnalysisSelection::Skipped(SkipReason::NotRequested) => SelectionView::NotRequested,
        AnalysisSelection::Skipped(SkipReason::Config(_)) => SelectionView::OffByConfig,
    }
}

fn analysis_view(decision: &AnalysisDecision) -> AnalysisView {
    AnalysisView {
        selection: decision.selection.clone().map(selection_view),
        production: decision.production.clone(),
    }
}

fn cloned<T: Clone>(value: &Sourced<T>) -> Sourced<T> {
    value.clone()
}

impl ResolvedRun {
    /// The view of every decision of the run, with its source. The
    /// environment block names the variables and never holds a value.
    pub fn view(&self) -> ResolvedRunView {
        let settings = &self.settings;
        ResolvedRunView {
            schema_version: RESOLVED_RUN_VIEW_SCHEMA_VERSION,
            surface: self.surface,
            command: self.command,
            root: cloned(&self.root),
            config_file: cloned(&self.config_file),
            analyses: AnalysesView {
                dead_code: analysis_view(&self.dead_code),
                duplication: analysis_view(&self.duplication),
                health: analysis_view(&self.health),
            },
            settings: RunSettingsView {
                threads: cloned(&settings.threads),
                no_cache: cloned(&settings.no_cache),
                allow_remote_extends: cloned(&settings.allow_remote_extends),
                max_file_size_bytes: cloned(&settings.max_file_size_bytes),
                cache_dir: cloned(&settings.cache_dir),
                cache_max_size_mb: cloned(&settings.cache_max_size_mb),
                package_baselines: cloned(&settings.package_baselines),
                include_entry_exports: cloned(&settings.include_entry_exports),
            },
            env: EnvView {
                read: self.env_read.iter().map(|var| var.name()).collect(),
                ignored: self.env_ignored.iter().map(|var| var.name()).collect(),
            },
            fell_back: self.fell_back,
        }
    }
}
