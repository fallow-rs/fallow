#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests and benches use unwrap and expect to keep fixture setup concise"
)]

//! `fallow fix` keeps `@sveltejs/kit` when source files import its virtual
//! modules (`$app/*`, `./$types`), also in a project without
//! `svelte.config.js`. A removal breaks those imports (issue #5019).

use crate::common::{parse_json, run_fallow};

#[test]
fn fix_does_not_remove_kit_that_virtual_modules_use() {
    let preview = run_fallow(
        "fix",
        "sveltekit-project",
        &["--dry-run", "--format", "json", "--quiet", "--no-cache"],
    );
    let mut removed: Vec<String> = parse_json(&preview)["fixes"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|fix| fix["type"] == "remove_dependency" && fix["skipped"] != true)
        .map(|fix| fix["package"].as_str().unwrap().to_owned())
        .collect();
    removed.sort();
    assert_eq!(
        removed,
        vec![
            "@sveltejs/adapter-auto".to_owned(),
            "@sveltejs/vite-plugin-svelte".to_owned(),
        ],
        "the $app and ./$types imports use @sveltejs/kit; only the config-only packages go"
    );
}
