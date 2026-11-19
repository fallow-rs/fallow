use super::common::{create_config, fixture_path};

fn analyze(fixture: &str) -> fallow_core::results::AnalysisResults {
    let config = create_config(fixture_path(fixture));
    fallow_core::analyze(&config).expect("analysis should succeed")
}

fn unused_export_names(results: &fallow_core::results::AnalysisResults) -> Vec<String> {
    let mut names: Vec<String> = results
        .unused_exports
        .iter()
        .map(|export| export.export.export_name.clone())
        .collect();
    names.sort_unstable();
    names
}

/// Issue #4714: a Svelte declaration tag (`{const x = ...}`) reads a prop and
/// an import in the same way as `{@const x = ...}`.
#[test]
fn svelte_declaration_tag_credits_props_and_imports() {
    let results = analyze("issue-4714-svelte-declaration-tag");
    let props: Vec<&str> = results
        .unused_component_props
        .iter()
        .map(|prop| prop.prop.prop_name.as_str())
        .collect();
    assert!(props.is_empty(), "no prop is unused: {props:?}");
    assert_eq!(unused_export_names(&results), ["unusedHelper"]);
}

/// Issue #4715: the `{#await expr then}` and `{#await expr catch}` shorthand
/// forms read `expr` in the same way as the long form.
#[test]
fn svelte_await_shorthand_credits_the_await_expression() {
    let results = analyze("issue-4715-svelte-await-shorthand");
    assert_eq!(unused_export_names(&results), ["neverAwaited"]);
}
