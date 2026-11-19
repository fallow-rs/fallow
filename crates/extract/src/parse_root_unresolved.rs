//! Names that the top-level scope of a program references but does not declare.

/// Split the names that no binding of the program declares into value-position
/// names (the auto-import candidates) and type-position names. Both lists are
/// sorted and free of duplicates.
pub(super) fn compute_root_unresolved_names(
    scoping: &oxc_semantic::Scoping,
) -> (Vec<String>, Vec<String>) {
    let mut values = Vec::new();
    let mut types = Vec::new();
    for (name, reference_ids) in scoping.root_unresolved_references() {
        let references = || {
            reference_ids
                .iter()
                .map(|reference_id| scoping.get_reference(*reference_id))
        };
        if references().any(oxc_semantic::Reference::is_value) {
            values.push(name.as_str().to_string());
        }
        if references().any(oxc_semantic::Reference::is_type) {
            types.push(name.as_str().to_string());
        }
    }
    values.sort_unstable();
    types.sort_unstable();
    (values, types)
}
