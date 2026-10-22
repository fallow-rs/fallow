use super::super::PathRule;
use super::AggregatedPluginResult;

fn entry(pattern: &str, plugin: &str) -> (PathRule, String) {
    (PathRule::new(pattern), plugin.to_string())
}

#[test]
fn unique_entry_patterns_drops_repeats_from_a_workspace_merge() {
    let mut root = AggregatedPluginResult {
        entry_patterns: vec![
            entry("packages/a/src/app.ts", "egg"),
            entry("src/index.ts", "vitest"),
        ],
        ..AggregatedPluginResult::default()
    };
    let mut workspace = AggregatedPluginResult {
        entry_patterns: vec![entry("src/app.ts", "egg")],
        ..AggregatedPluginResult::default()
    };
    workspace.apply_workspace_prefix("packages/a");
    root.merge_into(workspace);
    root.entry_patterns
        .push(entry("packages/a/src/app.ts", "other"));

    let unique: Vec<(&str, &str)> = root
        .unique_entry_patterns()
        .map(|(rule, plugin)| (rule.pattern.as_str(), plugin.as_str()))
        .collect();

    assert_eq!(
        unique,
        vec![
            ("packages/a/src/app.ts", "egg"),
            ("src/index.ts", "vitest"),
            ("packages/a/src/app.ts", "other"),
        ],
    );
}

#[test]
fn unique_entry_patterns_keeps_rules_that_differ_in_exclusions() {
    let mut excluded = PathRule::new("src/**/*.ts");
    excluded.exclude_globs.push("src/**/*.test.ts".to_string());
    let result = AggregatedPluginResult {
        entry_patterns: vec![entry("src/**/*.ts", "egg"), (excluded, "egg".to_string())],
        ..AggregatedPluginResult::default()
    };

    assert_eq!(result.unique_entry_patterns().count(), 2);
}
