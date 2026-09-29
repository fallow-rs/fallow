//! Issue #2954: formatter and linter targets are not entry points.
//!
//! `oxfmt --check "**/*.ts"` or `eslint src/x.ts` reads the files but does not
//! execute them. The tool stays a used dependency, but its file arguments must
//! not make the files reachable. `node scripts/x.ts` still creates an entry.

use super::common::{create_config, fixture_path};

fn unused_file_paths(results: &fallow_types::results::AnalysisResults) -> Vec<String> {
    results
        .unused_files
        .iter()
        .map(|f| f.file.path.to_string_lossy().replace('\\', "/"))
        .collect()
}

fn unused_dev_dependency_names(results: &fallow_types::results::AnalysisResults) -> Vec<String> {
    results
        .unused_dev_dependencies
        .iter()
        .map(|d| d.dep.package_name.clone())
        .collect()
}

fn is_reported(paths: &[String], suffix: &str) -> bool {
    paths.iter().any(|p| p.ends_with(suffix))
}

#[test]
fn package_json_formatter_and_linter_targets_do_not_seed_entries() {
    let root = fixture_path("issue-2954-script-lint-targets");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let paths = unused_file_paths(&results);
    for dead in [
        "src/dead.ts",
        "src/dead-lint.ts",
        "src/dead-prettier.ts",
        "src/dead-yarn.ts",
        "src/dead-wrapped.ts",
        "src/dead-env.ts",
        "src/dead-text.ts",
        "src/dead-call-npm.ts",
        "src/dead-call-npm-positional.ts",
        "src/dead-call-yarn.ts",
        "src/dead-call-pnpm.ts",
        "src/dead-call-pnpm-run.ts",
        "src/dead-call-pnpm-filter.ts",
        "src/dead-filter.ts",
        "src/dead-recursive.ts",
        "src/dead-dotenv.ts",
    ] {
        assert!(
            is_reported(&paths, dead),
            "{dead} is only a formatter or linter target and must stay unused. Got: {paths:?}"
        );
    }
    assert!(
        !is_reported(&paths, "scripts/tool.ts"),
        "scripts/tool.ts runs through `node` and must stay an entry. Got: {paths:?}"
    );
    for loaded in [
        "tools/fmt.js",
        "tools/prettier-plugin.mjs",
        "tools/rules/no-foo.js",
    ] {
        assert!(
            !is_reported(&paths, loaded),
            "{loaded} is loaded through a formatter or plugin flag and must stay reachable. \
             Got: {paths:?}"
        );
    }
    assert!(
        !is_reported(&paths, "src/index.ts"),
        "src/index.ts is the package main. Got: {paths:?}"
    );

    let unused_dev = unused_dev_dependency_names(&results);
    for tool in ["oxfmt", "eslint", "oxlint", "prettier", "textlint"] {
        assert!(
            !unused_dev.iter().any(|name| name == tool),
            "{tool} runs in a script and must stay a used dependency. Got: {unused_dev:?}"
        );
    }
}

#[test]
fn ci_formatter_and_linter_targets_do_not_seed_entries() {
    let root = fixture_path("issue-2954-ci-lint-targets");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let paths = unused_file_paths(&results);
    for dead in [
        "src/dead.ts",
        "src/dead-lint.ts",
        "src/dead-yarn.ts",
        "src/dead-docker.ts",
        "src/dead-ci-call.ts",
        "src/dead-docker-call.ts",
        "src/dead-ci-positional.ts",
        "src/dead-docker-positional.ts",
    ] {
        assert!(
            is_reported(&paths, dead),
            "{dead} is only a formatter or linter target in CI or a Dockerfile and must \
             stay unused. Got: {paths:?}"
        );
    }
    assert!(
        !is_reported(&paths, "scripts/ci-tool.ts"),
        "scripts/ci-tool.ts runs through `node` in CI and must stay an entry. Got: {paths:?}"
    );

    let unused_dev = unused_dev_dependency_names(&results);
    for tool in ["oxfmt", "eslint"] {
        assert!(
            !unused_dev.iter().any(|name| name == tool),
            "{tool} runs in CI and must stay a used dependency. Got: {unused_dev:?}"
        );
    }
}

