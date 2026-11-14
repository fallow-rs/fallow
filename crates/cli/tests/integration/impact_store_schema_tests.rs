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

/// After `npx fallow impact enable`, the tip names `npx fallow init --hooks`,
/// because a bare `fallow` is usually not on `PATH`.
#[test]
fn impact_enable_tip_uses_npx_under_npm_exec() {
    let home = tempfile::tempdir().expect("home tempdir");
    let project = tempfile::tempdir().expect("project tempdir");
    let root = project.path().to_str().expect("utf8 root");
    let mut env = isolated_env(home.path());
    env.push(("npm_command", "exec".to_owned()));
    let env: Vec<(&str, &str)> = env.iter().map(|(k, v)| (*k, v.as_str())).collect();

    let output = run_fallow_raw_with_env(&["impact", "enable", "--root", root], &env);

    assert_eq!(output.code, 0, "enable failed: {}", output.stderr);
    assert!(
        output.stdout.contains("Tip: run `npx fallow init --hooks`"),
        "the tip must name the npx form: {}",
        output.stdout
    );
}

/// The env vars whose presence marks a CI run. The record gate skips CI runs,
/// so a test of the record path must remove them.
const CI_ENV_VARS: [&str; 3] = ["CI", "GITHUB_ACTIONS", "GITLAB_CI"];

/// Record counts of the store after one `fallow audit` and one bare `fallow`
/// run on a committed git project at `root`: `(records, project_records)`.
fn record_counts_after_runs(home: &Path, root: &Path) -> (usize, usize) {
    std::fs::create_dir_all(root.join("src")).expect("create src");
    std::fs::write(
        root.join("package.json"),
        r#"{"name":"impact-temp-root","main":"src/index.ts"}"#,
    )
    .expect("write package.json");
    std::fs::write(root.join("src/index.ts"), "export const used = 1;\n").expect("write index");
    super::common::git(root, &["init", "-q", "-b", "main"]);
    super::common::commit_all(root, "init");
    let root = root.to_str().expect("utf8 root");

    let enable = run_isolated(home, &["impact", "enable", "--root", root, "--quiet"]);
    assert_eq!(enable.code, 0, "enable failed: {}", enable.stderr);

    for args in [
        vec![
            "audit", "--root", root, "--base", "HEAD", "--format", "json", "--quiet",
        ],
        vec!["--root", root, "--format", "json", "--quiet"],
    ] {
        let mut cmd = std::process::Command::new(super::common::fallow_bin());
        cmd.env("RUST_LOG", "").env("NO_COLOR", "1");
        super::common::scrub_analysis_env(&mut cmd);
        for name in CI_ENV_VARS {
            cmd.env_remove(name);
        }
        for (key, value) in isolated_env(home) {
            cmd.env(key, value);
        }
        let output = cmd.args(&args).output().expect("run fallow");
        assert!(
            output.status.code().is_some_and(|code| code < 2),
            "{args:?} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let store: serde_json::Value =
        serde_json::from_str(&read(&find_store(home))).expect("store json");
    let count = |field: &str| store[field].as_array().map_or(0, Vec::len);
    (count("records"), count("project_records"))
}

/// A project in the system temp folder is a test fixture or a throwaway clone.
/// A run there must not add a record to the user's Impact store (#4555).
#[test]
fn runs_in_temp_folder_record_no_impact() {
    let home = tempfile::tempdir().expect("home tempdir");
    let project = tempfile::tempdir().expect("project tempdir");

    assert_eq!(
        record_counts_after_runs(home.path(), project.path()),
        (0, 0)
    );
}

/// Control for the test above: the same runs outside the temp folder record.
#[test]
fn runs_outside_temp_folder_record_impact() {
    let target_tmp = Path::new(env!("CARGO_TARGET_TMPDIR"));
    let temp = std::env::temp_dir();
    let canonical = |path: &Path| dunce::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    if canonical(target_tmp).starts_with(canonical(&temp))
        || (cfg!(unix) && canonical(target_tmp).starts_with(canonical(Path::new("/tmp"))))
    {
        // The gate skips every project in this target folder, so the control
        // has nothing to prove here.
        return;
    }
    let home = tempfile::tempdir().expect("home tempdir");
    let project = tempfile::Builder::new()
        .prefix("impact-record-")
        .tempdir_in(target_tmp)
        .expect("project dir in target tmp");

    assert_eq!(
        record_counts_after_runs(home.path(), project.path()),
        (1, 1)
    );
}
