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
/// ignores the `audit` keys and runs duplication, so only the bare run has a
/// full selection.
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

/// When the project has duplication on, `--skip dupes` narrows the run, so
/// neither run records.
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

const DUPLICATES_OFF: &str = r#"{"duplicates": {"enabled": false}}"#;

/// Write a committed project with one clone group (`src/first.ts` and
/// `src/second.ts`) to `dir` and turn Impact on in a fresh home.
fn clone_project(dir: &Path, config: Option<&str>) -> tempfile::TempDir {
    let home = tempfile::tempdir().expect("home tempdir");
    std::fs::create_dir_all(dir.join("src")).expect("create src");
    if let Some(config) = config {
        std::fs::write(dir.join(".fallowrc.json"), config).expect("write config");
    }
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
    home
}

/// Run bare `fallow` on `project` with `extra` args, then read the store.
fn bare_run(home: &Path, project: &Path, extra: &[&str]) -> serde_json::Value {
    let root = project.to_str().expect("utf8 root");
    let bare = ["--root", root, "--format", "json", "--quiet"];
    run_recording(home, &[&bare[..], extra].concat());
    read_store_json(home)
}

/// The clone groups in the clone frontier of every worktree of `store`.
fn clone_groups(store: &serde_json::Value) -> usize {
    store["clone_frontier"].as_object().map_or(0, |worktrees| {
        worktrees
            .values()
            .filter_map(serde_json::Value::as_object)
            .map(serde_json::Map::len)
            .sum()
    })
}

fn record_count(store: &serde_json::Value, field: &str) -> usize {
    store[field].as_array().map_or(0, Vec::len)
}

fn resolved_kinds(store: &serde_json::Value) -> Vec<String> {
    store["recent_resolved"]
        .as_array()
        .expect("recent_resolved")
        .iter()
        .filter_map(|event| event["kind"].as_str().map(ToOwned::to_owned))
        .collect()
}

/// Assert that the bare run stored the clone group and credited nothing.
fn assert_seeded(store: &serde_json::Value) {
    assert_eq!(record_count(store, "project_records"), 1, "{store:#}");
    assert_eq!(clone_groups(store), 1, "{store:#}");
    assert_eq!(store["resolved_total"], 0, "{store:#}");
}

/// Assert that a record without duplication credited no clone group and kept
/// the stored clone group.
fn assert_clone_frontier_kept(store: &serde_json::Value) {
    assert_eq!(store["resolved_total"], 0, "{store:#}");
    assert!(resolved_kinds(store).is_empty(), "{store:#}");
    assert_eq!(clone_groups(store), 1, "{store:#}");
}

/// With `duplicates.enabled: false`, a run with `--only dead-code,dupes,health`
/// still runs duplication and fills the clone frontier. A later `--skip dupes`
/// record does not run duplication, so it credits no clone group as resolved
/// and keeps the stored clone frontier.
#[test]
fn skip_dupes_record_keeps_clones_that_an_earlier_run_stored() {
    let Some(project) = project_outside_temp_folder("impact-skip-dupes-seeded-clones-") else {
        return;
    };
    let home = clone_project(project.path(), Some(DUPLICATES_OFF));
    let seeded = bare_run(
        home.path(),
        project.path(),
        &["--only", "dead-code,dupes,health"],
    );
    assert_seeded(&seeded);

    let store = bare_run(home.path(), project.path(), &["--skip", "dupes"]);
    assert_eq!(record_count(&store, "project_records"), 2, "{store:#}");
    assert_clone_frontier_kept(&store);
}

/// A bare run stores the clone group. After `duplicates.enabled: false`, an
/// audit record of the clone files does not run duplication, so it credits no
/// clone group as resolved.
#[test]
fn audit_with_duplication_off_keeps_clones_that_a_bare_run_stored() {
    let Some(project) = project_outside_temp_folder("impact-audit-dupes-off-clones-") else {
        return;
    };
    let home = clone_project(project.path(), None);
    let dir = project.path();
    assert_seeded(&bare_run(home.path(), dir, &[]));

    std::fs::write(dir.join(".fallowrc.json"), DUPLICATES_OFF).expect("write config");
    for name in ["first", "second"] {
        let path = dir.join(format!("src/{name}.ts"));
        let source = format!("{}export const {name}Edited = 1;\n", cloned_function(name));
        std::fs::write(path, source).expect("edit clone file");
    }
    let root = dir.to_str().expect("utf8 root");
    run_recording(
        home.path(),
        &[
            "audit", "--root", root, "--base", "HEAD", "--format", "json", "--quiet",
        ],
    );
    let store = read_store_json(home.path());
    assert_eq!(record_count(&store, "records"), 1, "{store:#}");
    assert_clone_frontier_kept(&store);
}

