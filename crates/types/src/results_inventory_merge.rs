//! Merge inventory and analysis metadata without changing finding counts.
use super::*;

impl AnalysisResults {
    pub(super) fn merge_metadata_and_security(&mut self, parts: AnalysisResultsMetadataMergeParts) {
        crate::shared_state::merge_inventory(&mut self.shared_state, parts.shared_state);
        self.semantic_reachable_files
            .extend(parts.semantic_reachable_files);
        self.feature_flags.extend(parts.feature_flags);
        self.security_findings.extend(parts.security_findings);
        self.suppressed_security_findings
            .extend(parts.suppressed_security_findings);
        self.security_unresolved_edge_files += parts.security_unresolved_edge_files;
        self.security_unresolved_callee_sites += parts.security_unresolved_callee_sites;
        self.security_unresolved_callee_diagnostics
            .extend(parts.security_unresolved_callee_diagnostics);
        self.export_usages.extend(parts.export_usages);
        self.active_suppressions.extend(parts.active_suppressions);
        self.suppression_count += parts.suppression_count;
        self.cascade_hidden += parts.cascade_hidden;
        self.cascade
            .suppressed_unused_files
            .extend(parts.cascade.suppressed_unused_files);
        self.cascade.hidden.extend(parts.cascade.hidden);
        self.unused_component_props_exempted += parts.unused_component_props_exempted;
        if self.entry_point_summary.is_none() {
            self.entry_point_summary = parts.entry_point_summary;
        }
        if self.render_fan_in.is_none() {
            self.render_fan_in = parts.render_fan_in;
        }
        self.react_component_intel
            .extend(parts.react_component_intel);
        for contract in parts.semantic_framework_contracts {
            if !self.semantic_framework_contracts.contains(&contract) {
                self.semantic_framework_contracts.push(contract);
            }
        }
    }
}
