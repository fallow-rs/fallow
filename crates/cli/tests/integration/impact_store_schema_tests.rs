//! An older binary must not rewrite an Impact store that a newer Fallow wrote.

use std::fmt::Write as _;
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
    record_counts_after_runs_with_config(home, root, None)
}

/// [`record_counts_after_runs`] on a project with `config` as its
/// `.fallowrc.json`.
fn record_counts_after_runs_with_config(
    home: &Path,
    root: &Path,
    config: Option<&str>,
) -> (usize, usize) {
    record_counts_after_runs_with_audit_args(home, root, config, &[])
}

/// [`record_counts_after_runs_with_config`] with `audit_args` added to the
/// `fallow audit` command line.
fn record_counts_after_runs_with_audit_args(
    home: &Path,
    root: &Path,
    config: Option<&str>,
    audit_args: &[&str],
) -> (usize, usize) {
    record_counts_after_runs_with_args(home, root, config, audit_args, &[])
}

/// [`record_counts_after_runs_with_audit_args`] with `bare_args` added to the
/// bare `fallow` command line.
fn record_counts_after_runs_with_args(
    home: &Path,
    root: &Path,
    config: Option<&str>,
    audit_args: &[&str],
    bare_args: &[&str],
) -> (usize, usize) {
    std::fs::create_dir_all(root.join("src")).expect("create src");
    if let Some(config) = config {
        std::fs::write(root.join(".fallowrc.json"), config).expect("write config");
    }
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

    let mut audit = vec![
        "audit", "--root", root, "--base", "HEAD", "--format", "json", "--quiet",
    ];
    audit.extend_from_slice(audit_args);
    let mut bare = vec!["--root", root, "--format", "json", "--quiet"];
    bare.extend_from_slice(bare_args);
    for args in [audit, bare] {
        run_recording(home, &args);
    }

    let store = read_store_json(home);
    let count = |field: &str| store[field].as_array().map_or(0, Vec::len);
    (count("records"), count("project_records"))
}

