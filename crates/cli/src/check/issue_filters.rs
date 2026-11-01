//! The issue-type selection of a dead-code run.

use fallow_config::{RulesConfig, Severity};

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct IssueFilters {
    pub unused_files: bool,
    pub unused_exports: bool,
    pub unused_deps: bool,
    pub misplaced_deps: bool,
    pub unused_types: bool,
    pub private_type_leaks: bool,
    pub deprecated_exports_in_use: bool,
    pub unused_enum_members: bool,
    pub unused_class_members: bool,
    pub unused_store_members: bool,
    pub unprovided_injects: bool,
    pub unrendered_components: bool,
    pub unused_component_props: bool,
    pub absent_component_props: bool,
    pub unused_component_emits: bool,
    pub unused_component_inputs: bool,
    pub unused_component_outputs: bool,
    pub unused_svelte_events: bool,
    pub unused_effect_layers: bool,
    pub unused_tagged_errors: bool,
    pub unproduced_catch_tags: bool,
    pub unused_server_actions: bool,
    pub unused_load_data_keys: bool,
    pub unresolved_imports: bool,
    pub unlisted_deps: bool,
    pub duplicate_exports: bool,
    pub circular_deps: bool,
    pub re_export_cycles: bool,
    pub package_cycles: bool,
    pub boundary_violations: bool,
    pub policy_violations: bool,
    pub stale_suppressions: bool,
    pub unused_catalog_entries: bool,
    pub empty_catalog_groups: bool,
    pub unresolved_catalog_references: bool,
    pub unused_dependency_overrides: bool,
    pub misconfigured_dependency_overrides: bool,
    pub invalid_client_exports: bool,
    pub mixed_client_server_barrels: bool,
    pub misplaced_directives: bool,
    pub route_collisions: bool,
    pub dynamic_segment_name_conflicts: bool,
    /// Bare `fallow --only architecture` / `--skip architecture`. The selection
    /// applies after the baseline stage, so it narrows the report and leaves the
    /// baseline file and the health input as a run without it has them.
    pub architecture_selection: ArchitectureSelection,
}

/// Which dead-code findings a bare `fallow` run reports.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ArchitectureSelection {
    /// Every dead-code finding (the default).
    #[default]
    All,
    /// Only the architecture findings (`--only architecture`).
    Only,
    /// Every finding except the architecture findings (`--skip architecture`).
    Exclude,
}

impl ArchitectureSelection {
    /// Remove the findings the selection leaves out of the report.
    pub fn apply(self, results: &mut fallow_types::results::AnalysisResults) {
        match self {
            Self::All => {}
            Self::Only => {
                let mut only = IssueFilters::default();
                for flag in [
                    "--circular-deps",
                    "--re-export-cycles",
                    "--package-cycles",
                    "--boundary-violations",
                    "--policy-violations",
                ] {
                    only.enable_cli_filter_flag(flag);
                }
                only.apply(results);
            }
            Self::Exclude => {
                results.circular_dependencies.clear();
                results.re_export_cycles.clear();
                results.package_cycles.clear();
                results.boundary_violations.clear();
                results.boundary_coverage_violations.clear();
                results.boundary_call_violations.clear();
                results.policy_violations.clear();
            }
        }
    }
}

impl IssueFilters {
    /// Enable a registered filter flag when the command line set it.
    pub fn enable_when_active(&mut self, flag: &str, active: bool) {
        if active {
            assert!(
                self.enable_cli_filter_flag(flag),
                "check command uses unregistered dead-code filter flag {flag}"
            );
        }
    }

    pub fn enable_cli_filter_flag(&mut self, flag: &str) -> bool {
        match flag {
            "--unused-files" => self.unused_files = true,
            "--unused-exports" => self.unused_exports = true,
            "--unused-deps" => self.unused_deps = true,
            "--misplaced-deps" => self.misplaced_deps = true,
            "--unused-types" => self.unused_types = true,
            "--private-type-leaks" => self.private_type_leaks = true,
            "--deprecated-exports-in-use" => self.deprecated_exports_in_use = true,
            "--unused-enum-members" => self.unused_enum_members = true,
            "--unused-class-members" => self.unused_class_members = true,
            "--unused-store-members" => self.unused_store_members = true,
            "--unprovided-injects" => self.unprovided_injects = true,
            "--unrendered-components" => self.unrendered_components = true,
            "--unused-component-props" => self.unused_component_props = true,
            "--absent-component-props" => self.absent_component_props = true,
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
            "--duplicate-exports" => self.duplicate_exports = true,
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

    pub const fn any_active(&self) -> bool {
        self.unused_files
            || self.unused_exports
            || self.unused_deps
            || self.misplaced_deps
            || self.unused_types
            || self.private_type_leaks
            || self.deprecated_exports_in_use
            || self.unused_enum_members
            || self.unused_class_members
            || self.unused_store_members
            || self.unprovided_injects
            || self.unrendered_components
            || self.unused_component_props
            || self.absent_component_props
            || self.unused_component_emits
            || self.unused_component_inputs
            || self.unused_component_outputs
            || self.unused_svelte_events
            || self.unused_effect_layers
            || self.unused_tagged_errors
            || self.unproduced_catch_tags
            || self.unused_server_actions
            || self.unused_load_data_keys
            || self.unresolved_imports
            || self.unlisted_deps
            || self.duplicate_exports
            || self.circular_deps
            || self.re_export_cycles
            || self.package_cycles
            || self.boundary_violations
            || self.policy_violations
            || self.stale_suppressions
            || self.unused_catalog_entries
            || self.empty_catalog_groups
            || self.unresolved_catalog_references
            || self.unused_dependency_overrides
            || self.misconfigured_dependency_overrides
            || self.invalid_client_exports
            || self.mixed_client_server_barrels
            || self.misplaced_directives
            || self.route_collisions
            || self.dynamic_segment_name_conflicts
    }

    /// Whether the report keeps dependency findings: no filter is active, or
    /// `--unused-deps`, `--misplaced-deps` or `--unlisted-deps` is one of the
    /// active filters, and
    /// the run does not report only the architecture findings.
    ///
    /// These are the issue types `ignoreDependencies` controls, so an
    /// unmatched `ignoreDependencies` glob is reported only when this is true.
    pub fn reports_dependency_findings(&self) -> bool {
        self.architecture_selection != ArchitectureSelection::Only
            && (!self.any_active() || self.unused_deps || self.misplaced_deps || self.unlisted_deps)
    }

    /// Enable off-by-default issue types when explicitly requested as filters.
    pub fn activate_explicit_opt_ins(&self, rules: &mut RulesConfig) {
        if self.absent_component_props && rules.absent_component_props == Severity::Off {
            rules.absent_component_props = Severity::Warn;
        }
        if self.private_type_leaks && rules.private_type_leaks == Severity::Off {
            rules.private_type_leaks = Severity::Warn;
        }
        if self.deprecated_exports_in_use && rules.deprecated_exports_in_use == Severity::Off {
            rules.deprecated_exports_in_use = Severity::Warn;
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
        }
        if !self.misplaced_deps {
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
        if !self.unlisted_deps {
            results.unlisted_dependencies.clear();
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
