//! The `framework-config` rule for the functions of a tool config file, and
//! the `build-tool-only` rule for modules that only tool config files reach.

use super::*;

const CONFIG: MutantDisposition = MutantDisposition::Pruned(PruneReason::FrameworkConfig);
const BUILD_TOOL: MutantDisposition = MutantDisposition::Pruned(PruneReason::BuildToolOnly);

fn build_tool_plan() -> MutationPlan {
    plan_mutations_for_session(
        &session(&fixture("mutation-plan-build-tool")),
        &MutationPlanRequest::new(MutationScope::All),
    )
    .expect("plan")
}

#[test]
fn each_mutant_of_an_untested_tool_config_file_is_framework_config() {
    let plan = build_tool_plan();
    assert_eq!(
        dispositions(&plan).get("vite.config.ts"),
        Some(&set(&[CONFIG])),
        "the inline plugin hook, its middleware callback and the top-level code are pruned"
    );
    assert!(
        function_dispositions(&plan, "vite.config.ts").contains_key("configureServer"),
        "the fixture has mutants in the inline plugin hook"
    );
}

#[test]
fn a_module_that_only_a_tool_config_file_reaches_is_build_tool_only() {
    let plan = build_tool_plan();
    let map = dispositions(&plan);
    assert_eq!(
        map.get("vite-plugins/mockApi.ts"),
        Some(&set(&[BUILD_TOOL]))
    );
    assert_eq!(
        map.get("vite-plugins/items.ts"),
        Some(&set(&[BUILD_TOOL])),
        "a module that the plugin module imports is build-tool only too"
    );
    assert_eq!(
        plan.counts.pruned.build_tool_only,
        plan.mutants
            .iter()
            .filter(|entry| entry.disposition == BUILD_TOOL)
            .count()
    );
    assert!(plan.counts.pruned.build_tool_only > 0);
}

#[test]
fn a_plugin_module_that_a_test_or_the_application_imports_keeps_its_status() {
    let plan = build_tool_plan();
    let map = dispositions(&plan);
    assert_eq!(
        map.get("vite-plugins/banner.ts"),
        Some(&set(&[PLANNED])),
        "a test imports the module, so its mutants run"
    );
    assert_eq!(
        map.get("vite-plugins/stamp.ts"),
        Some(&set(&[NO_COVERAGE])),
        "an application entry imports the module, so it is untested application code"
    );
    assert_eq!(map.get("src/add.ts"), Some(&set(&[PLANNED])));
}

#[test]
fn a_tool_config_file_that_a_test_imports_keeps_its_function_mutants() {
    let plan = plan_mutations_for_session(
        &session(&fixture("mutation-plan-build-tool-tested-config")),
        &MutationPlanRequest::new(MutationScope::All),
    )
    .expect("plan");
    let vite = function_dispositions(&plan, "vite.config.ts");
    assert_eq!(vite.get("<top-level>"), Some(&CONFIG));
    assert_eq!(
        vite.get("transform"),
        Some(&PLANNED),
        "a test imports the config, so the plugin hook can run"
    );
    assert_eq!(
        dispositions(&plan).get("vite-plugins/helper.ts"),
        Some(&set(&[PLANNED])),
        "the test reaches the module through the config file"
    );
}

#[test]
fn the_build_tool_rule_needs_the_framework_config_facts() {
    let plan = plans(
        &fixture("mutation-plan-build-tool"),
        None,
        &[MutationPlanRequest::new(MutationScope::All)],
    )
    .remove(0);
    assert_eq!(plan.counts.pruned.build_tool_only, 0);
    assert_eq!(plan.counts.pruned.framework_config, 0);
}

#[test]
fn a_config_named_file_that_an_application_entry_imports_is_not_pruned() {
    let plan = plan_mutations_for_session(
        &session(&fixture("mutation-plan-angular-app-config")),
        &MutationPlanRequest::new(MutationScope::All),
    )
    .expect("plan");
    let config = function_dispositions(&plan, "src/app/app.config.ts");
    assert_eq!(
        config.get("providers"),
        Some(&NO_COVERAGE),
        "`src/main.ts` imports the app config, so its initializer is untested application code"
    );
}
