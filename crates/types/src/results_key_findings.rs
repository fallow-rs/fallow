//! Per-key finding check over the counted result arrays.
//!
//! `results.rs` declares this module after its field macros, which it uses.

use super::AnalysisResults;

macro_rules! counted_result_key_has_findings {
    ($input:expr, $($field:ident => $key:literal,)+) => {{
        let (results, key): (&AnalysisResults, &str) = $input;
        match key {
            $($key => Some(!results.$field.is_empty()),)+
            "prop_drilling_chains" => Some(!results.prop_drilling_chains.is_empty()),
            "thin_wrappers" => Some(!results.thin_wrappers.is_empty()),
            "duplicate_prop_shapes" => Some(!results.duplicate_prop_shapes.is_empty()),
            _ => None,
        }
    }};
}

impl AnalysisResults {
    /// Whether the serialized result array `key` holds at least one entry.
    ///
    /// `key` is a name from [`super::TOTAL_ISSUE_RESULT_KEYS`]. Returns `None` for
    /// any other name. Advisory entries count, also when they do not count
    /// toward [`Self::total_issues`].
    #[must_use]
    pub fn result_key_has_findings(&self, key: &str) -> Option<bool> {
        counted_analysis_result_fields!(counted_result_key_has_findings, (self, key))
    }
}
