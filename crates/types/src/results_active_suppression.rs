//! Suppression markers present in the analyzed files.

use std::path::PathBuf;

/// A suppression comment present in an analyzed file this run.
///
/// This is the "active-suppression state" the Fallow Impact value report needs
/// to tell a genuinely resolved finding (the code was fixed) from one merely
/// silenced by a newly-added `fallow-ignore`. It captures every PRESENT marker,
/// not only the ones a detector consumed: complexity and code-duplication
/// suppressions are consumed in the CLI layer rather than the core suppression
/// context, so presence is the single uniform signal that covers all impact
/// categories. A present-but-stale marker is harmless because impact keys on a
/// suppression that newly appeared between two recorded runs. It is internal:
/// never serialized into the public JSON output schema (the field on
/// [`AnalysisResults`](crate::results::AnalysisResults) is `#[serde(skip)]`), only read in-process by
/// `fallow impact`.
#[derive(Debug, Clone)]
pub struct ActiveSuppression {
    /// Absolute path to the file carrying the suppression comment.
    pub path: PathBuf,
    /// The suppressed issue kind in kebab-case (e.g. `"unused-export"`), or
    /// `None` for a blanket marker that suppresses every kind on its target.
    pub kind: Option<String>,
    /// Whether this is a `fallow-ignore-file` (file-level) marker rather than a
    /// `fallow-ignore-next-line` marker.
    pub is_file_level: bool,
    /// Human-authored reason after `--`, when present.
    pub reason: Option<String>,
    /// 1-based line of the suppression comment itself; 0 only if unknown.
    pub comment_line: u32,
    /// The `fallow-ignore-end` line when this is a range marker pair.
    pub range_end_line: Option<u32>,
    /// What is wrong with a range marker that has no effect.
    pub range_problem: Option<crate::suppress::RangeMarkerProblemKind>,
}
