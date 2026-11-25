//! The selection and the activation of the issue types that `--<issue-type>`
//! flags request.

use super::*;

impl IssueFilters {
    pub fn enable_cli_filter_flag(&mut self, flag: &str) -> bool {
        match flag {
            "--unused-files" => self.unused_files = true,
            "--unused-exports" => self.unused_exports = true,
            "--unused-deps" => self.unused_deps = true,
            "--unused-types" => self.unused_types = true,
            "--private-type-leaks" => self.private_type_leaks = true,
            "--deprecated-exports-in-use" => self.deprecated_exports_in_use = true,
            "--unused-enum-members" => self.unused_enum_members = true,
            "--unused-class-members" => self.unused_class_members = true,
            "--unused-namespace-members" => self.unused_namespace_members = true,
            "--unused-protected-class-members" => self.unused_protected_class_members = true,
            "--namespace-escapes" => self.namespace_escapes = true,
            "--test-only-exports" => self.test_only_exports = true,
            "--unused-store-members" => self.unused_store_members = true,
            "--unprovided-injects" => self.unprovided_injects = true,
            "--unrendered-components" => self.unrendered_components = true,
            "--unused-component-props" => self.unused_component_props = true,
            "--absent-component-props" => self.absent_component_props = true,
            "--constant-parameters" => self.constant_parameters = true,
            "--unused-component-emits" => self.unused_component_emits = true,
            "--unused-component-inputs" => self.unused_component_inputs = true,
            "--unused-component-outputs" => self.unused_component_outputs = true,
            "--unused-svelte-events" => self.unused_svelte_events = true,
            "--unused-effect-layers" => self.unused_effect_layers = true,
            "--unused-tagged-errors" => self.unused_tagged_errors = true,
            "--unproduced-catch-tags" => self.unproduced_catch_tags = true,
            "--unused-server-actions" => self.unused_server_actions = true,
            "--unused-load-data-keys" => self.unused_load_data_keys = true,
            "--unresolved-imports" => self.unresolved_imports = true,
            "--unlisted-deps" => self.unlisted_deps = true,
            "--unlisted-binaries" => self.unlisted_binaries = true,
            "--duplicate-exports" => self.duplicate_exports = true,
            "--duplicate-export-aliases" => self.duplicate_export_aliases = true,
            "--circular-deps" => self.circular_deps = true,
            "--re-export-cycles" => self.re_export_cycles = true,
            "--package-cycles" => self.package_cycles = true,
            "--boundary-violations" => self.boundary_violations = true,
            "--policy-violations" => self.policy_violations = true,
            "--stale-suppressions" => self.stale_suppressions = true,
            "--unused-catalog-entries" => self.unused_catalog_entries = true,
            "--empty-catalog-groups" => self.empty_catalog_groups = true,
            "--unresolved-catalog-references" => self.unresolved_catalog_references = true,
            "--unused-dependency-overrides" => self.unused_dependency_overrides = true,
            "--misconfigured-dependency-overrides" => {
                self.misconfigured_dependency_overrides = true;
            }
            _ => return false,
        }
        true
    }

    /// Whether the report keeps dependency findings: no filter is active, or
    /// `--unused-deps` or `--unlisted-deps` is one of the active filters, and
    /// the run does not report only the architecture findings.
    ///
    /// These are the issue types `ignoreDependencies` controls, so an
    /// unmatched `ignoreDependencies` glob is reported only when this is true.
    pub fn reports_dependency_findings(&self) -> bool {
        self.architecture_selection != ArchitectureSelection::Only
            && (!self.any_active() || self.unused_deps || self.unlisted_deps)
    }

    /// Enable off-by-default issue types when explicitly requested as filters.
    pub fn activate_explicit_opt_ins(&self, rules: &mut RulesConfig) {
        if self.absent_component_props && rules.absent_component_props == Severity::Off {
            rules.absent_component_props = Severity::Warn;
        }
        if self.constant_parameters && rules.constant_parameters == Severity::Off {
            rules.constant_parameters = Severity::Warn;
        }
        if self.private_type_leaks && rules.private_type_leaks == Severity::Off {
            rules.private_type_leaks = Severity::Warn;
        }
        if self.deprecated_exports_in_use && rules.deprecated_exports_in_use == Severity::Off {
            rules.deprecated_exports_in_use = Severity::Warn;
        }
        if self.unused_protected_class_members
            && rules.unused_protected_class_members == Severity::Off
        {
            rules.unused_protected_class_members = Severity::Warn;
        }
        if self.namespace_escapes && rules.namespace_escapes == Severity::Off {
            rules.namespace_escapes = Severity::Warn;
        }
        if self.test_only_exports && rules.test_only_exports == Severity::Off {
            rules.test_only_exports = Severity::Warn;
        }
    }

    /// When any filter is active, clear issue types that were NOT requested.
    pub fn apply(&self, results: &mut fallow_types::results::AnalysisResults) {
        if !self.any_active() {
            return;
        }
        self.apply_core_filters(results);
        self.apply_component_filters(results);
        self.apply_graph_filters(results);
        self.apply_policy_filters(results);
        self.apply_catalog_filters(results);
        if !self.constant_parameters {
            results.constant_parameters.clear();
        }
    }

