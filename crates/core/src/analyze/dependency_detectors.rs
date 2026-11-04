//! The dependency detectors: unused, unlisted, type-only, test-only and
//! dev-in-production dependencies, and unlisted binaries.

use super::*;

#[derive(Clone, Copy)]
pub(super) struct DependencyDetectorInput<'a> {
    pub(super) graph: &'a ModuleGraph,
    pub(super) pkg: Option<&'a PackageJson>,
    pub(super) config: &'a ResolvedConfig,
    pub(super) plugin_result: Option<&'a crate::plugins::AggregatedPluginResult>,
    pub(super) workspaces: &'a [fallow_config::WorkspaceInfo],
    pub(super) resolved_modules: &'a [ResolvedModule],
    pub(super) modules: &'a [ModuleInfo],
    pub(super) line_offsets_by_file: &'a LineOffsetsMap<'a>,
}

pub(super) fn run_dependency_detectors(input: DependencyDetectorInput<'_>) -> AnalysisResults {
    let mut results = AnalysisResults::default();
    let Some(pkg) = input.pkg else {
        return results;
    };

    populate_unused_dependency_findings(input, pkg, &mut results);
    populate_unlisted_dependency_findings(input, pkg, &mut results);
    populate_unlisted_binary_findings(input, pkg, &mut results);
    populate_type_only_dependency_findings(input, pkg, &mut results);
    populate_test_only_dependency_findings(input, pkg, &mut results);
    populate_dev_dependency_in_production_findings(input, pkg, &mut results);
    unused_deps::credit_production_ignores(input.graph, pkg, input.config, input.workspaces);
    results
}

fn populate_unlisted_dependency_findings(
    input: DependencyDetectorInput<'_>,
    pkg: &PackageJson,
    results: &mut AnalysisResults,
) {
    if input.config.rules.unlisted_dependencies != Severity::Off {
        results.unlisted_dependencies = find_unlisted_dependencies(UnlistedDependencyInput {
            graph: input.graph,
            pkg,
            config: input.config,
            workspaces: input.workspaces,
            plugin_result: input.plugin_result,
            resolved_modules: input.resolved_modules,
            modules: input.modules,
            line_offsets_by_file: input.line_offsets_by_file,
        })
        .into_iter()
        .map(UnlistedDependencyFinding::with_actions)
        .collect();
    }
}

fn populate_unlisted_binary_findings(
    input: DependencyDetectorInput<'_>,
    pkg: &PackageJson,
    results: &mut AnalysisResults,
) {
    if input.config.rules.unlisted_binaries == Severity::Off {
        return;
    }
    results.unlisted_binaries =
        unlisted_binaries::find_unlisted_binaries(unlisted_binaries::UnlistedBinaryInput {
            graph: input.graph,
            pkg,
            config: input.config,
            workspaces: input.workspaces,
            modules: input.modules,
            line_offsets_by_file: input.line_offsets_by_file,
        })
        .into_iter()
        .map(fallow_types::output_dead_code::UnlistedBinaryFinding::with_actions)
        .collect();
}

fn populate_type_only_dependency_findings(
    input: DependencyDetectorInput<'_>,
    pkg: &PackageJson,
    results: &mut AnalysisResults,
) {
    if input.config.production {
        results.type_only_dependencies =
            find_type_only_dependencies(input.graph, pkg, input.config, input.workspaces)
                .into_iter()
                .map(TypeOnlyDependencyFinding::with_actions)
                .collect();
    }
}

fn populate_test_only_dependency_findings(
    input: DependencyDetectorInput<'_>,
    pkg: &PackageJson,
    results: &mut AnalysisResults,
) {
    if !input.config.production && input.config.rules.test_only_dependencies != Severity::Off {
        results.test_only_dependencies =
            find_test_only_dependencies(input.graph, pkg, input.config, input.workspaces)
                .into_iter()
                .map(TestOnlyDependencyFinding::with_actions)
                .collect();
    }
}

fn populate_dev_dependency_in_production_findings(
    input: DependencyDetectorInput<'_>,
    pkg: &PackageJson,
    results: &mut AnalysisResults,
) {
    // Unlike the test-only sibling, this rule stays ON in production mode:
    // test files being undiscovered makes the question unanswerable for
    // test-only, but for dev-in-prod it only makes the signal cleaner (every
    // discovered file is production), and production CI is exactly where a
    // `pnpm install --prod` breakage matters.
    if input.config.rules.dev_dependencies_in_production != Severity::Off {
        results.dev_dependencies_in_production = find_dev_dependencies_in_production(
            input.graph,
            pkg,
            input.config,
            input.workspaces,
            input.plugin_result,
        )
        .into_iter()
        .map(DevDependencyInProductionFinding::with_actions)
        .collect();
    }
}

/// Populate the unused-dependency family (prod / dev / optional) on `results`,
/// each gated on its own rule severity. The three collections share one
/// `find_unused_dependencies` computation, so they are populated together.
fn populate_unused_dependency_findings(
    input: DependencyDetectorInput<'_>,
    pkg: &PackageJson,
    results: &mut AnalysisResults,
) {
    if unused_dependency_rules_are_disabled(input.config) {
        return;
    }
    let (deps, dev_deps, optional_deps) = find_unused_dependencies(
        input.graph,
        pkg,
        input.config,
        input.plugin_result,
        input.workspaces,
    );
    populate_unused_prod_dependency_findings(results, input.config, deps);
    populate_unused_dev_dependency_findings(results, input.config, dev_deps);
    populate_unused_optional_dependency_findings(results, input.config, optional_deps);
}

fn unused_dependency_rules_are_disabled(config: &ResolvedConfig) -> bool {
    config.rules.unused_dependencies == Severity::Off
        && config.rules.unused_dev_dependencies == Severity::Off
        && config.rules.unused_optional_dependencies == Severity::Off
}

fn populate_unused_prod_dependency_findings(
    results: &mut AnalysisResults,
    config: &ResolvedConfig,
    deps: Vec<UnusedDependency>,
) {
    if config.rules.unused_dependencies == Severity::Off {
        return;
    }
    results.unused_dependencies = deps
        .into_iter()
        .map(UnusedDependencyFinding::with_actions)
        .collect();
}

fn populate_unused_dev_dependency_findings(
    results: &mut AnalysisResults,
    config: &ResolvedConfig,
    dev_deps: Vec<UnusedDependency>,
) {
    if config.rules.unused_dev_dependencies == Severity::Off {
        return;
    }
    results.unused_dev_dependencies = dev_deps
        .into_iter()
        .map(UnusedDevDependencyFinding::with_actions)
        .collect();
}

fn populate_unused_optional_dependency_findings(
    results: &mut AnalysisResults,
    config: &ResolvedConfig,
    optional_deps: Vec<UnusedDependency>,
) {
    if config.rules.unused_optional_dependencies == Severity::Off {
        return;
    }
    results.unused_optional_dependencies = optional_deps
        .into_iter()
        .map(UnusedOptionalDependencyFinding::with_actions)
        .collect();
}
