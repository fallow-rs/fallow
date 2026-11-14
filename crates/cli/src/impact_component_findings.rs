//! Impact records of component contract findings.

use super::*;

/// Push unrendered-component and component prop/emit/input/output findings.
pub(super) fn collect_component_findings(
    results: &AnalysisResults,
    push: &mut impl FnMut(&Path, &'static str, Option<String>),
) {
    for f in &results.unrendered_components {
        push(
            &f.component.path,
            "unrendered-component",
            Some(f.component.component_name.clone()),
        );
    }
    for finding in &results.constant_parameters {
        push(
            &finding.parameter.path,
            "constant-parameter",
            Some(format!(
                "{}({})",
                finding.parameter.function_name,
                finding.parameter.label()
            )),
        );
    }
    for finding in &results.absent_component_props {
        push(
            &finding.prop.path,
            "absent-component-prop",
            Some(format!(
                "{}.{}",
                finding.prop.component_name, finding.prop.prop_name
            )),
        );
    }
    for f in &results.unused_component_props {
        push(
            &f.prop.path,
            "unused-component-prop",
            Some(f.prop.prop_name.clone()),
        );
    }
    for f in &results.unused_component_emits {
        push(
            &f.emit.path,
            "unused-component-emit",
            Some(f.emit.emit_name.clone()),
        );
    }
    for f in &results.unused_component_inputs {
        push(
            &f.input.path,
            "unused-component-input",
            Some(f.input.input_name.clone()),
        );
    }
    for f in &results.unused_component_outputs {
        push(
            &f.output.path,
            "unused-component-output",
            Some(f.output.output_name.clone()),
        );
    }
}
