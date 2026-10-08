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
