use std::path::PathBuf;

/// Runtime code that no test dependency path reaches.
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct UntestedFile {
    /// Absolute file path.
    pub path: PathBuf,
    /// Number of value exports declared by the file.
    pub value_export_count: usize,
}

/// Runtime export that no test-reachable module references.
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct UntestedExport {
    /// Absolute file path.
    pub path: PathBuf,
    /// Export name.
    pub export_name: String,
    /// 1-based source line.
    pub line: u32,
    /// 0-based source column.
    pub col: u32,
}

/// Aggregate coverage-gap counters for the current analysis scope.
#[derive(Debug, Clone, Default, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct CoverageGapSummary {
    /// Runtime-reachable files in scope.
    pub runtime_files: usize,
    /// Runtime-reachable files also reachable from tests.
    pub covered_files: usize,
    /// Percentage of runtime files that are test-reachable.
    pub file_coverage_pct: f64,
    /// Runtime files with no test dependency path.
    pub untested_files: usize,
    /// Runtime exports with no test-reachable reference chain.
    pub untested_exports: usize,
}

/// Static test coverage gaps derived from the module graph. Shows runtime files
/// and exports with no test dependency path.
#[derive(Debug, Clone, Default, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct CoverageGaps {
    /// Summary metrics for the current analysis scope.
    pub summary: CoverageGapSummary,
    /// Runtime files with no test dependency path.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    #[cfg_attr(feature = "schema", schemars(default))]
    pub files: Vec<UntestedFile>,
    /// Runtime exports with no test-reachable reference chain.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    #[cfg_attr(feature = "schema", schemars(default))]
    pub exports: Vec<UntestedExport>,
}

impl CoverageGaps {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.files.is_empty() && self.exports.is_empty()
    }
}
