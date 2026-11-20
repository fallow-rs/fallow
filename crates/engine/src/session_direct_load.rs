//! The session constructors that load or take a config without a resolved
//! run. Tests and benchmarks use them (feature `testing`). A release build
//! opens each session from a resolved run
//! ([`crate::resolved_run::Runnable::open_session`]).

use std::path::Path;

use fallow_config::ResolvedConfig;

use super::AnalysisSession;
use crate::EngineResult;
use crate::project_config::{ProjectConfig, config_for_project, default_project_config};

impl AnalysisSession {
    /// Load config and discover files for a project root.
    ///
    /// # Errors
    ///
    /// Returns an error when config loading fails.
    pub fn load(root: &Path, config_path: Option<&Path>) -> EngineResult<Self> {
        let project_config = config_for_project(root, config_path)?;
        Ok(Self::from_project_config(project_config))
    }

    /// Load config, apply one caller-supplied config adjustment, then discover
    /// files for a project root.
    ///
    /// # Errors
    ///
    /// Returns an error when config loading fails.
    pub fn load_with_config(
        root: &Path,
        config_path: Option<&Path>,
        configure: impl FnOnce(&mut ResolvedConfig),
    ) -> EngineResult<Self> {
        Self::load_with_config_options(
            root,
            config_path,
            fallow_config::ConfigLoadOptions::default(),
            configure,
        )
    }

    /// Load config with an explicit inheritance trust policy, apply one
    /// caller-supplied adjustment, then discover project files.
    ///
    /// When duplication has another production mode than dead code, the
    /// session walks the files of duplication from the loaded config. The
    /// adjustment does not reach that walk, so it must not change the root,
    /// the ignore patterns or the production mode.
    ///
    /// # Errors
    ///
    /// Returns an error when config loading fails.
    pub fn load_with_config_options(
        root: &Path,
        config_path: Option<&Path>,
        load_options: fallow_config::ConfigLoadOptions,
        configure: impl FnOnce(&mut ResolvedConfig),
    ) -> EngineResult<Self> {
        let mut project_config = crate::project_config::config_for_project_with_load_options(
            root,
            config_path,
            load_options,
        )?;
        configure(&mut project_config.config);
        Ok(Self::from_project_config(
            project_config.without_workspace_metadata(),
        ))
    }

    /// Build a session from built-in defaults, ignoring project config files.
    #[must_use]
    pub fn load_default(root: &Path) -> Self {
        Self::load_default_with_config(root, |_| {})
    }

    /// Build a session from built-in defaults, apply one caller-supplied
    /// adjustment, then discover project files.
    #[must_use]
    pub fn load_default_with_config(
        root: &Path,
        configure: impl FnOnce(&mut ResolvedConfig),
    ) -> Self {
        let mut project_config = default_project_config(root);
        configure(&mut project_config.config);
        Self::from_project_config(project_config)
    }

    /// Build a session from a project config of a direct load.
    #[must_use]
    pub fn from_config(project_config: ProjectConfig) -> Self {
        Self::from_project_config(project_config)
    }
}
