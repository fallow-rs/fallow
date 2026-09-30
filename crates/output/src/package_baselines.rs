//! Applied package Git baselines shared by JSON reports and editor notifications.

use serde::{Deserialize, Serialize};

/// One applied baseline for an exact, project-relative workspace root.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct PackageBaselineStatus {
    /// Workspace package root, relative to the analysis root with `/` separators.
    pub workspace_root: String,
    /// Git ref used to select changed files in this package.
    pub reference: String,
}
