//! Architecture boundary findings.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::serde_path;

/// An import that crosses an architecture boundary rule.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct BoundaryViolation {
    /// The file making the disallowed import.
    #[serde(serialize_with = "serde_path::serialize")]
    pub from_path: PathBuf,
    /// The imported file that violates the boundary. Through a re-export chain,
    /// the origin module that declares the symbol, not the barrel; a chain
    /// stops at the first public entry file (`publicFiles`), which it then names.
    #[serde(serialize_with = "serde_path::serialize")]
    pub to_path: PathBuf,
    /// The zone the importing file belongs to.
    pub from_zone: String,
    /// The zone the imported file belongs to.
    pub to_zone: String,
    /// The project-root-relative path of the imported file, as the analysis
    /// resolved it (`src/services/orders.ts`). It is not the specifier text
    /// of the import statement (`../services/orders`).
    pub import_specifier: String,
    /// 1-based line number of the import statement in the source file.
    pub line: u32,
    /// 0-based byte column offset of the import statement.
    pub col: u32,
    /// The barrel file that the source file imports directly, when the
    /// violation comes from a re-export chain. Absent for a direct import.
    #[serde(
        default,
        serialize_with = "serde_path::serialize_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub via_path: Option<PathBuf>,
    /// Why the import breaks the boundary rules. Omitted for
    /// `disallowed_zone`, so a project without `publicFiles` keeps its output.
    #[serde(
        default,
        skip_serializing_if = "BoundaryViolationReason::is_disallowed_zone"
    )]
    pub reason: BoundaryViolationReason,
    /// The public entry files of `to_zone` that the importer can use instead.
    /// Set only when `reason` is `internal_file`.
    #[serde(
        default,
        serialize_with = "serde_path::serialize_vec",
        skip_serializing_if = "Vec::is_empty"
    )]
    pub public_files: Vec<PathBuf>,
}

/// The rule that a boundary violation breaks.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum BoundaryViolationReason {
    /// No rule of `from_zone` allows an import from `to_zone`.
    #[default]
    DisallowedZone,
    /// A rule allows the zone, but the imported file is not a public entry
    /// file (`publicFiles`) of `to_zone`.
    InternalFile,
}

/// The number of public entry files that a finding message names.
const SHOWN_PUBLIC_API_PATHS: usize = 3;

impl BoundaryViolationReason {
    /// Whether this is the default reason, which the output omits.
    #[must_use]
    pub const fn is_disallowed_zone(&self) -> bool {
        matches!(self, Self::DisallowedZone)
    }
}

impl BoundaryViolation {
    /// The message of an [`BoundaryViolationReason::InternalFile`] finding,
    /// with `display` to render each path. `None` for other reasons.
    #[must_use]
    pub fn internal_file_message(
        &self,
        display: &dyn Fn(&std::path::Path) -> String,
    ) -> Option<String> {
        if self.reason != BoundaryViolationReason::InternalFile {
            return None;
        }
        let target = display(&self.to_path);
        let zone = &self.to_zone;
        if self.public_files.is_empty() {
            return Some(format!(
                "{target} is not a public entry file of zone '{zone}', and no file of the zone \
                 matches its publicFiles globs"
            ));
        }
        let mut shown: Vec<String> = self
            .public_files
            .iter()
            .take(SHOWN_PUBLIC_API_PATHS)
            .map(|path| display(path))
            .collect();
        let hidden = self
            .public_files
            .len()
            .saturating_sub(SHOWN_PUBLIC_API_PATHS);
        if hidden > 0 {
            shown.push(format!("{hidden} more"));
        }
        let through = if self.public_files.len() == 1 {
            shown.join("")
        } else {
            format!("one of {}", shown.join(", "))
        };
        Some(format!(
            "{target} is not a public entry file of zone '{zone}'; import it through {through}"
        ))
    }

    /// The description and note of the refactor action of this finding.
    #[must_use]
    pub const fn refactor_action_text(&self) -> (&'static str, &'static str) {
        match self.reason {
            BoundaryViolationReason::DisallowedZone => (
                "Move the import through an allowed zone or restructure the dependency",
                "This import crosses an architecture boundary that is not permitted by the configured rules",
            ),
            BoundaryViolationReason::InternalFile => (
                "Import the symbol through a public entry file of the target zone (public_files)",
                "The target zone declares publicFiles, and the imported file is not one of its public entry files",
            ),
        }
    }
}
