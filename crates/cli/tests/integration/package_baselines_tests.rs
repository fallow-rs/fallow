use crate::common::{
    CommandOutput, commit_all, copy_fixture, git, parse_json, run_fallow_in_root, run_fallow_raw,
    run_fallow_raw_with_env, run_fallow_raw_with_type_aware_sidecar,
};
use std::fs;
use std::path::Path;

#[test]
fn package_baselines_keep_complete_cycles_touching_an_eligible_import() {
    let temp = copy_fixture("package-cycle-workspace");
    let root = temp.path();
    fs::write(
        root.join(".fallowrc.json"),
        r#"{"workspaces":{"changedSince":{"packages/a":"HEAD","packages/b":"HEAD"}}}"#,
    )
    .expect("config");
    git(root, &["init", "-q"]);
    commit_all(root, "base");

    let report = || {
        let output = run_fallow_in_root(
            "dead-code",
            root,
            &[
                "--package-cycles",
                "--format",
                "json",
                "--quiet",
                "--no-cache",
            ],
        );
        assert_eq!(output.code, 0, "{}", output.stderr);
        parse_json(&output)
    };
    assert_eq!(report()["package_cycles"], serde_json::json!([]));

    fs::write(
        root.join("packages/a/src/x.ts"),
        "import { y } from '@repro/b/y';\nexport const x = () => y() + '!';\n",
    )
    .expect("changed import owner");
    let changed = report();
    assert_eq!(
        changed["package_cycles"].as_array().expect("cycles").len(),
        1
    );
    assert_eq!(
        changed["package_cycles"][0]["packages"],
        serde_json::json!(["@repro/a", "@repro/b"])
    );
    assert_eq!(
        changed["package_cycles"][0]["edges"]
            .as_array()
            .expect("edges")
            .len(),
        2
    );
}

fn write_config(root: &Path, web_ref: &str) {
    fs::write(
        root.join(".fallowrc.json"),
        format!(
            r#"{{"workspaces":{{"changedSince":{{"packages/web":"{web_ref}","packages/legacy":"HEAD"}}}}}}"#
        ),
    )
    .expect("config");
}

fn unused_export_paths(root: &Path, args: &[&str]) -> Vec<String> {
    let mut command = vec!["check", "--root", root.to_str().expect("root path")];
    command.extend_from_slice(args);
    command.extend_from_slice(&["--format", "json", "--quiet"]);
    let output = run_fallow_raw(&command);
    assert!(
        output.code == 0 || output.code == 1,
        "check failed: {}",
        output.stderr
    );
    parse_json(&output)["unused_exports"]
        .as_array()
        .expect("unused exports")
        .iter()
        .filter_map(|finding| finding["path"].as_str())
        .map(|path| path.replace('\\', "/"))
        .collect()
}