fn ignore_command_entries_config(ignored: &[&str]) -> fallow_config::ResolvedConfig {
    let mut config = create_config(fixture_path("issue-2954-ignore-command-entries"));
    config.ignore_command_entries = ignored.iter().map(|name| (*name).to_string()).collect();
    config
}

#[test]
fn command_file_arguments_are_entries_without_the_option() {
    let config = ignore_command_entries_config(&[]);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let paths = unused_file_paths(&results);
    for entry in [
        "src/dead.ts",
        "src/ci-input.ts",
        "src/docker-input.ts",
        "src/gen-input.ts",
        "src/gen-positional-input.ts",
        "src/docker-call-input.ts",
        "src/docker-flag-input.ts",
    ] {
        assert!(
            !is_reported(&paths, entry),
            "{entry} is a command file argument and is an entry by default. Got: {paths:?}"
        );
    }
}

#[test]
fn ignore_command_entries_drops_the_listed_command_everywhere() {
    let config = ignore_command_entries_config(&["my-codegen"]);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let paths = unused_file_paths(&results);
    for dead in [
        "src/dead.ts",
        "src/ci-input.ts",
        "src/docker-input.ts",
        "src/gen-input.ts",
        "src/gen-positional-input.ts",
        "src/docker-call-input.ts",
        "src/docker-flag-input.ts",
    ] {
        assert!(
            is_reported(&paths, dead),
            "{dead} is only an argument of an ignored command and must be unused. \
             Got: {paths:?}"
        );
    }
    for kept in ["scripts/seed.ts", "codegen.config.ts", "src/index.ts"] {
        assert!(
            !is_reported(&paths, kept),
            "{kept} must stay reachable: a `node` entry, a `--config` file, or the \
             package main. Got: {paths:?}"
        );
    }
    let unused_dev = unused_dev_dependency_names(&results);
    assert!(
        !unused_dev.iter().any(|name| name == "my-codegen"),
        "an ignored command still counts as a used dependency. Got: {unused_dev:?}"
    );
}

#[test]
fn ignore_command_entries_wildcard_drops_every_command_entry() {
    let config = ignore_command_entries_config(&["*"]);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let paths = unused_file_paths(&results);
    for dead in ["scripts/seed.ts", "src/docker-flag-input.ts"] {
        assert!(
            is_reported(&paths, dead),
            "`*` drops {dead} too, also as a flag value forwarded through a script call. \
             Got: {paths:?}"
        );
    }
    assert!(
        !is_reported(&paths, "codegen.config.ts"),
        "`*` keeps `--config` files. Got: {paths:?}"
    );
}

#[test]
fn workspace_and_task_runner_forms_resolve_where_the_command_runs() {
    let root = fixture_path("issue-2954-workspace-command-forms");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let paths = unused_file_paths(&results);
    for dead in [
        "src/dead-workspace.ts",
        "src/dead-filter.ts",
        "src/dead-filter-run.ts",
        "src/dead-recursive.ts",
        "src/dead-foreach.ts",
        "src/dead-workspaces-run.ts",
        "src/dead-npm-workspace.ts",
        "src/dead-turbo.ts",
        "cfg/tagged.ts",
    ] {
        assert!(
            is_reported(&paths, dead),
            "{dead} is an argument of a command in another package, of a task runner, or \
             the value of an npm flag, and must stay unused. Got: {paths:?}"
        );
    }
    for kept in [
        "src/gen-input.ts",
        "packages/web/scripts/gen.ts",
        "src/docker-input.ts",
        "scripts/gen.ts",
        "tools/check.js",
    ] {
        assert!(
            !is_reported(&paths, kept),
            "{kept} must stay reachable: a forwarded runner argument, a runner file in the \
             `pnpm -C` directory, or the target of a script named after a formatter. \
             Got: {paths:?}"
        );
    }

    let unused_dev = unused_dev_dependency_names(&results);
    for tool in ["eslint", "tsx", "turbo"] {
        assert!(
            !unused_dev.iter().any(|name| name == tool),
            "{tool} runs in a root script and must stay a used dependency. Got: {unused_dev:?}"
        );
    }
}
