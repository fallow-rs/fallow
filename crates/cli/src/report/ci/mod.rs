pub mod diff_filter;
pub(crate) mod fingerprint;
pub mod pr_comment;
pub mod review;
pub(crate) mod severity;
pub(crate) mod suggestion;

pub(crate) const TYPE_AWARE_INCOMPLETE_MESSAGE: &str =
    "Required type-aware analysis was incomplete; this result is not a clean semantic report.";

const SAVED_TYPE_AWARE_META_POINTERS: [&str; 4] = [
    "/_meta/type_aware",
    "/_meta/check/type_aware",
    "/check/_meta/type_aware",
    "/dead_code/_meta/type_aware",
];

pub(crate) fn saved_type_aware_metadata(
    envelope: &serde_json::Value,
) -> Result<Vec<fallow_types::envelope::TypeAwareMeta>, String> {
    let mut metadata = Vec::new();
    for pointer in SAVED_TYPE_AWARE_META_POINTERS {
        let Some(value) = envelope.pointer(pointer).filter(|value| !value.is_null()) else {
            continue;
        };
        let meta = serde_json::from_value(value.clone()).map_err(|error| {
            format!(
                "saved type-aware metadata at `{pointer}` is incompatible with this Fallow version: {error}"
            )
        })?;
        metadata.push(meta);
    }
    Ok(metadata)
}

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
