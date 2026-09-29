//! Package-manager and task-runner forms that run a command in another
//! location, the npm config flags that take a value, and script names that
//! shadow a binary.

#![expect(
    clippy::disallowed_types,
    reason = "ScriptCatalog takes the serde-deserialized std HashMap"
)]

use std::collections::HashMap;
use std::path::Path;

use rustc_hash::{FxHashMap, FxHashSet};

use super::{
    IgnoredCommandEntries, NPM_DEPENDENCY_VALUE_FLAGS, NPM_LOCATION_VALUE_FLAGS,
    NPM_OUTPUT_VALUE_FLAGS, NPM_PUBLISH_VALUE_FLAGS, NPM_REGISTRY_VALUE_FLAGS,
    NPM_RUNTIME_VALUE_FLAGS, ScriptAnalysis, ScriptCatalog, analyze_commands_with_context,
};

fn scripts_map(scripts: &[(&str, &str)]) -> HashMap<String, String> {
    scripts
        .iter()
        .map(|(name, body)| ((*name).to_string(), (*body).to_string()))
        .collect()
}

fn analyze_with_catalog(
    command: &str,
    catalog: &ScriptCatalog,
    declared: &[&str],
) -> ScriptAnalysis {
    let declared: FxHashSet<String> = declared.iter().map(|name| (*name).to_string()).collect();
    analyze_commands_with_context(
        &[command.to_string()],
        Path::new("/nonexistent"),
        &FxHashMap::default(),
        &declared,
        catalog,
        IgnoredCommandEntries::NONE,
    )
}

fn analyze(command: &str, scripts: &[(&str, &str)], declared: &[&str]) -> ScriptAnalysis {
    analyze_with_catalog(
        command,
        &ScriptCatalog::from_scripts(&scripts_map(scripts)),
        declared,
    )
}

#[test]
fn a_linter_in_other_workspace_packages_is_credited_without_entries() {
    for command in [
        "yarn workspace web eslint src/dead.ts",
        "yarn workspace web run eslint src/dead.ts",
        "yarn workspace web exec eslint src/dead.ts",
        "yarn workspaces foreach -A exec eslint src/dead.ts",
        "yarn workspaces foreach --all --parallel -j 4 run eslint src/dead.ts",
        "pnpm --filter web eslint src/dead.ts",
        "pnpm -F web eslint src/dead.ts",
        "pnpm --filter=web eslint src/dead.ts",
        "pnpm -r eslint src/dead.ts",
        "pnpm --recursive --parallel eslint src/dead.ts",
        "npm exec -w web -- eslint src/dead.ts",
        "npm --workspaces exec -- eslint src/dead.ts",
    ] {
        let result = analyze(command, &[], &["eslint"]);
        assert!(
            result.used_packages.contains("eslint"),
            "`{command}` did not credit eslint: {:?}",
            result.used_packages
        );
        assert!(
            result.entry_files.is_empty(),
            "`{command}` produced entries: {:?}",
            result.entry_files
        );
    }
}

#[test]
fn a_runner_in_other_workspace_packages_makes_no_entry_here() {
    for command in [
        "yarn workspace web tsx scripts/run.ts",
        "yarn workspaces foreach -A exec tsx scripts/run.ts",
        "pnpm --filter web tsx scripts/run.ts",
        "pnpm --filter web exec tsx scripts/run.ts",
        "pnpm -r exec tsx scripts/run.ts",
        "npm exec --workspace=web -- tsx scripts/run.ts",
        "varlock run -- pnpm -r exec tsx scripts/run.ts",
    ] {
        let result = analyze(command, &[], &["tsx", "varlock"]);
        assert!(
            result.used_packages.contains("tsx"),
            "`{command}` did not credit tsx: {:?}",
            result.used_packages
        );
        assert!(
            result.entry_files.is_empty(),
            "`{command}` produced entries relative to the calling package: {:?}",
            result.entry_files
        );
    }
}

#[test]
fn a_runner_in_another_directory_resolves_its_file_there() {
    for command in [
        "pnpm -C packages/web exec tsx scripts/run.ts",
        "pnpm --dir=packages/web exec tsx ./scripts/run.ts",
        "npm --prefix packages/web exec -- tsx scripts/run.ts",
        "yarn --cwd packages/web tsx scripts/run.ts",
    ] {
        let result = analyze(command, &[], &["tsx"]);
        assert_eq!(
            result.entry_files,
            vec!["packages/web/scripts/run.ts"],
            "`{command}`"
        );
    }
}

