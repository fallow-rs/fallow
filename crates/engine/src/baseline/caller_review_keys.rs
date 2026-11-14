//! Baseline keys of the findings that list the inspected callers.

use super::*;

pub(super) fn absent_component_prop_baseline_keys(
    items: &[crate::results::AbsentComponentPropFinding],
    root: &Path,
) -> Vec<String> {
    items
        .iter()
        .map(|finding| {
            format!(
                "{}:{}:{}",
                relative_path(&finding.prop.path, root),
                finding.prop.component_name,
                finding.prop.prop_name
            )
        })
        .collect()
}

pub(super) fn constant_parameter_baseline_keys(
    items: &[crate::results::ConstantParameterFinding],
    root: &Path,
) -> Vec<String> {
    items
        .iter()
        .map(|finding| {
            format!(
                "{}:{}:{}",
                relative_path(&finding.parameter.path, root),
                finding.parameter.function_name,
                finding.parameter.parameter_name
            )
        })
        .collect()
}
