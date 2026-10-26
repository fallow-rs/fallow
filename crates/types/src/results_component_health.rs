use super::AnalysisResults;

macro_rules! enforced_component_count {
    ($findings:expr) => {{
        let findings = $findings;
        let mut count = 0;
        let mut index = 0;
        while index < findings.len() {
            if matches!(
                findings.as_slice()[index].effective_severity,
                Some(crate::output_dead_code::EffectiveSeverity::Error)
            ) {
                count += 1;
            }
            index += 1;
        }
        count
    }};
}

/// Component health arrays that contribute to totals only at explicit error severity.
pub const ERROR_ONLY_RESULT_KEYS: &[&str] = &[
    "prop_drilling_chains",
    "thin_wrappers",
    "duplicate_prop_shapes",
];

impl AnalysisResults {
    /// Prop-drilling chains that explicitly enforce the error policy.
    /// Advisory warnings stay outside totals used by strict CI count gates.
    #[must_use]
    pub const fn enforced_prop_drilling_count(&self) -> usize {
        enforced_component_count!(&self.prop_drilling_chains)
    }

    /// Thin wrappers whose saved severity explicitly enforces error policy.
    #[must_use]
    pub const fn enforced_thin_wrapper_count(&self) -> usize {
        enforced_component_count!(&self.thin_wrappers)
    }

    /// Duplicate prop shapes whose saved severity explicitly enforces error policy.
    #[must_use]
    pub const fn enforced_duplicate_prop_shape_count(&self) -> usize {
        enforced_component_count!(&self.duplicate_prop_shapes)
    }
}
