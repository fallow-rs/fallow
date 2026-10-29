use fallow_output::{CodeClimateIssueInput, CodeClimateSeverity, codeclimate_fingerprint_hash};
use fallow_types::output_dead_code::EffectiveSeverity;

use super::{CodeClimateBuilder, cc_path, dead_code_issue};

impl CodeClimateBuilder<'_> {
    pub(super) fn push_prop_drilling_issues(&mut self) {
        for finding in &self.results.prop_drilling_chains {
            if finding.effective_severity != Some(EffectiveSeverity::Error) {
                continue;
            }
            let Some(source) = finding.chain.hops.first() else {
                continue;
            };
            let path = cc_path(&source.file, self.root);
            let fingerprint = codeclimate_fingerprint_hash(&[
                "fallow/prop-drilling",
                &path,
                &source.component,
                &finding.chain.prop,
            ]);
            self.issues.push(dead_code_issue(
                finding.finding_id.as_deref(),
                &[],
                CodeClimateIssueInput {
                    check_name: "fallow/prop-drilling",
                    description: &format!("Prop '{}' is forwarded unchanged through {} components. Review composition, colocation or context, or suppress the source prop declaration.", finding.chain.prop, finding.chain.depth),
                    severity: CodeClimateSeverity::Major,
                    category: "Complexity",
                    path: &path,
                    begin_line: Some(source.line),
                    fingerprint: &fingerprint,
                },
            ));
        }
    }

    pub(super) fn push_component_shape_issues(&mut self) {
        let wrappers = self.results.thin_wrappers.iter()
            .filter(|finding| finding.effective_severity == Some(EffectiveSeverity::Error))
            .map(|finding| (finding.finding_id.as_deref(), "fallow/thin-wrapper", &finding.wrapper.file,
                finding.wrapper.line, finding.wrapper.component.as_str(), "Complexity",
                format!("Component '{}' only forwards its props to '{}'. Consider inlining it at its call sites.", finding.wrapper.component, finding.wrapper.child_component)));
        let shapes = self.results.duplicate_prop_shapes.iter()
            .filter(|finding| finding.effective_severity == Some(EffectiveSeverity::Error))
            .map(|finding| (finding.finding_id.as_deref(), "fallow/duplicate-prop-shape", &finding.shape.file,
                finding.shape.line, finding.shape.component.as_str(), "Duplication",
                format!("Component '{}' shares its prop shape with {} components. Consider a shared Props type.", finding.shape.component, finding.shape.group_size.saturating_sub(1))));
        for (finding_id, code, file, line, component, category, description) in
            wrappers.chain(shapes)
        {
            let path = cc_path(file, self.root);
            let fingerprint = codeclimate_fingerprint_hash(&[code, &path, component]);
            self.issues.push(dead_code_issue(
                finding_id,
                &[],
                CodeClimateIssueInput {
                    check_name: code,
                    description: &description,
                    severity: CodeClimateSeverity::Major,
                    category,
                    path: &path,
                    begin_line: Some(line),
                    fingerprint: &fingerprint,
                },
            ));
        }
    }
}
