//! Duplication runs of an [`AnalysisSession`].
//!
//! Duplication analyzes the files of its own production mode, as `fallow
//! dupes` does. A config can give dead code and duplication different modes,
//! for example `{"production": {"deadCode": true, "dupes": false}}`. The
//! session config then holds the dead-code mode, and the session walks the
//! files of the duplication mode on the first duplication call.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use fallow_config::{DuplicatesConfig, ResolvedConfig};
use fallow_types::discover::DiscoveredFile;

use super::AnalysisSession;
use crate::project_analysis::{
    ProjectAnalysisArtifactOptions, ProjectAnalysisArtifacts, ProjectAnalysisOutput,
};
use crate::project_config::{DuplicationScope, resolve_with_env_from};
use crate::results::DuplicationAnalysis;
use crate::{EngineResult, duplicates};

/// The files that duplication analyzes for one session.
#[derive(Debug)]
pub(super) struct DuplicationFiles {
    scope: DuplicationScope,
    /// The resolved config of [`DuplicationScope::Own`]. A discovery refresh
    /// keeps it, because the config file did not change.
    config: OnceLock<ResolvedConfig>,
    /// The walked files of [`DuplicationScope::Own`].
    files: OnceLock<Vec<DiscoveredFile>>,
}

impl DuplicationFiles {
    pub(super) const fn new(scope: DuplicationScope) -> Self {
        Self {
            scope,
            config: OnceLock::new(),
            files: OnceLock::new(),
        }
    }

    /// Drop the walked files, so that the next duplication call walks the
    /// project again.
    pub(super) fn forget_files(&mut self) {
        self.files.take();
    }
}

impl AnalysisSession {
    /// The resolved config of the duplication mode, when it differs from the
    /// mode of the session config.
    pub(crate) fn own_duplication_config(&self) -> Option<&ResolvedConfig> {
        let DuplicationScope::Own {
            config,
            options,
            env,
            given,
        } = &self.duplication.scope
        else {
            return None;
        };
        Some(self.duplication.config.get_or_init(|| {
            let mut resolved =
                resolve_with_env_from((**config).clone(), &self.config.root, *options, env);
            given.apply(&mut resolved);
            resolved
        }))
    }

    /// The files of the production mode of duplication.
    fn duplication_files(&self) -> &[DiscoveredFile] {
        let Some(resolved) = self.own_duplication_config() else {
            return self.files();
        };
        self.duplication.files.get_or_init(|| {
            crate::discover::prepare_analysis_discovery_with_workspaces(
                resolved,
                &self.workspaces,
                0.0,
            )
            .into_files()
        })
    }

    /// Run dead-code and duplication analysis for this session.
    ///
    /// When `retain_complexity_artifacts` is true, the dead-code result keeps
    /// parser artifacts needed by editor overlays such as inline complexity.
    ///
    /// # Errors
    ///
    /// Returns an error if dead-code parsing or analysis fails.
    pub fn analyze_project_with(
        &self,
        duplicates_config: &DuplicatesConfig,
        retain_complexity_artifacts: bool,
    ) -> EngineResult<ProjectAnalysisOutput> {
        self.analyze_project_with_artifacts(
            duplicates_config,
            ProjectAnalysisArtifactOptions {
                retain_complexity_artifacts,
                ..ProjectAnalysisArtifactOptions::default()
            },
        )
        .map(ProjectAnalysisArtifacts::into_output)
    }

    /// Run dead-code and duplication analysis with retained session reuse data.
    ///
    /// This is the engine-owned project artifact boundary for callers that need
    /// to hand one analysis result across audit, decision, editor, or follow-up
    /// analysis surfaces without rediscovering session metadata.
    ///
    /// # Errors
    ///
    /// Returns an error if dead-code parsing or analysis fails.
    pub fn analyze_project_with_artifacts(
        &self,
        duplicates_config: &DuplicatesConfig,
        options: ProjectAnalysisArtifactOptions,
    ) -> EngineResult<ProjectAnalysisArtifacts> {
        self.ensure_not_cancelled("duplication detection")?;
        let cache_dir = (!self.config.no_cache).then_some(self.config.cache_dir.as_path());
        let duplication = if let Some(changed_files) = options.changed_files.as_ref() {
            let changed_files = changed_files.iter().cloned().collect::<Vec<_>>();
            self.find_duplicates_touching_files_with_defaults(
                duplicates_config,
                &changed_files,
                cache_dir,
            )
            .report
        } else {
            self.find_duplicates_with_defaults(duplicates_config, cache_dir)
                .report
        };
        // Duplication detection is infallible, so a token set while it ran can
        // only be reported here, before its report is handed on as a complete
        // one.
        self.ensure_not_cancelled("the dead-code half of project analysis")?;
        let source_fingerprints = options
            .collect_source_fingerprints
            .then(|| self.source_fingerprints());
        Ok(ProjectAnalysisArtifacts {
            dead_code: self.analyze_dead_code_with_artifacts(
                options.retain_complexity_artifacts,
                options.retain_graph,
            )?,
            duplication,
            changed_files: options.changed_files,
            source_fingerprints,
        })
    }

    /// Run duplication detection and return report sidecar metadata.
    #[must_use]
    pub fn find_duplicates_with_defaults(
        &self,
        config: &DuplicatesConfig,
        cache_dir: Option<&Path>,
    ) -> DuplicationAnalysis {
        #[cfg(any(test, feature = "testing"))]
        crate::resolved_run::testing::duplication_runs::record(&self.config.root);
        duplicates::find_duplicates_with_defaults(
            &self.config.root,
            self.duplication_files(),
            config,
            cache_dir,
        )
    }

    /// Run focused duplication detection for a changed-file set.
    #[must_use]
    pub fn find_duplicates_touching_files_with_defaults(
        &self,
        config: &DuplicatesConfig,
        changed_files: &[PathBuf],
        cache_dir: Option<&Path>,
    ) -> DuplicationAnalysis {
        #[cfg(any(test, feature = "testing"))]
        crate::resolved_run::testing::duplication_runs::record(&self.config.root);
        duplicates::find_duplicates_touching_files_with_defaults(
            &self.config.root,
            self.duplication_files(),
            config,
            changed_files,
            cache_dir,
        )
    }
}
