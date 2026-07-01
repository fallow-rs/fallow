//! Project-level analysis contracts owned by the engine boundary.

use std::path::PathBuf;

use rustc_hash::FxHashSet;

pub use crate::public_api::public_api_package_entry_points;
pub use crate::results::{ProjectAnalysisArtifacts, ProjectAnalysisOutput};

/// Artifact retention options for one project-level analysis run.
#[derive(Debug, Default)]
pub struct ProjectAnalysisArtifactOptions {
    /// Keep parser artifacts needed by inline complexity and health overlays.
    pub retain_complexity_artifacts: bool,
    /// Keep the module graph for trace, routing, and decision-surface facts.
    pub retain_graph: bool,
    /// Changed files already resolved by the caller for this command run.
    pub changed_files: Option<FxHashSet<PathBuf>>,
    /// Collect source metadata fingerprints for follow-up cache decisions.
    pub collect_source_fingerprints: bool,
}
