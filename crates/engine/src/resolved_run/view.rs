//! The serializable view of a resolved run.

use fallow_config::{AnalysisSelection, SkipReason, WorkspaceDiagnosticKind};
use fallow_output::{
    AnalysesView, AnalysisView, EnvView, RESOLVED_RUN_VIEW_SCHEMA_VERSION, ResolvedRunKind,
    ResolvedRunView, RunSettingsView, SelectionView,
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
        let audit_base = self.audit_base();
        ResolvedRunView {
            schema_version: RESOLVED_RUN_VIEW_SCHEMA_VERSION,
            kind: ResolvedRunKind::ResolvedRun,
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
                type_aware: self
                    .type_aware()
                    .ok()
                    .map(|type_aware| type_aware.enabled.clone()),
                audit_base: audit_base.named,
                audit_base_fallback: audit_base.fallback,
            },
            env: self.env_view(),
            fell_back: self.fallback.is_some(),
        }
    }

    /// The variables by name. A variable that the run reads but whose value
    /// it rejects is in `ignored`, as a variable that the run does not read.
    fn env_view(&self) -> EnvView {
        let rejected: Vec<&str> = self
            .notices
            .iter()
            .filter_map(|notice| match &notice.kind {
                WorkspaceDiagnosticKind::EnvValueIgnored { name } => Some(name.as_str()),
                _ => None,
            })
            .collect();
        let (ignored_values, read): (Vec<&'static str>, Vec<&'static str>) = self
            .env_read
            .iter()
            .map(|var| var.name())
            .partition(|name| rejected.contains(name));
        let mut ignored: Vec<&'static str> =
            self.env_ignored.iter().map(|var| var.name()).collect();
        ignored.extend(ignored_values);
        EnvView { read, ignored }
    }
}