#[test]
fn package_baselines_scope_each_workspace_and_global_ref_overrides() {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path();
    fs::write(
        root.join("package.json"),
        r#"{"name":"scope-root","private":true,"workspaces":["packages/*"]}"#,
    )
    .expect("root manifest");
    for name in ["web", "legacy"] {
        let package = root.join("packages").join(name);
        fs::create_dir_all(package.join("src")).expect("package source directory");
        fs::write(
            package.join("package.json"),
            format!(r#"{{"name":"{name}","main":"src/index.ts"}}"#),
        )
        .expect("package manifest");
        fs::write(
            package.join("src/index.ts"),
            "import { used } from './utils';\nused();\n",
        )
        .expect("entry");
        fs::write(
            package.join("src/utils.ts"),
            format!("export const used = () => 1;\nexport const unused_{name} = 1;\n"),
        )
        .expect("utilities");
    }
    write_config(root, "HEAD~1");
    git(root, &["init", "-q"]);
    git(root, &["add", "."]);
    git(
        root,
        &[
            "-c",
            "user.name=Fallow Test",
            "-c",
            "user.email=fallow@example.test",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "-qm",
            "base",
        ],
    );
    fs::write(
        root.join("packages/web/src/utils.ts"),
        "export const used = () => 2;\nexport const unused_web = 2;\n",
    )
    .expect("web change");
    git(root, &["add", "."]);
    git(
        root,
        &[
            "-c",
            "user.name=Fallow Test",
            "-c",
            "user.email=fallow@example.test",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "-qm",
            "web change",
        ],
    );
    fs::write(
        root.join("packages/legacy/src/utils.ts"),
        "export const used = () => 3;\nexport const unused_legacy = 3;\n",
    )
    .expect("legacy change");

    let paths = unused_export_paths(root, &[]);
    let report = run_fallow_raw(&[
        "check",
        "--root",
        root.to_str().unwrap(),
        "--format",
        "json",
        "--quiet",
    ]);
    let json = parse_json(&report);
    assert_eq!(
        json["package_baselines"],
        serde_json::json!([
            {"workspace_root":"packages/legacy","reference":"HEAD"},
            {"workspace_root":"packages/web","reference":"HEAD~1"}
        ])
    );
    assert_eq!(
        json["request_outcomes"]["package-baselines"],
        serde_json::json!({
            "status": "applied",
            "affects": "scope",
            "requested": "workspaces.changedSince"
        })
    );
    assert!(
        paths
            .iter()
            .any(|path| path.ends_with("packages/web/src/utils.ts"))
    );
    assert!(
        paths
            .iter()
            .any(|path| path.ends_with("packages/legacy/src/utils.ts"))
    );

    let query_report = run_fallow_raw(&[
        "check",
        "--root",
        root.to_str().unwrap(),
        "--finding-id",
        "dc1:unused-export:0000000000000000",
        "--format",
        "json",
        "--quiet",
        "--no-cache",
    ]);
    assert!(
        query_report.code == 0 || query_report.code == 1,
        "{}",
        query_report.stderr
    );
    let query_json = parse_json(&query_report);
    assert_eq!(query_json["package_baselines"], json["package_baselines"]);
    assert_eq!(query_json["finding_id_query"]["conclusive"], false);
    assert_eq!(
        query_json["finding_id_query"]["inconclusive_reasons"],
        serde_json::json!(["package-baselines"])
    );

    write_config(root, "missing-ref");
    let global_paths = unused_export_paths(root, &["--changed-since", "HEAD"]);
    let global_report = run_fallow_raw(&[
        "check",
        "--root",
        root.to_str().unwrap(),
        "--changed-since",
        "HEAD",
        "--format",
        "json",
        "--quiet",
    ]);
    assert!(
        parse_json(&global_report)
            .get("package_baselines")
            .is_none()
    );
    assert!(
        global_paths
            .iter()
            .any(|path| path.ends_with("packages/legacy/src/utils.ts"))
    );
    assert!(
        !global_paths
            .iter()
            .any(|path| path.ends_with("packages/web/src/utils.ts"))
    );

    // A well-formed ref that Git cannot resolve stands the map down, as an
    // unresolved `--changed-since` does: full scope, a warning, and a
    // `not-applied` request outcome.
    let stood_down = run_fallow_raw(&[
        "check",
        "--root",
        root.to_str().expect("root path"),
        "--format",
        "json",
        "--quiet",
    ]);
    assert_eq!(stood_down.code, 1, "{}", stood_down.stderr);
    assert!(
        stood_down
            .stderr
            .contains("workspaces.changedSince was ignored"),
        "{}",
        stood_down.stderr
    );
    let stood_down_json = parse_json(&stood_down);
    assert!(stood_down_json.get("package_baselines").is_none());
    let outcome = &stood_down_json["request_outcomes"]["package-baselines"];
    assert_eq!(outcome["status"], "not-applied");
    assert_eq!(outcome["affects"], "scope");
    assert_eq!(outcome["requested"], "workspaces.changedSince");
    assert_eq!(outcome["reason"], "git-failed");
    let full_scope_paths = unused_export_paths(root, &[]);
    for package in ["web", "legacy"] {
        assert!(
            full_scope_paths
                .iter()
                .any(|path| path.ends_with(&format!("packages/{package}/src/utils.ts"))),
            "{full_scope_paths:?}"
        );
    }

    // A malformed ref is invalid input, as it is for `--changed-since`.
    write_config(root, "-malformed");
    let failed = run_fallow_raw(&[
        "check",
        "--root",
        root.to_str().expect("root path"),
        "--format",
        "json",
        "--quiet",
    ]);
    assert_eq!(failed.code, 2, "{}", failed.stderr);
    assert!(
        failed.stdout.contains("Workspace baseline error"),
        "stdout: {}; stderr: {}",
        failed.stdout,
        failed.stderr
    );
}

/// Two workspace packages, `a` and `b`. Each has one used and one unused
/// export in `src/utils.ts`. The repository has one commit.
fn two_package_repository(config: &str) -> tempfile::TempDir {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path();
    fs::write(
        root.join("package.json"),
        r#"{"name":"scope-root","private":true,"workspaces":["packages/*"]}"#,
    )
    .expect("root manifest");
    for name in ["a", "b"] {
        let package = root.join("packages").join(name);
        fs::create_dir_all(package.join("src")).expect("package source directory");
        fs::write(
            package.join("package.json"),
            format!(r#"{{"name":"{name}","main":"src/index.ts"}}"#),
        )
        .expect("package manifest");
        fs::write(
            package.join("src/index.ts"),
            "import { used } from './utils';\nused();\n",
        )
        .expect("entry");
        fs::write(
            package.join("src/utils.ts"),
            format!("export const used = () => 1;\nexport const unused_{name} = 1;\n"),
        )
        .expect("utilities");
    }
    fs::write(root.join(".fallowrc.json"), config).expect("config");
    git(root, &["init", "-q"]);
    commit_all(root, "base");
    temp
}

fn run_in(root: &Path, args: &[&str]) -> CommandOutput {
    let mut command = args.to_vec();
    command.extend_from_slice(&["--root", root.to_str().expect("root path"), "--quiet"]);
    run_fallow_raw(&command)
}

/// Audit narrows head and base itself. The base snapshot is not a Git
/// repository, so a package map that audit read there would fail the run.
#[test]
fn audit_ignores_package_baselines_on_head_and_base() {
    let temp = two_package_repository(
        r#"{"workspaces":{"changedSince":{"packages/a":"HEAD","packages/b":"HEAD"}}}"#,
    );
    let root = temp.path();
    fs::write(
        root.join("packages/b/src/utils.ts"),
        "export const used = () => 2;\nexport const unused_b = 1;\n",
    )
    .expect("change the used function only");

    let output = run_in(root, &["audit", "--base", "HEAD", "--format", "json"]);
    assert_eq!(
        output.code, 0,
        "stdout: {}\nstderr: {}",
        output.stdout, output.stderr
    );
    let json = parse_json(&output);
    assert_eq!(json["verdict"], "pass", "{json}");
    let exports = json["dead_code"]["unused_exports"]
        .as_array()
        .expect("unused exports");
    assert_eq!(exports.len(), 1, "{json}");
    assert_eq!(exports[0]["export_name"], "unused_b");
    assert_eq!(exports[0]["introduced"], false, "{json}");
    assert!(
        json["dead_code"].get("package_baselines").is_none(),
        "{json}"
    );
}

/// A package map narrows the report the same way as a global ref, so a saved
/// full baseline is not stale because of it.
#[test]
fn package_baselines_mark_the_report_change_scoped_for_the_stale_baseline_gate() {
    let temp = two_package_repository("{}");
    let root = temp.path();
    let baseline = root.join("baseline.json");
    let baseline = baseline.to_str().expect("baseline path");
    let saved = run_in(root, &["dead-code", "--save-baseline", baseline]);
    assert!(saved.code == 0 || saved.code == 1, "{}", saved.stderr);
    fs::write(
        root.join(".fallowrc.json"),
        r#"{"workspaces":{"changedSince":{"packages/a":"HEAD","packages/b":"HEAD"}}}"#,
    )
    .expect("package map");
    commit_all(root, "baseline and package map");

    let global = run_in(
        root,
        &[
            "dead-code",
            "--changed-since",
            "HEAD",
            "--baseline",
            baseline,
            "--fail-on-stale-baseline",
            "--format",
            "json",
        ],
    );
    assert_eq!(global.code, 0, "control: {}", global.stderr);

    let mapped = run_in(
        root,
        &[
            "dead-code",
            "--baseline",
            baseline,
            "--fail-on-stale-baseline",
            "--format",
            "json",
        ],
    );
    assert_eq!(
        mapped.code, 0,
        "stdout: {}\nstderr: {}",
        mapped.stdout, mapped.stderr
    );
    let json = parse_json(&mapped);
    assert_eq!(
        json["package_baselines"],
        serde_json::json!([
            {"workspace_root":"packages/a","reference":"HEAD"},
            {"workspace_root":"packages/b","reference":"HEAD"}
        ])
    );
    assert_eq!(json["unused_exports"], serde_json::json!([]));
}

/// Type-aware reconciliation adds private-type leaks after the syntactic
/// pass. A leak in a package without changes stays out of the report.
#[test]
fn type_aware_leaks_follow_the_package_scope() {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path();
    let files = [
        (
            "package.json",
            r#"{"name":"leak-root","private":true,"workspaces":["packages/*"]}"#,
        ),
        (
            "tsconfig.json",
            r#"{"compilerOptions":{"strict":true,"module":"esnext","moduleResolution":"bundler","target":"es2022"},"include":["packages/*/src"]}"#,
        ),
        (
            ".fallowrc.json",
            r#"{"publicPackages":["a"],"rules":{"private-type-leaks":"warn"},"workspaces":{"changedSince":{"packages/a":"HEAD"}}}"#,
        ),
        (
            "packages/a/package.json",
            r#"{"name":"a","main":"src/index.ts"}"#,
        ),
        ("packages/a/src/index.ts", "export * from './lib';\n"),
        (
            "packages/a/src/lib.ts",
            "type Internal = { id: string };\nconst make = (): Internal => ({ id: 'a' });\nexport const build = () => make();\n",
        ),
    ];
    for (path, content) in files {
        let target = root.join(path);
        fs::create_dir_all(target.parent().expect("parent")).expect("directory");
        fs::write(target, content).expect("fixture file");
    }
    git(root, &["init", "-q"]);
    commit_all(root, "base");

    let run = |extra: &[&str]| {
        let mut args = vec![
            "dead-code",
            "--root",
            root.to_str().expect("root path"),
            "--type-aware",
            "--private-type-leaks",
            "--format",
            "json",
            "--quiet",
            "--no-cache",
        ];
        args.extend_from_slice(extra);
        let output = run_fallow_raw_with_type_aware_sidecar(&args);
        assert!(output.code == 0 || output.code == 1, "{}", output.stderr);
        parse_json(&output)
    };
    let control = {
        fs::write(
            root.join(".fallowrc.json"),
            r#"{"publicPackages":["a"],"rules":{"private-type-leaks":"warn"}}"#,
        )
        .expect("config without map");
        let json = run(&[]);
        fs::write(
            root.join(".fallowrc.json"),
            r#"{"publicPackages":["a"],"rules":{"private-type-leaks":"warn"},"workspaces":{"changedSince":{"packages/a":"HEAD"}}}"#,
        )
        .expect("restore package map");
        json
    };
    assert_eq!(
        control["private_type_leaks"].as_array().map(Vec::len),
        Some(1),
        "the unscoped run must see the leak: {control}"
    );

    let scoped = run(&[]);
    assert_eq!(
        scoped["package_baselines"],
        serde_json::json!([{"workspace_root":"packages/a","reference":"HEAD"}])
    );
    assert_eq!(
        scoped["private_type_leaks"],
        serde_json::json!([]),
        "a leak in an unchanged package must stay out of the report"
    );
}

/// A run from a package subdirectory loads the parent config. Its keys name no
/// workspace of that project, so the map stands down instead of failing the run.
#[test]
fn a_run_from_a_package_directory_stands_the_map_down() {
    let temp = two_package_repository(
        r#"{"workspaces":{"changedSince":{"packages/a":"HEAD","packages/b":"HEAD"}}}"#,
    );
    let package = temp.path().join("packages/b");
    let output = run_in(&package, &["dead-code", "--format", "json"]);
    assert_eq!(
        output.code, 1,
        "stdout: {}\nstderr: {}",
        output.stdout, output.stderr
    );
    assert!(
        output
            .stderr
            .contains("workspaces.changedSince was ignored"),
        "{}",
        output.stderr
    );
    let json = parse_json(&output);
    assert_eq!(
        json["request_outcomes"]["package-baselines"]["reason"],
        "unknown-workspace"
    );
    assert_eq!(json["unused_exports"][0]["export_name"], "unused_b");
}

/// The shape of a key is checked when the config loads, so every command
/// rejects it, including one that never reads the map.
#[test]
fn a_malformed_key_fails_every_command_at_config_load() {
    let temp = two_package_repository(r#"{"workspaces":{"changedSince":{"./packages/a":"HEAD"}}}"#);
    for command in ["dead-code", "health"] {
        let output = run_in(temp.path(), &[command, "--format", "json"]);
        assert_eq!(output.code, 2, "{command}: {}", output.stderr);
        let rendered = format!("{}{}", output.stdout, output.stderr);
        assert!(
            rendered.contains("is not an exact workspace root"),
            "{command}: {rendered}"
        );
    }
}

/// A baseline saved under the package map is partial. The save warns, the file
/// records the scope, and a wider run that loads it warns before it compares.
#[test]
fn a_baseline_saved_under_the_map_records_its_scope() {
    let temp = two_package_repository(
        r#"{"workspaces":{"changedSince":{"packages/a":"HEAD","packages/b":"HEAD"}}}"#,
    );
    let root = temp.path();
    let baseline = root.join("baseline.json");
    let baseline = baseline.to_str().expect("baseline path");

    let partial = run_in(
        root,
        &["dead-code", "--save-baseline", baseline, "--format", "json"],
    );
    assert!(
        partial
            .stderr
            .contains("Save it with --no-package-baselines"),
        "a partial save warns: {}",
        partial.stderr
    );
    let saved: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(baseline).expect("saved baseline"))
            .expect("baseline JSON");
    assert_eq!(
        saved["scope_reasons"],
        serde_json::json!(["package-baselines"])
    );
    let wider = run_in(
        root,
        &[
            "dead-code",
            "--no-package-baselines",
            "--baseline",
            baseline,
            "--format",
            "json",
        ],
    );
    assert!(
        wider
            .stderr
            .contains("was saved from a run narrowed by package-baselines"),
        "a wider run warns about the partial baseline: {}",
        wider.stderr
    );
    let narrowed = run_in(
        root,
        &["dead-code", "--baseline", baseline, "--format", "json"],
    );
    let narrowed_json = parse_json(&narrowed);
    assert_eq!(
        narrowed_json["baseline_staleness"]["scope_reasons"],
        serde_json::json!(["package-baselines"])
    );
    let recheck = narrowed_json["next_steps"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|step| step["id"] == "recheck-baseline");
    if let Some(step) = recheck {
        assert!(
            step["command"]
                .as_str()
                .is_some_and(|command| command.ends_with("--no-package-baselines")),
            "{step}"
        );
    }
}

/// The package map applies to every run, so a baseline saved under it is
/// partial. `--no-package-baselines` gives the whole-project run that can save
/// and gate a baseline.
#[test]
fn no_package_baselines_saves_and_gates_a_whole_project_baseline() {
    let temp = two_package_repository(
        r#"{"workspaces":{"changedSince":{"packages/a":"HEAD","packages/b":"HEAD"}}}"#,
    );
    let root = temp.path();
    let baseline = root.join("baseline.json");
    let baseline = baseline.to_str().expect("baseline path");

    let full = run_in(
        root,
        &[
            "dead-code",
            "--no-package-baselines",
            "--save-baseline",
            baseline,
            "--format",
            "json",
        ],
    );
    assert!(
        !full.stderr.contains("--no-package-baselines to cover"),
        "{}",
        full.stderr
    );
    let full_saved: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(baseline).expect("saved baseline"))
            .expect("baseline JSON");
    assert!(full_saved.get("scope_reasons").is_none(), "{full_saved}");
    let full_json = parse_json(&full);
    assert!(full_json.get("package_baselines").is_none());
    assert!(full_json.get("request_outcomes").is_none());
    let exports = full_json["unused_exports"]
        .as_array()
        .expect("unused exports");
    assert_eq!(
        exports.len(),
        2,
        "every package is in full scope: {full_json}"
    );

    // The gate runs on the whole-project run and trips on a stale entry.
    fs::write(
        root.join("packages/a/src/utils.ts"),
        "export const used = () => 1;\n",
    )
    .expect("remove the unused export");
    let gated = run_in(
        root,
        &[
            "dead-code",
            "--no-package-baselines",
            "--baseline",
            baseline,
            "--fail-on-stale-baseline",
            "--format",
            "json",
        ],
    );
    assert_eq!(gated.code, 1, "{}", gated.stderr);
    assert!(
        gated.stderr.contains("Baseline gate failed"),
        "{}",
        gated.stderr
    );
}

#[test]
fn security_rejects_no_package_baselines() {
    let temp = two_package_repository("{}");
    let output = run_in(
        temp.path(),
        &["security", "--no-package-baselines", "--format", "json"],
    );
    assert_eq!(output.code, 2, "{}", output.stderr);
}

/// `FALLOW_PACKAGE_BASELINES=false` turns the map off for every run of the
/// process, so a CI job that saves or gates a whole-project baseline sets it
/// once instead of passing the flag to each command.
#[test]
fn package_baselines_env_false_turns_the_map_off() {
    let temp = two_package_repository(
        r#"{"workspaces":{"changedSince":{"packages/a":"HEAD","packages/b":"HEAD"}}}"#,
    );
    let root = temp.path().to_str().expect("root path");
    let args = ["dead-code", "--root", root, "--format", "json", "--quiet"];
    let mapped = parse_json(&run_fallow_raw(&args));
    assert_eq!(mapped["unused_exports"], serde_json::json!([]));

    let output = run_fallow_raw_with_env(&args, &[("FALLOW_PACKAGE_BASELINES", "false")]);
    let json = parse_json(&output);
    assert!(json.get("package_baselines").is_none(), "{json}");
    assert_eq!(
        json["unused_exports"].as_array().map(Vec::len),
        Some(2),
        "every package is in full scope: {json}"
    );
}
