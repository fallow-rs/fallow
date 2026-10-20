//! A Vitest `test.include` replaces the default test patterns, but it does not
//! remove the bench files, which `test.benchmark.include` selects. The
//! `test.include` of a config below the project root applies below the
//! directory of that config only, and it does not remove the patterns of the
//! root config. A reporter path is a file that Vitest imports.

use super::common::{create_config, fixture_path};

fn unused_files(fixture: &str) -> Vec<String> {
    let config = create_config(fixture_path(fixture));
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let mut files: Vec<String> = results
        .unused_files
        .iter()
        .map(|file| {
            file.file
                .path
                .strip_prefix(&config.root)
                .unwrap_or(&file.file.path)
                .to_string_lossy()
                .replace('\\', "/")
        })
        .collect();
    files.sort();
    files
}

#[test]
fn vitest_include_keeps_bench_scopes_nested_configs_and_credits_reporter_files() {
    assert_eq!(
        unused_files("vitest-include-scope"),
        vec!["examples/app/src/orphan.ts", "src/orphan.ts"],
        "bench files, the nested include, the root include and the reporter \
         file must be used; only the orphans stay unused"
    );
}

#[test]
fn nested_vitest_include_keeps_the_default_test_patterns_of_the_project() {
    assert_eq!(
        unused_files("vitest-nested-include-only"),
        vec!["examples/app/src/orphan.ts"],
        "the include of a config below the root must not remove the default \
         test entries of the project"
    );
}

#[test]
fn vitest_configs_that_a_script_or_a_project_loads_keep_their_includes() {
    assert_eq!(
        unused_files("vitest-config-load-paths"),
        vec!["src/orphan.ts"],
        "a script `--config` below the package root runs in the package root, \
         and a vite config that a `projects` entry names is not shadowed"
    );
}