#[test]
fn a_script_call_in_other_workspace_packages_makes_no_entry_here() {
    let scripts = [("gen", "node scripts/gen.js"), ("lint", "eslint")];
    for command in [
        "pnpm -r run gen -- src/a.ts",
        "pnpm -r gen src/a.ts",
        "pnpm --recursive run lint -- src/a.ts",
        "pnpm --filter web run gen src/a.ts",
        "pnpm run --filter web gen src/a.ts",
        "pnpm -C packages/web run gen src/a.ts",
        "npm -w web run gen -- src/a.ts",
        "npm --workspace=web run gen -- src/a.ts",
        "npm --workspaces run gen -- src/a.ts",
        "npm --prefix packages/web run gen -- src/a.ts",
        "yarn workspace web gen src/a.ts",
        "yarn workspace web run gen src/a.ts",
        "yarn workspaces foreach -A run gen src/a.ts",
        "yarn workspaces foreach --include 'web*' gen src/a.ts",
    ] {
        let result = analyze(command, &scripts, &[]);
        assert!(
            result.entry_files.is_empty(),
            "`{command}` produced entries relative to the calling package: {:?}",
            result.entry_files
        );
    }
}

#[test]
fn a_task_runner_forwards_no_entry() {
    for command in [
        "turbo run lint -- src/dead.ts",
        "turbo lint -- src/dead.ts",
        "npx turbo run lint --filter=web -- src/dead.ts",
        "nx run-many -t lint -- src/dead.ts",
        "lerna run lint -- src/dead.ts",
        "lerna exec -- node scripts/run.ts",
    ] {
        let result = analyze(command, &[], &["turbo", "nx", "lerna"]);
        assert!(
            result.entry_files.is_empty() && result.config_files.is_empty(),
            "`{command}` produced entries: {:?} {:?}",
            result.entry_files,
            result.config_files
        );
    }
    let result = analyze("turbo run lint -- src/dead.ts", &[], &["turbo"]);
    assert!(result.used_packages.contains("turbo"));
}

/// Call `gen` with `flag value` before a forwarded file and check that npm
/// consumes the value: only the forwarded file becomes an entry. A flag that
/// selects a workspace or a directory runs the script in another location,
/// so no file becomes an entry of the calling package.
fn assert_npm_consumes_values(group: &[&str]) {
    let scripts = [("gen", "node scripts/gen.js")];
    for flag in group {
        for command in [
            format!("npm run gen {flag} cfg/value.ts src/a.ts"),
            format!("npm run {flag} cfg/value.ts gen src/a.ts"),
        ] {
            let result = analyze(&command, &scripts, &[]);
            let expected: &[&str] = if matches!(*flag, "-w" | "--workspace" | "-C" | "--prefix") {
                &[]
            } else {
                &["scripts/gen.js", "src/a.ts"]
            };
            assert_eq!(result.entry_files, expected, "`{command}`");
        }
    }
}

#[test]
fn npm_location_flags_take_a_value() {
    assert_npm_consumes_values(NPM_LOCATION_VALUE_FLAGS);
}

#[test]
fn npm_registry_flags_take_a_value() {
    assert_npm_consumes_values(NPM_REGISTRY_VALUE_FLAGS);
}

#[test]
fn npm_dependency_flags_take_a_value() {
    assert_npm_consumes_values(NPM_DEPENDENCY_VALUE_FLAGS);
}

#[test]
fn npm_runtime_flags_take_a_value() {
    assert_npm_consumes_values(NPM_RUNTIME_VALUE_FLAGS);
}

#[test]
fn npm_output_flags_take_a_value() {
    assert_npm_consumes_values(NPM_OUTPUT_VALUE_FLAGS);
}

#[test]
fn npm_publish_flags_take_a_value() {
    assert_npm_consumes_values(NPM_PUBLISH_VALUE_FLAGS);
}

#[test]
fn npm_flags_named_in_the_issue_take_a_value() {
    let scripts = [("gen", "node scripts/gen.js")];
    for flag in [
        "--tag",
        "--scope",
        "--otp",
        "--before",
        "--node-options",
        "--include",
        "--omit",
        "--registry",
        "--userconfig",
    ] {
        let command = format!("npm run gen {flag} cfg/value.ts src/a.ts");
        let result = analyze(&command, &scripts, &[]);
        assert_eq!(
            result.entry_files,
            vec!["scripts/gen.js", "src/a.ts"],
            "`{command}`"
        );
    }
}

#[test]
fn a_declared_script_named_after_a_linter_keeps_its_targets() {
    let scripts = [("eslint", "node tools/check.js")];
    for command in [
        "yarn eslint src/a.ts",
        "yarn run eslint src/a.ts",
        "pnpm eslint src/a.ts",
        "varlock run -- yarn eslint src/a.ts",
    ] {
        let result = analyze(command, &scripts, &["eslint", "varlock"]);
        assert!(
            result.entry_files.iter().any(|path| path == "src/a.ts"),
            "`{command}` lost the target of the declared `eslint` script: {:?}",
            result.entry_files
        );
    }
}

#[test]
fn a_wrapped_call_of_a_linter_script_makes_no_entry() {
    let scripts = [("lint", "eslint")];
    for command in [
        "varlock run -- yarn lint src/dead.ts",
        "varlock run -- pnpm lint src/dead.ts",
        "varlock run -- npm run lint -- src/dead.ts",
    ] {
        let result = analyze(command, &scripts, &["eslint", "varlock"]);
        assert!(
            result.entry_files.is_empty(),
            "`{command}` produced entries: {:?}",
            result.entry_files
        );
    }
}
