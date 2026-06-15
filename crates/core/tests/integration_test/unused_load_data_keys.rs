use fallow_config::{FallowConfig, OutputFormat, RulesConfig, Severity};

use crate::common::fixture_path;

/// Resolve the fixture with the default rule set: `unused-load-data-key` at
/// `warn` (its default). The detector is gated on the project declaring
/// `@sveltejs/kit`, which the fixture's package.json does.
fn fixture_config(name: &str) -> fallow_config::ResolvedConfig {
    FallowConfig::default().resolve(fixture_path(name), OutputFormat::Human, 4, true, true, None)
}

/// Same fixture with the rule turned off (neuter check).
fn fixture_config_rule_off(name: &str) -> fallow_config::ResolvedConfig {
    FallowConfig {
        rules: RulesConfig {
            unused_load_data_keys: Severity::Off,
            ..RulesConfig::default()
        },
        ..Default::default()
    }
    .resolve(fixture_path(name), OutputFormat::Human, 4, true, true, None)
}

fn key_names(results: &fallow_core::results::AnalysisResults) -> Vec<String> {
    results
        .unused_load_data_keys
        .iter()
        .map(|f| f.key.key_name.clone())
        .collect()
}

#[test]
fn dead_load_key_is_flagged_and_anchored() {
    let config = fixture_config("sveltekit-load-data");
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let keys = key_names(&results);
    assert!(
        keys.contains(&"dead".to_string()),
        "the `dead` load key (read by no consumer) should be flagged: {keys:?}"
    );

    let dead = results
        .unused_load_data_keys
        .iter()
        .find(|f| f.key.key_name == "dead")
        .expect("dead key finding");
    assert!(
        dead.key
            .path
            .to_string_lossy()
            .replace('\\', "/")
            .ends_with("routes/blog/+page.ts"),
        "finding should anchor at the producer +page.ts, got {}",
        dead.key.path.display()
    );
    assert_eq!(
        dead.key.route_dir.as_deref(),
        Some("src/routes/blog"),
        "route_dir should be the producer's directory relative to root"
    );
}

#[test]
fn consumed_load_key_is_not_flagged() {
    let config = fixture_config("sveltekit-load-data");
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let keys = key_names(&results);
    // `used` is read via data.used in the sibling +page.svelte.
    assert!(
        !keys.contains(&"used".to_string()),
        "`used` is read off data.used in +page.svelte and must not be flagged: {keys:?}"
    );
}

#[test]
fn global_page_data_channel_credits_the_key() {
    let config = fixture_config("sveltekit-load-data");
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let keys = key_names(&results);
    // `globalKey` is read ONLY via page.data.globalKey in a shared Header.svelte,
    // never off the sibling +page.svelte. The project-wide channel must credit it.
    assert!(
        !keys.contains(&"globalKey".to_string()),
        "`globalKey` is read via page.data.globalKey and must not be flagged: {keys:?}"
    );
}

#[test]
fn server_load_key_consumed_by_universal_sibling_is_not_flagged() {
    let config = fixture_config("sveltekit-load-data");
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let keys = key_names(&results);
    // `serverKey` is returned by +page.server.ts and read only by the sibling
    // universal +page.ts's `data` param (FP-2). It must not be flagged.
    assert!(
        !keys.contains(&"serverKey".to_string()),
        "`serverKey` is consumed by the universal +page.ts sibling and must not be flagged: {keys:?}"
    );
}

#[test]
fn rule_off_produces_no_findings() {
    let config = fixture_config_rule_off("sveltekit-load-data");
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    assert!(
        results.unused_load_data_keys.is_empty(),
        "rule off must produce no unused-load-data-key findings: {:?}",
        key_names(&results)
    );
}

#[test]
fn no_findings_when_sveltekit_is_absent() {
    let config = fixture_config("sveltekit-load-data-no-dep");
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    assert!(
        results.unused_load_data_keys.is_empty(),
        "without @sveltejs/kit declared, the rule must not fire: {:?}",
        key_names(&results)
    );
}
