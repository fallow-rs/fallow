pub mod diff_filter;
pub(crate) mod fingerprint;
pub mod pr_comment;
pub mod review;
pub(crate) mod severity;
pub(crate) mod suggestion;

pub(crate) const TYPE_AWARE_INCOMPLETE_MESSAGE: &str =
    "Required type-aware analysis was incomplete; this result is not a clean semantic report.";

#[must_use]
pub(crate) fn required_type_aware_incomplete(
    meta: Option<&fallow_types::envelope::TypeAwareMeta>,
) -> bool {
    let Some(meta) = meta else {
        return false;
    };
    if meta.required_completeness
        != Some(fallow_types::semantic::SemanticCompletenessRequirement::Complete)
    {
        return false;
    }
    meta.identity.as_ref().is_none_or(|identity| {
        identity.completeness != fallow_types::semantic::SemanticCompleteness::Complete
    }) || meta
        .queries
        .iter()
        .any(|query| query.status != fallow_types::semantic::SemanticCompleteness::Complete)
}

#[cfg(test)]
mod tests {
    use super::*;
    use fallow_types::envelope::TypeAwareMeta;
    use fallow_types::semantic::SemanticCompletenessRequirement;

    #[test]
    fn required_type_aware_metadata_without_identity_is_incomplete() {
        let meta = TypeAwareMeta {
            required_completeness: Some(SemanticCompletenessRequirement::Complete),
            ..TypeAwareMeta::default()
        };
        assert!(required_type_aware_incomplete(Some(&meta)));
    }
}
