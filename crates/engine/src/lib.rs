//! Typed analysis engine facade for fallow consumers.
//!
//! `fallow-core` remains the internal orchestration backend. This crate owns
//! the typed boundary that editor, API, and embedding surfaces can depend on
//! without calling deprecated core entry points directly.

#![cfg_attr(not(test), deny(clippy::disallowed_methods))]
#![cfg_attr(
    test,
    allow(
        clippy::unwrap_used,
        clippy::expect_used,
        reason = "tests use unwrap and expect to keep fixture setup concise"
    )
)]

use std::fmt;
use std::path::{Path, PathBuf};

use fallow_config::{DuplicatesConfig, ResolvedConfig};

/// Duplication result types exposed through the engine boundary.
pub mod duplicates {
    pub use fallow_core::duplicates::*;
}

/// Extracted semantic types exposed through the engine boundary.
pub mod extract {
    pub use fallow_types::extract::*;
}

/// Analysis result types exposed through the engine boundary.
pub mod results {
    pub use fallow_core::results::*;
}

/// Suppression helpers exposed for editor and embedding surfaces.
pub mod suppress {
    pub use fallow_core::suppress::{IssueKind, is_suppressed};
}

pub use fallow_core::duplicates::{
    CloneFamily, CloneGroup, CloneInstance, DuplicationReport, DuplicationStats, MirroredDirectory,
    RefactoringSuggestion,
};
pub use fallow_types::discover::{DiscoveredFile, FileId};
pub use fallow_types::extract::ModuleInfo;
pub use fallow_types::results::AnalysisResults;

/// Result alias for typed engine operations.
pub type EngineResult<T> = Result<T, EngineError>;

/// Error type exposed by the typed engine boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EngineError {
    message: String,
}

impl EngineError {
    /// Create an engine error from a user-facing message.
    #[must_use]
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    /// User-facing error message from the backend.
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for EngineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for EngineError {}

fn engine_error(err: impl fmt::Display) -> EngineError {
    EngineError::new(err.to_string())
}

/// Resolved project config plus the config file path when one was loaded.
#[derive(Debug)]
pub struct ProjectConfig {
    pub config: ResolvedConfig,
    pub path: Option<PathBuf>,
}

/// Typed dead-code analysis result.
#[derive(Debug)]
pub struct DeadCodeAnalysis {
    pub results: AnalysisResults,
}

/// Typed dead-code analysis result with retained parser artifacts.
#[derive(Debug)]
pub struct DeadCodeAnalysisOutput {
    pub results: AnalysisResults,
    pub modules: Option<Vec<ModuleInfo>>,
    pub files: Option<Vec<DiscoveredFile>>,
}

/// Resolve the analysis config for a project.
///
/// # Errors
///
/// Returns an error when an explicit config cannot be loaded or automatic
/// config discovery finds an invalid config.
pub fn config_for_project(root: &Path, config_path: Option<&Path>) -> EngineResult<ProjectConfig> {
    fallow_core::config_for_project(root, config_path)
        .map(|(config, path)| ProjectConfig { config, path })
        .map_err(engine_error)
}

/// Run dead-code analysis on a project directory with export usage collection.
///
/// # Errors
///
/// Returns an error if config loading, file discovery, parsing, or analysis
/// fails.
pub fn analyze_project(root: &Path) -> EngineResult<DeadCodeAnalysis> {
    #[expect(
        deprecated,
        reason = "fallow-engine is the typed migration boundary over the internal core backend"
    )]
    fallow_core::analyze_project(root)
        .map(|results| DeadCodeAnalysis { results })
        .map_err(engine_error)
}

/// Run dead-code analysis with export usage collection for a resolved config.
///
/// # Errors
///
/// Returns an error if file discovery, parsing, or analysis fails.
pub fn analyze_with_usages(config: &ResolvedConfig) -> EngineResult<DeadCodeAnalysis> {
    #[expect(
        deprecated,
        reason = "fallow-engine is the typed migration boundary over the internal core backend"
    )]
    fallow_core::analyze_with_usages(config)
        .map(|results| DeadCodeAnalysis { results })
        .map_err(engine_error)
}

/// Run dead-code analysis with export usage and retained complexity artifacts.
///
/// # Errors
///
/// Returns an error if file discovery, parsing, or analysis fails.
pub fn analyze_with_usages_and_complexity(
    config: &ResolvedConfig,
) -> EngineResult<DeadCodeAnalysisOutput> {
    #[expect(
        deprecated,
        reason = "fallow-engine is the typed migration boundary over the internal core backend"
    )]
    fallow_core::analyze_with_usages_and_complexity(config)
        .map(|output| DeadCodeAnalysisOutput {
            results: output.results,
            modules: output.modules,
            files: output.files,
        })
        .map_err(engine_error)
}

/// Discover source files for a resolved config, including plugin scopes.
#[must_use]
pub fn discover_files_with_plugin_scopes(config: &ResolvedConfig) -> Vec<DiscoveredFile> {
    fallow_core::discover::discover_files_with_plugin_scopes(config)
}

/// Run duplication detection on a discovered file set.
#[must_use]
pub fn find_duplicates(
    root: &Path,
    files: &[DiscoveredFile],
    config: &DuplicatesConfig,
) -> DuplicationReport {
    fallow_core::duplicates::find_duplicates(root, files, config)
}

/// Run duplication detection on a project directory.
#[must_use]
pub fn find_duplicates_in_project(root: &Path, config: &DuplicatesConfig) -> DuplicationReport {
    fallow_core::duplicates::find_duplicates_in_project(root, config)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn engine_error_displays_message() {
        let err = EngineError::new("config failed");

        assert_eq!(err.message(), "config failed");
        assert_eq!(err.to_string(), "config failed");
    }
}
