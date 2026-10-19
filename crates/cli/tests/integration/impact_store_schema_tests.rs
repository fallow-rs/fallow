//! An older binary must not rewrite an Impact store that a newer Fallow wrote.

use std::path::{Path, PathBuf};

use super::common::{CommandOutput, run_fallow_raw_with_env};

const FUTURE_SCHEMA: u32 = 999;

/// Isolated user config dirs for every platform the store path resolves on.
fn isolated_env(home: &Path) -> Vec<(&'static str, String)> {
    let utf8 = |path: &Path| path.to_str().expect("utf8 path").to_owned();
    vec![
        ("HOME", utf8(home)),
        ("USERPROFILE", utf8(home)),
        ("APPDATA", utf8(&home.join("AppData"))),
        ("XDG_CONFIG_HOME", utf8(&home.join(".config"))),
    ]
}

fn run_isolated(home: &Path, args: &[&str]) -> CommandOutput {
    let env = isolated_env(home);
    let env: Vec<(&str, &str)> = env.iter().map(|(k, v)| (*k, v.as_str())).collect();
    run_fallow_raw_with_env(args, &env)
}

/// The single per-project store file under the isolated config dir.
fn find_store(home: &Path) -> PathBuf {
    let bases = [
        home.join("Library").join("Application Support"),
        home.join(".config"),
        home.join("AppData"),
    ];
    let mut found = Vec::new();
    for base in bases {
        let Ok(entries) = std::fs::read_dir(base.join("fallow").join("impact")) else {
            continue;
        };
        found.extend(
            entries
                .flatten()
                .map(|entry| entry.path())
                .filter(|path| path.extension().is_some_and(|ext| ext == "json")),
        );
    }
    assert_eq!(found.len(), 1, "expected one store file, found {found:?}");
    found.remove(0)
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).expect("read store")
}

#[test]
fn older_binary_leaves_a_newer_schema_store_unchanged() {
    let home = tempfile::tempdir().expect("home tempdir");
    let project = tempfile::tempdir().expect("project tempdir");
    let root = project.path().to_str().expect("utf8 root");

    let enable = run_isolated(
        home.path(),
        &["impact", "enable", "--root", root, "--quiet"],
    );
    assert_eq!(enable.code, 0, "seed enable failed: {}", enable.stderr);
    let store = find_store(home.path());

    let future = format!(
        r#"{{"schema_version":{FUTURE_SCHEMA},"enabled":false,"records":[],"future_field":{{"sentinel":"preserve"}}}}"#
    );
    std::fs::write(&store, &future).expect("seed future store");

    for args in [
        [
            "impact", "disable", "--root", root, "--format", "json", "--quiet",
        ],
        [
            "impact", "enable", "--root", root, "--format", "json", "--quiet",
        ],
    ] {
        let output = run_isolated(home.path(), &args);
        assert_eq!(
            output.code, 2,
            "{args:?} must refuse a newer store: {} {}",
            output.stdout, output.stderr
        );
        let text = format!("{}{}", output.stdout, output.stderr);
        assert!(
            text.contains(&format!("schema_version {FUTURE_SCHEMA}")),
            "{args:?} must name the store schema: {text}"
        );
        assert_eq!(
            read(&store),
            future,
            "{args:?} must leave the store unchanged"
        );
    }

    let decline = run_isolated(
        home.path(),
        &["init", "--decline", "--root", root, "--quiet"],
    );
    assert_eq!(decline.code, 2, "init --decline must refuse a newer store");
    assert_eq!(read(&store), future);

    let status = run_isolated(
        home.path(),
        &["impact", "--root", root, "--format", "json", "--quiet"],
    );
    assert_eq!(status.code, 0, "reads stay available: {}", status.stderr);
    assert_eq!(read(&store), future);
}