/// A bare run stores the clone group. After `duplicates.enabled: false`, the
/// next bare run does not run duplication, so it credits no clone group.
#[test]
fn bare_run_with_duplication_off_keeps_clones_that_an_earlier_run_stored() {
    let Some(project) = project_outside_temp_folder("impact-bare-dupes-off-clones-") else {
        return;
    };
    let home = clone_project(project.path(), None);
    let dir = project.path();
    assert_seeded(&bare_run(home.path(), dir, &[]));

    std::fs::write(dir.join(".fallowrc.json"), DUPLICATES_OFF).expect("write config");
    let store = bare_run(home.path(), dir, &[]);
    assert_eq!(record_count(&store, "project_records"), 2, "{store:#}");
    assert_clone_frontier_kept(&store);
}

/// Control: with duplication on, the removal of one clone file resolves the
/// clone group.
#[test]
fn bare_run_credits_a_clone_group_that_the_user_removed() {
    let Some(project) = project_outside_temp_folder("impact-bare-clone-removed-") else {
        return;
    };
    let home = clone_project(project.path(), None);
    let dir = project.path();
    assert_seeded(&bare_run(home.path(), dir, &[]));

    std::fs::remove_file(dir.join("src/second.ts")).expect("remove second");
    std::fs::write(
        dir.join("src/index.ts"),
        "import { first } from './first';\nexport const used = [first];\n",
    )
    .expect("write index");
    let store = bare_run(home.path(), dir, &[]);
    assert_eq!(record_count(&store, "project_records"), 2, "{store:#}");
    assert_eq!(store["resolved_total"], 1, "{store:#}");
    assert_eq!(resolved_kinds(&store), ["code-duplication"], "{store:#}");
    assert_eq!(clone_groups(&store), 0, "{store:#}");
}

/// A run with duplication off keeps the clone frontier. When the user turns
/// duplication on again and the clone is still there, the next run credits
/// nothing.
#[test]
fn bare_run_after_duplication_on_again_credits_no_clone_that_is_still_there() {
    let Some(project) = project_outside_temp_folder("impact-bare-dupes-on-again-") else {
        return;
    };
    let home = clone_project(project.path(), None);
    let dir = project.path();
    assert_seeded(&bare_run(home.path(), dir, &[]));

    let config = dir.join(".fallowrc.json");
    std::fs::write(&config, DUPLICATES_OFF).expect("write config");
    assert_clone_frontier_kept(&bare_run(home.path(), dir, &[]));

    std::fs::write(&config, r#"{"duplicates": {"enabled": true}}"#).expect("write config");
    let store = bare_run(home.path(), dir, &[]);
    assert_eq!(record_count(&store, "project_records"), 3, "{store:#}");
    assert_clone_frontier_kept(&store);
}

/// The user removes every file of a stored clone group while duplication is
/// off. The group stays in the clone frontier, and the next run with
/// duplication on credits the removal.
#[test]
fn clone_removal_with_duplication_off_is_credited_when_duplication_is_on_again() {
    let Some(project) = project_outside_temp_folder("impact-clone-removed-while-off-") else {
        return;
    };
    let home = clone_project(project.path(), None);
    let dir = project.path();
    assert_seeded(&bare_run(home.path(), dir, &[]));

    let config = dir.join(".fallowrc.json");
    std::fs::write(&config, DUPLICATES_OFF).expect("write config");
    std::fs::remove_file(dir.join("src/first.ts")).expect("remove first");
    std::fs::remove_file(dir.join("src/second.ts")).expect("remove second");
    std::fs::write(dir.join("src/index.ts"), "export const used = [1];\n").expect("write index");
    assert_clone_frontier_kept(&bare_run(home.path(), dir, &[]));

    std::fs::write(&config, r#"{"duplicates": {"enabled": true}}"#).expect("write config");
    let store = bare_run(home.path(), dir, &[]);
    assert_eq!(store["resolved_total"], 1, "{store:#}");
    assert_eq!(resolved_kinds(&store), ["code-duplication"], "{store:#}");
    assert_eq!(clone_groups(&store), 0, "{store:#}");
}
