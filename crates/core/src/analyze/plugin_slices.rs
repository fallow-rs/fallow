//! Borrowed string views of the plugin result that the detectors read.

/// Derive the borrowed plugin string slices (virtual module prefixes, generated
/// import patterns, generated type-import prefixes) consumed by the detectors.
pub fn derive_plugin_string_slices(
    plugin_result: Option<&crate::plugins::AggregatedPluginResult>,
) -> (Vec<&str>, Vec<&str>, Vec<&str>) {
    let virtual_prefixes = plugin_result
        .map(|pr| {
            pr.virtual_module_prefixes
                .iter()
                .map(String::as_str)
                .collect()
        })
        .unwrap_or_default();
    let generated_patterns = plugin_result
        .map(|pr| {
            pr.generated_import_patterns
                .iter()
                .map(String::as_str)
                .collect()
        })
        .unwrap_or_default();
    let generated_type_prefixes = plugin_result
        .map(|pr| {
            pr.generated_type_import_prefixes
                .iter()
                .map(String::as_str)
                .collect()
        })
        .unwrap_or_default();
    (
        virtual_prefixes,
        generated_patterns,
        generated_type_prefixes,
    )
}