    fn apply_core_filters(&self, results: &mut fallow_types::results::AnalysisResults) {
        if !self.unused_files {
            results.unused_files.clear();
            results.cascade.suppressed_unused_files.clear();
        }
        if !self.unused_exports {
            results.unused_exports.clear();
        }
        if !self.unused_types {
            results.unused_types.clear();
        }
        if !self.private_type_leaks {
            results.private_type_leaks.clear();
        }
        if !self.deprecated_exports_in_use {
            results.deprecated_exports_in_use.clear();
        }
        if !self.unused_deps {
            results.unused_dependencies.clear();
            results.unused_dev_dependencies.clear();
            results.unused_optional_dependencies.clear();
            results.type_only_dependencies.clear();
            results.test_only_dependencies.clear();
            results.dev_dependencies_in_production.clear();
        }
        if !self.unused_enum_members {
            results.unused_enum_members.clear();
        }
        if !self.unused_class_members {
            results.unused_class_members.clear();
        }
        if !self.unused_store_members {
            results.unused_store_members.clear();
        }
        if !self.unused_namespace_members {
            results.unused_namespace_members.clear();
        }
        if !self.unused_protected_class_members {
            results.unused_protected_class_members.clear();
        }
        if !self.namespace_escapes {
            results.namespace_escapes.clear();
        }
        if !self.test_only_exports {
            results.test_only_exports.clear();
        }
        if !self.unlisted_deps {
            results.unlisted_dependencies.clear();
        }
        if !self.unlisted_binaries {
            results.unlisted_binaries.clear();
        }
    }

    fn apply_component_filters(&self, results: &mut fallow_types::results::AnalysisResults) {
        if !self.unprovided_injects {
            results.unprovided_injects.clear();
        }
        if !self.unrendered_components {
            results.unrendered_components.clear();
        }
        if !self.absent_component_props {
            results.absent_component_props.clear();
        }
        if !self.unused_component_props {
            results.unused_component_props.clear();
        }
        if !self.unused_component_emits {
            results.unused_component_emits.clear();
        }
        if !self.unused_component_inputs {
            results.unused_component_inputs.clear();
        }
        if !self.unused_component_outputs {
            results.unused_component_outputs.clear();
        }
        if !self.unused_svelte_events {
            results.unused_svelte_events.clear();
        }
        if !self.unused_effect_layers {
            results.unused_effect_layers.clear();
        }
        if !self.unused_tagged_errors {
            results.unused_tagged_errors.clear();
        }
        if !self.unproduced_catch_tags {
            results.unproduced_catch_tags.clear();
        }
        if !self.unused_server_actions {
            results.unused_server_actions.clear();
        }
        if !self.unused_load_data_keys {
            results.unused_load_data_keys.clear();
        }
        if !self.unresolved_imports {
            results.unresolved_imports.clear();
        }
        if !self.invalid_client_exports {
            results.invalid_client_exports.clear();
        }
        if !self.mixed_client_server_barrels {
            results.mixed_client_server_barrels.clear();
        }
        if !self.misplaced_directives {
            results.misplaced_directives.clear();
        }
        if !self.route_collisions {
            results.route_collisions.clear();
        }
        if !self.dynamic_segment_name_conflicts {
            results.dynamic_segment_name_conflicts.clear();
        }
    }

    fn apply_graph_filters(&self, results: &mut fallow_types::results::AnalysisResults) {
        if !self.duplicate_exports {
            results.duplicate_exports.clear();
        }
        if !self.duplicate_export_aliases {
            results.duplicate_export_aliases.clear();
        }
        if !self.circular_deps {
            results.circular_dependencies.clear();
        }
        if !self.re_export_cycles {
            results.re_export_cycles.clear();
        }
        if !self.package_cycles {
            results.package_cycles.clear();
        }
        if !self.boundary_violations {
            results.boundary_violations.clear();
            results.boundary_coverage_violations.clear();
            results.boundary_call_violations.clear();
        }
    }

    fn apply_policy_filters(&self, results: &mut fallow_types::results::AnalysisResults) {
        if !self.policy_violations {
            results.policy_violations.clear();
        }
        if !self.stale_suppressions {
            results.stale_suppressions.clear();
        }
    }

    fn apply_catalog_filters(&self, results: &mut fallow_types::results::AnalysisResults) {
        if !self.unused_catalog_entries {
            results.unused_catalog_entries.clear();
        }
        if !self.empty_catalog_groups {
            results.empty_catalog_groups.clear();
        }
        if !self.unresolved_catalog_references {
            results.unresolved_catalog_references.clear();
        }
        if !self.unused_dependency_overrides {
            results.unused_dependency_overrides.clear();
        }
        if !self.misconfigured_dependency_overrides {
            results.misconfigured_dependency_overrides.clear();
        }
    }
}