/// Run fallow with `args` outside CI, with the isolated config dirs of `home`,
/// so that the record gate can write the Impact store.
fn run_recording(home: &Path, args: &[&str]) {
    let mut cmd = std::process::Command::new(super::common::fallow_bin());
    cmd.env("RUST_LOG", "").env("NO_COLOR", "1");
    super::common::scrub_analysis_env(&mut cmd);
    for name in CI_ENV_VARS {
        cmd.env_remove(name);
    }
    for (key, value) in isolated_env(home) {
        cmd.env(key, value);
    }
    let output = cmd.args(args).output().expect("run fallow");
    assert!(
        output.status.code().is_some_and(|code| code < 2),
        "{args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn read_store_json(home: &Path) -> serde_json::Value {
    serde_json::from_str(&read(&find_store(home))).expect("store json")
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

/// `duplicates.enabled: false` turns the duplication analysis off in both
/// commands. That is no narrowing of one run, so both runs still record.
#[test]
fn runs_with_duplication_off_in_the_config_record_impact() {
    let target_tmp = Path::new(env!("CARGO_TARGET_TMPDIR"));
    let temp = std::env::temp_dir();
    let canonical = |path: &Path| dunce::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    if canonical(target_tmp).starts_with(canonical(&temp))
        || (cfg!(unix) && canonical(target_tmp).starts_with(canonical(Path::new("/tmp"))))
    {
        // The gate skips every project in this target folder.
        return;
    }
    let home = tempfile::tempdir().expect("home tempdir");
    let project = tempfile::Builder::new()
        .prefix("impact-no-duplication-")
        .tempdir_in(target_tmp)
        .expect("project dir in target tmp");

    assert_eq!(
        record_counts_after_runs_with_config(
            home.path(),
            project.path(),
            Some(r#"{"duplicates": {"enabled": false}}"#),
        ),
        (1, 1)
    );
}

/// A project dir in the cargo target folder, or `None` when that folder is in
/// the temp folder. The record gate skips every project in the temp folder,
/// so a test of a record has nothing to prove there.
fn project_outside_temp_folder(prefix: &str) -> Option<tempfile::TempDir> {
    let target_tmp = Path::new(env!("CARGO_TARGET_TMPDIR"));
    let canonical = |path: &Path| dunce::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    if canonical(target_tmp).starts_with(canonical(&std::env::temp_dir()))
        || (cfg!(unix) && canonical(target_tmp).starts_with(canonical(Path::new("/tmp"))))
    {
        return None;
    }
    Some(
        tempfile::Builder::new()
            .prefix(prefix)
            .tempdir_in(target_tmp)
            .expect("project dir in target tmp"),
    )
}

/// `audit.dupes: false` skips duplication in the audit only. The bare run
/// ignores the `audit` keys and fills the clone history that both runs share,
/// so an audit record would credit those clones as resolved.
#[test]
fn audit_dupes_false_records_the_bare_run_only() {
    let Some(project) = project_outside_temp_folder("impact-audit-dupes-off-") else {
        return;
    };
    let home = tempfile::tempdir().expect("home tempdir");

    assert_eq!(
        record_counts_after_runs_with_config(
            home.path(),
            project.path(),
            Some(r#"{"audit": {"dupes": false}}"#),
        ),
        (0, 1)
    );
}

/// `--skip architecture` narrows the dead-code findings, so the audit records
/// nothing.
#[test]
fn audit_skip_architecture_records_no_audit_run() {
    let Some(project) = project_outside_temp_folder("impact-skip-architecture-") else {
        return;
    };
    let home = tempfile::tempdir().expect("home tempdir");

    assert_eq!(
        record_counts_after_runs_with_audit_args(
            home.path(),
            project.path(),
            None,
            &["--skip", "architecture"],
        ),
        (0, 1)
    );
}

/// With `duplicates.enabled: false`, the project has no duplication analysis.
/// `--skip dupes` then narrows nothing, so the audit still records.
#[test]
fn audit_skip_dupes_records_when_duplication_is_off_in_the_config() {
    let Some(project) = project_outside_temp_folder("impact-skip-dupes-off-") else {
        return;
    };
    let home = tempfile::tempdir().expect("home tempdir");

    assert_eq!(
        record_counts_after_runs_with_audit_args(
            home.path(),
            project.path(),
            Some(r#"{"duplicates": {"enabled": false}}"#),
            &["--skip", "dupes"],
        ),
        (1, 1)
    );
}

/// With `duplicates.enabled: false`, `--skip dupes` narrows nothing in either
/// command, so both runs record.
#[test]
fn skip_dupes_records_both_runs_when_duplication_is_off_in_the_config() {
    let Some(project) = project_outside_temp_folder("impact-both-skip-dupes-off-") else {
        return;
    };
    let home = tempfile::tempdir().expect("home tempdir");

    assert_eq!(
        record_counts_after_runs_with_args(
            home.path(),
            project.path(),
            Some(r#"{"duplicates": {"enabled": false}}"#),
            &["--skip", "dupes"],
            &["--skip", "dupes"],
        ),
        (1, 1)
    );
}

/// `--only dead-code,health` selects the same analyses as `--skip dupes`, so
/// both runs record when `duplicates.enabled` is false.
#[test]
fn only_dead_code_and_health_records_both_runs_when_duplication_is_off_in_the_config() {
    let Some(project) = project_outside_temp_folder("impact-both-only-dupes-off-") else {
        return;
    };
    let home = tempfile::tempdir().expect("home tempdir");

    assert_eq!(
        record_counts_after_runs_with_args(
            home.path(),
            project.path(),
            Some(r#"{"duplicates": {"enabled": false}}"#),
            &["--only", "dead-code,health"],
            &["--only", "dead-code,health"],
        ),
        (1, 1)
    );
}

/// When the project has duplication on, `--skip dupes` narrows the run. A
/// record would credit the clones of earlier runs as resolved, so neither
/// run records.
#[test]
fn skip_dupes_records_no_run_when_duplication_is_on() {
    let Some(project) = project_outside_temp_folder("impact-both-skip-dupes-on-") else {
        return;
    };
    let home = tempfile::tempdir().expect("home tempdir");

    assert_eq!(
        record_counts_after_runs_with_args(
            home.path(),
            project.path(),
            None,
            &["--skip", "dupes"],
            &["--skip", "dupes"],
        ),
        (0, 0)
    );
}

/// A function body long enough to form a clone group when two files hold it.
fn cloned_function(name: &str) -> String {
    let mut source = format!("export function {name}(items: number[]) {{\n  const out = [];\n");
    for step in 0..12 {
        writeln!(
            source,
            "  out.push(items.map((item) => item * {step}).filter((value) => value > {step}));"
        )
        .expect("write to String");
    }
    source.push_str("  return out;\n}\n");
    source
}

/// With `duplicates.enabled: false`, a run with `--only dead-code,dupes,health`
/// still runs duplication and fills the clone frontier. A later `--skip dupes`
/// record has no clones, so it credits each stored clone group as resolved.
#[test]
fn skip_dupes_record_credits_clones_that_an_earlier_run_stored() {
    let Some(project) = project_outside_temp_folder("impact-skip-dupes-seeded-clones-") else {
        return;
    };
    let home = tempfile::tempdir().expect("home tempdir");
    let dir = project.path();
    std::fs::create_dir_all(dir.join("src")).expect("create src");
    std::fs::write(
        dir.join(".fallowrc.json"),
        r#"{"duplicates": {"enabled": false}}"#,
    )
    .expect("write config");
    std::fs::write(
        dir.join("package.json"),
        r#"{"name":"impact-seeded-clones","main":"src/index.ts"}"#,
    )
    .expect("write package.json");
    std::fs::write(
        dir.join("src/index.ts"),
        "import { first } from './first';\nimport { second } from './second';\nexport const used = [first, second];\n",
    )
    .expect("write index");
    std::fs::write(dir.join("src/first.ts"), cloned_function("first")).expect("write first");
    std::fs::write(dir.join("src/second.ts"), cloned_function("second")).expect("write second");
    super::common::git(dir, &["init", "-q", "-b", "main"]);
    super::common::commit_all(dir, "init");
    let root = dir.to_str().expect("utf8 root");
    let enable = run_isolated(
        home.path(),
        &["impact", "enable", "--root", root, "--quiet"],
    );
    assert_eq!(enable.code, 0, "enable failed: {}", enable.stderr);
    let bare = ["--root", root, "--format", "json", "--quiet"];
    let clone_groups = |store: &serde_json::Value| -> usize {
        store["clone_frontier"].as_object().map_or(0, |worktrees| {
            worktrees
                .values()
                .filter_map(serde_json::Value::as_object)
                .map(serde_json::Map::len)
                .sum()
        })
    };

    run_recording(
        home.path(),
        &[&bare[..], &["--only", "dead-code,dupes,health"]].concat(),
    );
    let seeded = read_store_json(home.path());
    assert_eq!(
        seeded["project_records"].as_array().map_or(0, Vec::len),
        1,
        "{seeded:#}"
    );
    assert_eq!(clone_groups(&seeded), 1, "{seeded:#}");
    assert_eq!(seeded["resolved_total"], 0, "{seeded:#}");

    run_recording(home.path(), &[&bare[..], &["--skip", "dupes"]].concat());
    let store = read_store_json(home.path());
    assert_eq!(
        store["project_records"].as_array().map_or(0, Vec::len),
        2,
        "{store:#}"
    );
    assert_eq!(clone_groups(&store), 0, "{store:#}");
    assert_eq!(store["resolved_total"], 1, "{store:#}");
    let kinds: Vec<&str> = store["recent_resolved"]
        .as_array()
        .expect("recent_resolved")
        .iter()
        .filter_map(|event| event["kind"].as_str())
        .collect();
    assert_eq!(kinds, ["code-duplication"], "{store:#}");
}
