use std::path::{Path, PathBuf};

use fallow_types::output_health::{
    UntestedExportAction, UntestedExportActionType, UntestedFileAction, UntestedFileActionType,
};

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

/// Wire-shape envelope for an [`UntestedFile`] finding. Carries the bare
/// [`UntestedFile`] flattened in plus a typed `actions` array. The action
/// vec is computed at construction time using a project-root-relative path
/// so descriptions match `strip_root_prefix`'s post-pass output on the inner
/// `path` field. Schemars derives the merged shape natively; this retires
/// the `augment_finding_definition` graft for `UntestedFile`.
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct UntestedFileFinding {
    /// The underlying coverage-gap entry.
    #[serde(flatten)]
    pub file: UntestedFile,
    /// Suggested next steps: an `add-tests` primary and a `suppress-file`
    /// secondary. Always emitted (possibly empty for forward-compat).
    pub actions: Vec<UntestedFileAction>,
}

impl UntestedFileFinding {
    /// Build the wrapper from a raw [`UntestedFile`] and the project root.
    /// `root` is used to compute the relative path string embedded in action
    /// descriptions; the inner `file.path` stays absolute and is converted to
    /// the wire form by `strip_root_prefix` later in the JSON pipeline.
    #[must_use]
    pub fn with_actions(file: UntestedFile, root: &Path) -> Self {
        let display_path = relative_display(&file.path, root);
        let actions = vec![
            UntestedFileAction {
                kind: UntestedFileActionType::AddTests,
                auto_fixable: false,
                description: format!("Add test coverage for `{display_path}`"),
                note: Some("No test dependency path reaches this runtime file".to_string()),
                comment: None,
            },
            UntestedFileAction {
                kind: UntestedFileActionType::SuppressFile,
                auto_fixable: false,
                description: format!("Suppress coverage gap reporting for `{display_path}`"),
                note: None,
                comment: Some("// fallow-ignore-file coverage-gaps".to_string()),
            },
        ];
        Self { file, actions }
    }
}

/// Wire-shape envelope for an [`UntestedExport`] finding. Same pattern as
/// [`UntestedFileFinding`]: flattens the bare finding and carries a typed
/// `actions` array computed at construction time.
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct UntestedExportFinding {
    /// The underlying coverage-gap entry.
    #[serde(flatten)]
    pub export: UntestedExport,
    /// Suggested next steps: an `add-test-import` primary and a
    /// `suppress-file` secondary.
    pub actions: Vec<UntestedExportAction>,
}

impl UntestedExportFinding {
    /// Build the wrapper from a raw [`UntestedExport`] and the project root.
    #[must_use]
    pub fn with_actions(export: UntestedExport, root: &Path) -> Self {
        let display_path = relative_display(&export.path, root);
        let export_name = export.export_name.clone();
        let actions = vec![
            UntestedExportAction {
                kind: UntestedExportActionType::AddTestImport,
                auto_fixable: false,
                description: format!("Import and test `{export_name}` from `{display_path}`"),
                note: Some(
                    "This export is runtime-reachable but no test-reachable module references it"
                        .to_string(),
                ),
                comment: None,
            },
            UntestedExportAction {
                kind: UntestedExportActionType::SuppressFile,
                auto_fixable: false,
                description: format!("Suppress coverage gap reporting for `{display_path}`"),
                note: None,
                comment: Some("// fallow-ignore-file coverage-gaps".to_string()),
            },
        ];
        Self { export, actions }
    }
}

fn relative_display(path: &Path, root: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .display()
        .to_string()
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
    /// Runtime files with no test dependency path. Each entry carries its
    /// own `actions` array via [`UntestedFileFinding`].
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[cfg_attr(feature = "schema", schemars(default))]
    pub files: Vec<UntestedFileFinding>,
    /// Runtime exports with no test-reachable reference chain. Each entry
    /// carries its own `actions` array via [`UntestedExportFinding`].
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[cfg_attr(feature = "schema", schemars(default))]
    pub exports: Vec<UntestedExportFinding>,
}

impl CoverageGaps {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.files.is_empty() && self.exports.is_empty()
    }
}
