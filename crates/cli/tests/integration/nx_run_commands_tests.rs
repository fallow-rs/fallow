#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests and benches use unwrap and expect to keep fixture setup concise"
)]

use crate::common::{parse_json, run_fallow};

fn unused_files(fixture: &str) -> Vec<String> {
    let output = run_fallow(
        "dead-code",
        fixture,
        &["--format", "json", "--quiet", "--no-cache"],
    );
    let json = parse_json(&output);
    json["unused_files"]
        .as_array()
        .map(|files| {
            files
                .iter()
                .filter_map(|file| file["path"].as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

#[test]
fn run_commands_targets_credit_script_files_and_jest_configs() {
    let unused = unused_files("nx-run-commands-targets");
    for credited in [
        "scripts/seed.ts",
        "lib/db.ts",
        "int/a.int.ts",
        "src/helper.ts",
        "integration.jest.ts",
    ] {
        assert!(
            !unused.iter().any(|path| path.ends_with(credited)),
            "{credited} is reached through a project.json target, got unused: {unused:?}"
        );
    }
    assert!(
        unused.iter().any(|path| path.ends_with("src/orphan.ts")),
        "a file that no target reaches stays unused, got: {unused:?}"
    );
}

/// Issue #5325: a command file argument without an extension resolves like a
/// package.json script argument, in a root target, a workspace target and a
/// CI workflow.
#[test]
fn run_commands_targets_credit_extensionless_script_files() {
    let unused = unused_files("nx-run-commands-extensionless");
    for credited in [
        "scripts/noext.js",
        "scripts/tsnoext.ts",
        "scripts/ci-check.mjs",
        "packages/app/tools/run.cjs",
        "scripts/nx-dir/index.js",
        "scripts/ci-dir/index.mjs",
        "scripts/docker-dir/index.js",
        "scripts/wrapped-dir/index.js",
    ] {
        assert!(
            !unused.iter().any(|path| path.ends_with(credited)),
            "{credited} is run without its extension or as a directory, got unused: {unused:?}"
        );
    }
    // `bash scripts/deploy.sh` and `bash scripts/pkg-deploy.sh` name shell
    // files that are not on disk, and a shell cannot run the directory of
    // `bash scripts/sh-dir`.
    for unrun in [
        "src/orphan.ts",
        "scripts/deploy.ts",
        "scripts/pkg-deploy.ts",
        "scripts/sh-dir/index.js",
    ] {
        assert!(
            unused.iter().any(|path| path.ends_with(unrun)),
            "{unrun} is run by no command, got: {unused:?}"
        );
    }
}

/// Issue #5370: a directory argument such as `vitest run src/lib/` is a
/// filter, not a module that the command runs, so the index file of the
/// directory does not become an entry. The fixture passes the directory in an
/// Nx target, a package.json script and a Dockerfile.
#[test]
fn run_commands_directory_argument_is_not_an_index_entry() {
    let output = run_fallow(
        "dead-code",
        "nx-run-commands-extensionless",
        &["--format", "json", "--quiet", "--no-cache"],
    );
    let json = parse_json(&output);
    let unused_exports: Vec<&str> = json["unused_exports"]
        .as_array()
        .map(|exports| {
            exports
                .iter()
                .filter_map(|export| export["export_name"].as_str())
                .collect()
        })
        .unwrap_or_default();
    assert!(
        unused_exports.contains(&"unusedHelper"),
        "src/lib/index.ts stays a plain module, got: {unused_exports:?}"
    );
}
