//! The `testing` feature of `fallow-config`, `fallow-engine` and `fallow-api`
//! makes the session constructors that take a config without a resolved run
//! public. A release build must not have them, so only a dev-dependency may
//! turn the feature on. This test reads each manifest of the workspace and
//! fails when a normal dependency, a build dependency, a workspace
//! dependency or another feature turns `testing` on.

use toml::Value;

use crate::architecture_boundaries::rust_sources_under;

const TESTING: &str = "testing";

/// Each manifest of the workspace, relative to the workspace root.
fn manifests() -> Vec<String> {
    let mut manifests = vec!["Cargo.toml".to_owned()];
    let mut crates: Vec<String> = rust_sources_under(["crates"])
        .into_iter()
        .filter_map(|path| {
            let mut parts = path.split('/');
            Some(format!("crates/{}/Cargo.toml", parts.nth(1)?))
        })
        .collect();
    crates.sort();
    crates.dedup();
    manifests.extend(crates);
    manifests
}

fn read_manifest(path: &str) -> Value {
    let root = std::path::PathBuf::from(
        std::env::var_os("CARGO_MANIFEST_DIR").expect("cargo sets CARGO_MANIFEST_DIR"),
    )
    .join("../..");
    let text = std::fs::read_to_string(root.join(path))
        .unwrap_or_else(|error| panic!("read {path}: {error}"));
    toml::from_str(&text).unwrap_or_else(|error| panic!("parse {path}: {error}"))
}

/// Whether a feature list entry turns on `testing` of this crate or of a
/// dependency (`testing`, `dep/testing`, `dep?/testing`).
fn names_testing(entry: &str) -> bool {
    entry == TESTING
        || entry
            .rsplit_once('/')
            .is_some_and(|(_, feature)| feature == TESTING)
}

/// Each dependency of `table` that turns on `testing`, as `<section>.<name>`.
fn testing_dependencies(section: &str, table: Option<&Value>) -> Vec<String> {
    let Some(table) = table.and_then(Value::as_table) else {
        return Vec::new();
    };
    table
        .iter()
        .filter(|(_, spec)| {
            spec.get("features")
                .and_then(Value::as_array)
                .is_some_and(|features| features.iter().any(|f| f.as_str() == Some(TESTING)))
        })
        .map(|(name, _)| format!("{section}.{name}"))
        .collect()
}

/// Each place in `manifest` that turns on `testing` in a build that is not a
/// test build.
fn release_testing_entries(manifest: &Value) -> Vec<String> {
    let mut entries = Vec::new();
    for section in ["dependencies", "build-dependencies"] {
        entries.extend(testing_dependencies(section, manifest.get(section)));
    }
    if let Some(targets) = manifest.get("target").and_then(Value::as_table) {
        for (target, tables) in targets {
            for section in ["dependencies", "build-dependencies"] {
                entries.extend(testing_dependencies(
                    &format!("target.{target}.{section}"),
                    tables.get(section),
                ));
            }
        }
    }
    entries.extend(testing_dependencies(
        "workspace.dependencies",
        manifest
            .get("workspace")
            .and_then(|w| w.get("dependencies")),
    ));
    if let Some(features) = manifest.get("features").and_then(Value::as_table) {
        for (feature, enables) in features {
            if feature == TESTING {
                continue;
            }
            let enables_testing = enables
                .as_array()
                .is_some_and(|list| list.iter().filter_map(Value::as_str).any(names_testing));
            if enables_testing {
                entries.push(format!("features.{feature}"));
            }
        }
    }
    entries
}

#[test]
fn no_release_build_turns_on_the_testing_feature() {
    let offenders: Vec<String> = manifests()
        .iter()
        .flat_map(|path| {
            release_testing_entries(&read_manifest(path))
                .into_iter()
                .map(move |entry| format!("{path}: {entry}"))
        })
        .collect();
    assert!(
        offenders.is_empty(),
        "turn on the testing feature in [dev-dependencies] only: {offenders:#?}"
    );
}

#[test]
fn the_check_finds_each_release_entry() {
    let manifest: Value = toml::from_str(
        r#"
        [dependencies]
        fallow-engine = { workspace = true, features = ["testing"] }
        fallow-config = { workspace = true }
        [target.'cfg(unix)'.dependencies]
        fallow-api = { workspace = true, features = ["schema", "testing"] }
        [workspace.dependencies]
        fallow-config = { path = "crates/config", features = ["testing"] }
        [features]
        default = ["testing"]
        schema = ["fallow-api?/testing"]
        testing = ["fallow-engine/testing"]
        [dev-dependencies]
        fallow-api = { workspace = true, features = ["testing"] }
        "#,
    )
    .expect("parse");
    let mut entries = release_testing_entries(&manifest);
    entries.sort();
    assert_eq!(
        entries,
        [
            "dependencies.fallow-engine",
            "features.default",
            "features.schema",
            "target.cfg(unix).dependencies.fallow-api",
            "workspace.dependencies.fallow-config",
        ]
    );
}
