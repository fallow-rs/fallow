//! The config load contract of the CLI: the error text and the exit code of a
//! config that does not load, the `loaded config` line, the order of the env
//! overrides, the workspace diagnostics notice, the refusal of remote
//! `extends`, and the production mode of each analysis.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests use unwrap and expect to keep fixture setup concise"
)]

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;

use fallow_config::FallowConfig;

use crate::common::{CommandOutput, fallow_bin, git, parse_json, scrub_analysis_env};

/// The commands that load the config through the main CLI loader.
const ANALYSIS_COMMANDS: &[&[&str]] = &[
    &[],
    &["dead-code"],
    &["dupes"],
    &["health"],
    &["audit", "--base", "HEAD"],
    &["list"],
    &["flags"],
    &["security"],
];

/// The duplicated block of the production fixture. Production mode leaves
/// the story file out of the duplication analysis.
fn duplicated_block(name: &str) -> String {
    let mut block = format!("export function {name}(items: number[]): number {{\n");
    for i in 0..12 {
        writeln!(
            block,
            "  const v{i} = items.map((item) => item * {i} + items.length).filter((item) => item > {i}).reduce((acc, item) => acc + item, 0);"
        )
        .unwrap();
    }
    block.push_str("  return v0 + v11;\n}\n");
    block
}

/// A git project with one signal per analysis:
/// - dead code: an unused dev dependency, which production mode does not report;
/// - health: a complex test file, which production mode leaves out;
/// - duplication: a story file that copies a source file.
fn project() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(
        root.join("package.json"),
        r#"{"name":"config-load","main":"src/index.ts","devDependencies":{"left-pad":"1.0.0"}}"#,
    )
    .unwrap();
    std::fs::write(root.join("src/index.ts"), "export const ok = 1;\n").unwrap();
    let mut complex =
        String::from("export function complexTest(n: number): number {\n  let total = 0;\n");
    for i in 1..=25 {
        writeln!(complex, "  if (n > {i}) total++;").unwrap();
    }
    complex.push_str("  return total;\n}\n");
    std::fs::write(root.join("src/complex.test.ts"), complex).unwrap();
    std::fs::write(root.join("src/copy.ts"), duplicated_block("blockCopy")).unwrap();
    std::fs::write(
        root.join("src/copy.stories.ts"),
        duplicated_block("storyCopy"),
    )
    .unwrap();
    git(root, &["init", "-q"]);
    git(root, &["add", "."]);
    git(
        root,
        &["-c", "commit.gpgsign=false", "commit", "-m", "init", "-q"],
    );
    dir
}

fn canonical_root(dir: &tempfile::TempDir) -> PathBuf {
    dunce::canonicalize(dir.path()).expect("canonical root")
}

/// Run fallow in `root` with `args` and `envs`. The production variables of
/// the parent process do not reach the run.
fn run(root: &Path, args: &[&str], envs: &[(&str, &str)]) -> CommandOutput {
    let mut cmd = Command::new(fallow_bin());
    cmd.env("RUST_LOG", "").env("NO_COLOR", "1");
    scrub_analysis_env(&mut cmd);
    for name in [
        "FALLOW_PRODUCTION",
        "FALLOW_PRODUCTION_DEAD_CODE",
        "FALLOW_PRODUCTION_HEALTH",
        "FALLOW_PRODUCTION_DUPES",
        "FALLOW_MAX_FILE_SIZE",
        "FALLOW_CACHE_DIR",
        "FALLOW_CACHE_MAX_SIZE",
        "FALLOW_FORMAT",
        "FALLOW_QUIET",
    ] {
        cmd.env_remove(name);
    }
    cmd.envs(envs.iter().copied())
        .current_dir(root)
        .args(args)
        .arg("--root")
        .arg(root);
    let output = cmd.output().expect("run fallow");
    CommandOutput::from_process(output)
}

fn with_args<'a>(command: &[&'a str], extra: &[&'a str]) -> Vec<&'a str> {
    command.iter().chain(extra).copied().collect()
}

fn json_error(message: &str) -> String {
    serde_json::json!({ "error": true, "message": message, "exit_code": 2 }).to_string()
}

/// Each analysis command prints `message` once and exits 2: on stderr in
/// human mode, and as a JSON error object on stdout in JSON mode.
fn assert_config_error(root: &Path, extra: &[&str], message: &str) {
    for command in ANALYSIS_COMMANDS {
        let args = with_args(command, extra);
        let human = run(root, &args, &[]);
        assert_eq!(human.code, 2, "{args:?}: {}", human.stderr);
        assert_eq!(human.stdout, "", "{args:?}");
        assert_eq!(human.stderr, format!("Error: {message}\n"), "{args:?}");

        let json_args = with_args(&args, &["--format", "json"]);
        let json = run(root, &json_args, &[]);
        assert_eq!(json.code, 2, "{json_args:?}: {}", json.stderr);
        assert_eq!(
            json.stdout,
            format!("{}\n", json_error(message)),
            "{json_args:?}"
        );
        assert_eq!(json.stderr, "", "{json_args:?}");
    }

    let config = run(root, &with_args(&["config"], extra), &[]);
    assert_eq!(config.code, 2, "config: {}", config.stderr);
    assert_eq!(
        config.stdout,
        format!("{}\n", json_error(message)),
        "config"
    );
    assert_eq!(config.stderr, "", "config");
}

#[test]
fn a_discovered_config_with_a_syntax_error_stops_each_command() {
    let dir = project();
    let root = canonical_root(&dir);
    std::fs::write(root.join(".fallowrc.json"), "{ \"rules\": \n").unwrap();
    let message = FallowConfig::find_and_load(&root).expect_err("syntax error");
    assert_config_error(&root, &[], &message);
}

#[test]
fn a_discovered_config_with_a_wrong_value_type_stops_each_command() {
    let dir = project();
    let root = canonical_root(&dir);
    std::fs::write(root.join(".fallowrc.json"), r#"{ "production": "yes" }"#).unwrap();
    let message = FallowConfig::find_and_load(&root).expect_err("wrong type");
    assert_config_error(&root, &[], &message);
}

#[test]
fn an_explicit_config_that_does_not_load_names_its_path() {
    let dir = project();
    let root = canonical_root(&dir);
    let broken = root.join("broken.json");
    std::fs::write(&broken, r#"{ "production": "yes" }"#).unwrap();
    let missing = root.join("missing.json");
    for path in [&broken, &missing] {
        let cause = FallowConfig::load(path).expect_err("does not load");
        let message = format!("failed to load config '{}': {cause}", path.display());
        assert_config_error(&root, &["--config", path.to_str().unwrap()], &message);
    }
}

#[test]
fn an_invalid_plugin_file_stops_the_analysis_but_not_the_config_command() {
    let dir = project();
    let root = canonical_root(&dir);
    std::fs::create_dir_all(root.join(".fallow/plugins")).unwrap();
    std::fs::write(
        root.join(".fallow/plugins/escape.json"),
        r#"{ "name": "escape", "entryPoints": ["../outside/**"] }"#,
    )
    .unwrap();
    for command in ANALYSIS_COMMANDS {
        let output = run(&root, command, &[]);
        assert_eq!(output.code, 2, "{command:?}: {}", output.stderr);
        assert!(
            output
                .stderr
                .starts_with("Error: invalid external plugin definition:\n  - "),
            "{command:?}: {}",
            output.stderr
        );
        assert_eq!(output.stderr.matches("Error: ").count(), 1, "{command:?}");
    }
    let config = run(&root, &["config", "--quiet"], &[]);
    assert_eq!(config.code, 0, "config: {}", config.stderr);
}

fn loaded_config_lines(output: &CommandOutput) -> Vec<&str> {
    output
        .stderr
        .lines()
        .filter(|line| line.starts_with("loaded config: "))
        .collect()
}

#[test]
fn the_loaded_config_line_prints_once_in_human_mode_only() {
    let dir = project();
    let root = canonical_root(&dir);
    std::fs::write(root.join(".fallowrc.json"), "{}").unwrap();
    let expected = format!("loaded config: {}", root.join(".fallowrc.json").display());

    for command in ANALYSIS_COMMANDS {
        let human = run(&root, command, &[]);
        assert!(
            human.code == 0 || human.code == 1,
            "{command:?}: {}",
            human.stderr
        );
        // `fallow list` loads the config in quiet mode.
        let lines: &[&str] = if command == &["list"] {
            &[]
        } else {
            &[expected.as_str()]
        };
        assert_eq!(loaded_config_lines(&human), lines, "{command:?}");

        let quiet = run(&root, &with_args(command, &["--quiet"]), &[]);
        assert_eq!(
            loaded_config_lines(&quiet),
            Vec::<&str>::new(),
            "{command:?}"
        );

        let json = run(&root, &with_args(command, &["--format", "json"]), &[]);
        assert_eq!(
            loaded_config_lines(&json),
            Vec::<&str>::new(),
            "{command:?}"
        );
    }

    let config = run(&root, &["config"], &[]);
    assert_eq!(config.code, 0, "config: {}", config.stderr);
    assert_eq!(loaded_config_lines(&config), [expected.as_str()], "config");
    let config_quiet = run(&root, &["config", "--quiet"], &[]);
    assert_eq!(loaded_config_lines(&config_quiet), Vec::<&str>::new());
}

#[test]
fn an_explicit_config_logs_the_path_as_given() {
    let dir = project();
    let root = canonical_root(&dir);
    std::fs::write(root.join("custom.json"), "{}").unwrap();
    let output = run(&root, &["dead-code", "--config", "custom.json"], &[]);
    assert!(output.code == 0 || output.code == 1, "{}", output.stderr);
    assert_eq!(loaded_config_lines(&output), ["loaded config: custom.json"]);
}

#[test]
fn report_from_a_saved_file_loads_the_config_silently() {
    let dir = project();
    let root = canonical_root(&dir);
    std::fs::write(root.join(".fallowrc.json"), "{}").unwrap();
    let saved = run(&root, &["dead-code", "--format", "json", "--quiet"], &[]);
    let saved_path = root.join("saved.json");
    std::fs::write(&saved_path, &saved.stdout).unwrap();
    let saved_path = saved_path.to_str().unwrap();
    for format in ["sarif", "markdown", "codeclimate"] {
        let output = run(
            &root,
            &["report", "--from", saved_path, "--format", format],
            &[],
        );
        assert_eq!(output.code, 0, "{format}: {}", output.stderr);
        assert_eq!(loaded_config_lines(&output), Vec::<&str>::new(), "{format}");
    }
}

/// `report --from` reads optional settings from the discovered config. A
/// discovered config that does not load gives the same report as no config,
/// with no stderr text and exit 0.
#[test]
fn report_from_a_saved_file_ignores_a_broken_discovered_config() {
    let dir = project();
    let root = canonical_root(&dir);
    let saved = run(&root, &["dead-code", "--format", "json", "--quiet"], &[]);
    let saved_path = root.join("saved.json");
    std::fs::write(&saved_path, &saved.stdout).unwrap();
    let saved_path = saved_path.to_str().unwrap();
    for format in ["sarif", "markdown", "codeclimate"] {
        let args = ["report", "--from", saved_path, "--format", format];
        let _ = std::fs::remove_file(root.join(".fallowrc.json"));
        let without_config = run(&root, &args, &[]);
        assert_eq!(
            without_config.code, 0,
            "{format}: {}",
            without_config.stderr
        );

        std::fs::write(root.join(".fallowrc.json"), "{ \"rules\": \n").unwrap();
        let broken = run(&root, &args, &[]);
        assert_eq!(broken.code, 0, "{format}: {}", broken.stderr);
        assert_eq!(broken.stderr, "", "{format}");
        assert_eq!(broken.stderr, without_config.stderr, "{format}");
        assert_eq!(broken.stdout, without_config.stdout, "{format}");
    }
}

/// `audit-cache prune` reads `cache.dir` from the config. An explicit config
/// that does not load gives the default cache dir, so prune does not fail.
#[test]
fn audit_cache_prune_ignores_a_config_that_does_not_load() {
    let dir = project();
    let root = canonical_root(&dir);
    std::fs::write(root.join("broken.json"), "{ \"rules\": \n").unwrap();
    let scan = tempfile::tempdir().expect("scan root");
    let scan_root = scan.path().to_str().unwrap();
    let envs = [
        ("TMPDIR", scan_root),
        ("TMP", scan_root),
        ("TEMP", scan_root),
    ];
    for config in ["broken.json", "missing.json"] {
        let output = run(
            &root,
            &[
                "audit-cache",
                "prune",
                "--dry-run",
                "--format",
                "json",
                "--config",
                config,
            ],
            &envs,
        );
        assert_eq!(output.code, 0, "{config}: {}", output.stderr);
        assert_eq!(output.stderr, "", "{config}");
        let json = parse_json(&output);
        assert_eq!(json["complete"], serde_json::json!(true), "{config}");
    }
}

/// A config with a remote `extends` entry: no server listens on port 1, so
/// the fetch fails at once and offline.
fn remote_extends_config(root: &Path) -> PathBuf {
    let config = root.join("remote.json");
    std::fs::write(&config, r#"{"extends":"https://127.0.0.1:1/fallow.json"}"#).unwrap();
    config
}

/// `report --from` loads the `--config` file with the trust policy of
/// `--allow-remote-extends`: without the flag the load refuses the remote
/// entry, with the flag it fetches it.
#[test]
fn report_from_a_saved_file_fetches_remote_extends_only_with_the_flag() {
    let dir = project();
    let root = canonical_root(&dir);
    let saved = run(&root, &["dead-code", "--format", "json", "--quiet"], &[]);
    let saved_path = root.join("saved.json");
    std::fs::write(&saved_path, &saved.stdout).unwrap();
    let config = remote_extends_config(&root);
    let cause = FallowConfig::load(&config).expect_err("refused");
    let refused = format!(
        "Error: failed to load report config {}: {cause}\n",
        config.display()
    );
    assert!(
        cause.to_string().contains("is disabled by default"),
        "{cause}"
    );
    let args = [
        "report",
        "--from",
        saved_path.to_str().unwrap(),
        "--format",
        "sarif",
        "--config",
        config.to_str().unwrap(),
    ];
    let output = run(&root, &args, &[]);
    assert_eq!(output.code, 2, "{}", output.stderr);
    assert_eq!(output.stderr, refused);

    let output = run(&root, &with_args(&args, &["--allow-remote-extends"]), &[]);
    assert_eq!(output.code, 2, "{}", output.stderr);
    assert!(
        output.stderr.contains("Failed to fetch remote config"),
        "{}",
        output.stderr
    );
}

/// `plugin-check` reads the external plugins of the `--config` file.
#[test]
fn plugin_check_reads_the_explicit_config() {
    let dir = project();
    let root = canonical_root(&dir);
    std::fs::create_dir_all(root.join("tools")).unwrap();
    std::fs::write(
        root.join("tools/entries.json"),
        r#"{"name":"ci-entries","entryPoints":["src/index.ts"]}"#,
    )
    .unwrap();
    std::fs::write(
        root.join("custom.json"),
        r#"{"plugins":["tools/entries.json"]}"#,
    )
    .unwrap();
    let output = run(
        &root,
        &[
            "plugin-check",
            "--format",
            "json",
            "--config",
            "custom.json",
        ],
        &[],
    );
    assert_eq!(output.code, 0, "{}", output.stderr);
    let names: Vec<String> = parse_json(&output)["plugins"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|plugin| plugin["name"].as_str().map(str::to_owned))
        .collect();
    assert_eq!(names, ["ci-entries"]);
}

/// `plugin-check` loads the config with the trust policy of
/// `--allow-remote-extends`.
#[test]
fn plugin_check_fetches_remote_extends_only_with_the_flag() {
    let dir = project();
    let root = canonical_root(&dir);
    let config = remote_extends_config(&root);
    let args = [
        "plugin-check",
        "--format",
        "json",
        "--config",
        config.to_str().unwrap(),
    ];
    let refused = run(&root, &args, &[]);
    assert_eq!(refused.code, 2, "{}", refused.stderr);
    assert!(
        refused.stdout.contains("Remote config extends"),
        "{}",
        refused.stdout
    );
    let fetched = run(&root, &with_args(&args, &["--allow-remote-extends"]), &[]);
    assert_eq!(fetched.code, 2, "{}", fetched.stderr);
    assert!(
        fetched.stdout.contains("Failed to fetch remote config"),
        "{}",
        fetched.stdout
    );
}

/// The coverage uploads analyze the project with the `--config` file.
#[test]
fn coverage_upload_reads_the_explicit_config() {
    let dir = project();
    let root = canonical_root(&dir);
    std::fs::write(root.join("broken.json"), "{ \"rules\": \n").unwrap();
    let output = run(
        &root,
        &[
            "coverage",
            "upload-static-findings",
            "--dry-run",
            "--allow-dirty",
            "--project-id",
            "acme/app",
            "--git-sha",
            "0123456789abcdef0123456789abcdef01234567",
            "--config",
            "broken.json",
        ],
        &[],
    );
    assert_ne!(output.code, 0, "{}", output.stdout);
    assert!(
        output.stderr.contains("config load failed"),
        "{}",
        output.stderr
    );
}

/// The arguments of a dry-run `coverage upload-static-findings`.
const UPLOAD_DRY_RUN: &[&str] = &[
    "coverage",
    "upload-static-findings",
    "--dry-run",
    "--allow-dirty",
    "--project-id",
    "acme/app",
    "--git-sha",
    "0123456789abcdef0123456789abcdef01234567",
];

/// A coverage upload names an explicit config that does not load once, with
/// the cause of the parser.
#[test]
fn coverage_upload_names_a_broken_explicit_config_once() {
    let dir = project();
    let root = canonical_root(&dir);
    let broken = root.join("broken.json");
    std::fs::write(&broken, "{ \"rules\": \n").unwrap();
    let output = run(
        &root,
        &with_args(UPLOAD_DRY_RUN, &["--config", "broken.json"]),
        &[],
    );
    assert_ne!(output.code, 0, "{}", output.stdout);
    let cause = FallowConfig::load(&broken).expect_err("does not load");
    assert!(
        output
            .stderr
            .contains(&format!("error: config load failed: {cause}")),
        "{}",
        output.stderr
    );
    assert!(
        !output.stderr.contains("failed to load config"),
        "{}",
        output.stderr
    );
}

/// A coverage upload reads the project as the config file gives it. A rule
/// pack that does not exist does not stop the upload.
#[test]
fn coverage_upload_does_not_validate_the_config() {
    let dir = project();
    let root = canonical_root(&dir);
    std::fs::write(
        root.join(".fallowrc.json"),
        r#"{"rulePacks":["./packs/missing.json"]}"#,
    )
    .unwrap();
    let output = run(&root, UPLOAD_DRY_RUN, &[]);
    assert_eq!(output.code, 0, "{}", output.stderr);
}

/// `plugin-check` reports an external plugin that fails validation, as the
/// command exists to check such a plugin.
#[test]
fn plugin_check_reports_a_plugin_with_an_invalid_glob() {
    let dir = project();
    let root = canonical_root(&dir);
    std::fs::create_dir_all(root.join(".fallow/plugins")).unwrap();
    std::fs::write(
        root.join(".fallow/plugins/escape.json"),
        r#"{"name":"escape","entryPoints":["../outside/**"]}"#,
    )
    .unwrap();
    let output = run(&root, &["plugin-check", "--format", "json"], &[]);
    assert_eq!(output.code, 0, "{}{}", output.stdout, output.stderr);
    let names: Vec<String> = parse_json(&output)["plugins"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|plugin| plugin["name"].as_str().map(str::to_owned))
        .collect();
    assert_eq!(names, ["escape"]);
}

/// `fallow security` rejects an unsupported `--format` before it loads the
/// config, so the format error shows also when the config is invalid.
#[test]
fn security_reports_an_unsupported_format_before_a_config_error() {
    let dir = project();
    let root = canonical_root(&dir);
    std::fs::write(
        root.join(".fallowrc.json"),
        r#"{"rulePacks":["./packs/missing.json"]}"#,
    )
    .unwrap();
    let cases: [(&[&str], &str); 2] = [
        (
            &["security", "--format", "compact"],
            "Error: fallow security supports --format human, json, sarif, github-annotations, or github-summary only.\n",
        ),
        (
            &["security", "blind-spots", "--format", "sarif"],
            "Error: fallow security blind-spots supports --format human or json only.\n",
        ),
    ];
    for (args, expected) in cases {
        let output = run(&root, args, &[]);
        assert_eq!(output.code, 2, "{args:?}: {}", output.stderr);
        assert_eq!(output.stderr, expected, "{args:?}");
    }
}

fn workspace_notices(output: &CommandOutput) -> usize {
    output
        .stderr
        .lines()
        .filter(|line| {
            line.starts_with("fallow: ")
                && line.ends_with("workspace discovery diagnostics. Run `fallow workspaces`.")
        })
        .count()
}

/// A glob of the `workspaces` field that matches two directories without a
/// `package.json` gives a workspace discovery diagnostic. Each config load of
/// the main CLI loader prints the notice in human mode. The bare run,
/// `dead-code`, `dupes`, `health` and `audit` resolve their run once, so they
/// print the notice once.
#[test]
fn the_workspace_diagnostics_notice_prints_once_per_config_load() {
    let dir = project();
    let root = canonical_root(&dir);
    for package in ["pkgs/aaa", "pkgs/bbb"] {
        std::fs::create_dir_all(root.join(package)).unwrap();
        std::fs::write(root.join(package).join("readme.txt"), "no manifest\n").unwrap();
    }
    std::fs::write(
        root.join("package.json"),
        r#"{"name":"config-load","main":"src/index.ts","workspaces":["pkgs/*"]}"#,
    )
    .unwrap();
    git(&root, &["add", "."]);
    git(
        &root,
        &["-c", "commit.gpgsign=false", "commit", "-m", "pkgs", "-q"],
    );
    let commands: [(&[&str], usize); 8] = [
        (&[], 1),
        (&["dead-code"], 1),
        (&["dupes"], 1),
        (&["health"], 1),
        (&["audit", "--base", "HEAD"], 1),
        // `fallow list` loads the config in quiet mode and prints its own
        // diagnostics block.
        (&["list"], 0),
        (&["flags"], 1),
        (&["security"], 1),
    ];
    for (command, notices) in commands {
        let human = run(&root, command, &[]);
        assert!(
            human.code == 0 || human.code == 1,
            "{command:?}: {}",
            human.stderr
        );
        assert_eq!(
            workspace_notices(&human),
            notices,
            "{command:?}: {}",
            human.stderr
        );
        let quiet = run(&root, &with_args(command, &["--quiet"]), &[]);
        assert_eq!(workspace_notices(&quiet), 0, "{command:?}");
    }
}

/// The lines of the workspace discovery notice, singular or plural.
fn discovery_notice_lines(output: &CommandOutput) -> Vec<&str> {
    output
        .stderr
        .lines()
        .filter(|line| line.contains("workspace discovery diagnostic"))
        .collect()
}

/// The notice counts only the diagnostics of workspace discovery. A
/// `package.json` that no `workspaces` glob matches is an undeclared
/// workspace: `fallow workspaces` lists it, but the notice does not count it.
#[test]
fn the_workspace_discovery_notice_leaves_out_undeclared_workspaces() {
    let dir = project();
    let root = canonical_root(&dir);
    for package in ["packages/a", "tools/x"] {
        std::fs::create_dir_all(root.join(package)).unwrap();
        let name = package.replace('/', "-");
        std::fs::write(
            root.join(package).join("package.json"),
            format!(r#"{{"name":"{name}"}}"#),
        )
        .unwrap();
    }
    std::fs::write(
        root.join("package.json"),
        r#"{"name":"config-load","main":"src/index.ts","workspaces":["packages/*"]}"#,
    )
    .unwrap();
    git(&root, &["add", "."]);
    git(
        &root,
        &[
            "-c",
            "commit.gpgsign=false",
            "commit",
            "-m",
            "packages",
            "-q",
        ],
    );
    let commands: [&[&str]; 5] = [
        &[],
        &["dead-code"],
        &["dupes"],
        &["health"],
        &["audit", "--base", "HEAD"],
    ];
    for command in commands {
        let human = run(&root, command, &[]);
        assert!(
            human.code == 0 || human.code == 1,
            "{command:?}: {}",
            human.stderr
        );
        assert_eq!(
            discovery_notice_lines(&human),
            Vec::<&str>::new(),
            "{command:?}"
        );
    }

    std::fs::create_dir_all(root.join("packages/nopkg")).unwrap();
    std::fs::write(root.join("packages/nopkg/readme.txt"), "no manifest\n").unwrap();
    // A commit keeps the audit free of changed files, so it loads the config
    // one time.
    git(&root, &["add", "."]);
    git(
        &root,
        &["-c", "commit.gpgsign=false", "commit", "-m", "nopkg", "-q"],
    );
    for command in commands {
        let human = run(&root, command, &[]);
        assert_eq!(
            discovery_notice_lines(&human),
            ["fallow: 1 workspace discovery diagnostic. Run `fallow workspaces`."],
            "{command:?}"
        );
    }
}

/// A flag usage error prints before the config loads: the run prints the
/// error and no `loaded config` line. For `dead-code` and `dupes`, the flag
/// error also wins over a config that fails validation. The bare run needs
/// the valid config for its plan, so it stops on the config error first.
#[test]
fn a_flag_usage_error_prints_before_the_config_loads() {
    let dir = project();
    let root = canonical_root(&dir);
    std::fs::write(root.join(".fallowrc.json"), "{}").unwrap();
    let growth = "Error: --baseline-base needs --fail-on-baseline-growth.";
    let cases: [(&[&str], &str); 6] = [
        (
            &["dead-code", "--type-aware-project", "tsconfig.json"],
            "Error: --type-aware-project requires --type-aware",
        ),
        (&["dead-code", "--baseline-base", "main"], growth),
        (&["dupes", "--baseline-base", "main"], growth),
        (&["--only", "dupes", "--baseline-base", "main"], growth),
        (
            &[
                "health",
                "--coverage-root",
                "rel/path",
                "--coverage",
                "cov.json",
            ],
            "Error: ",
        ),
        (
            &["health", "--report-only", "--min-score", "50"],
            "Error: --report-only cannot be combined",
        ),
    ];
    let assert_usage_errors = |cases: &[(&[&str], &str)]| {
        for (args, error) in cases {
            let human = run(&root, args, &[]);
            assert_eq!(human.code, 2, "{args:?}: {}", human.stderr);
            assert!(
                human.stderr.starts_with(error),
                "{args:?}: {}",
                human.stderr
            );
            assert_eq!(human.stderr.matches("Error: ").count(), 1, "{args:?}");
            assert_eq!(loaded_config_lines(&human), Vec::<&str>::new(), "{args:?}");
        }
    };
    assert_usage_errors(&cases);

    std::fs::write(
        root.join(".fallowrc.json"),
        r#"{"plugins":["plugins/escape.json"]}"#,
    )
    .unwrap();
    std::fs::create_dir_all(root.join("plugins")).unwrap();
    std::fs::write(
        root.join("plugins/escape.json"),
        r#"{ "name": "escape", "entryPoints": ["../outside/**"] }"#,
    )
    .unwrap();
    assert_usage_errors(&cases[..3]);
}

/// An audit that selects no sub-analysis stops when the run resolves, before
/// the `loaded config` line, so the error is the only line.
#[test]
fn an_audit_with_an_empty_selection_prints_only_the_error() {
    let dir = project();
    let root = canonical_root(&dir);
    std::fs::write(
        root.join(".fallowrc.json"),
        r#"{"audit":{"deadCode":false,"health":false,"dupes":false}}"#,
    )
    .unwrap();
    let human = run(&root, &["audit", "--base", "HEAD"], &[]);
    assert_eq!(human.code, 2, "{}", human.stderr);
    assert_eq!(
        human.stderr,
        "Error: this audit selects no sub-analysis: --skip, the audit.deadCode, audit.health and audit.dupes config keys, and duplicates.enabled leave nothing to run\n"
    );
}

/// Whether the dead-code run of `root` skips `src/huge.ts` for its size.
fn skips_the_large_file(root: &Path, extra: &[&str], envs: &[(&str, &str)]) -> bool {
    let args = with_args(
        &["dead-code", "--format", "json", "--quiet", "--no-cache"],
        extra,
    );
    let output = run(root, &args, envs);
    assert!(
        output.code == 0 || output.code == 1,
        "{args:?}: {}",
        output.stderr
    );
    parse_json(&output)["workspace_diagnostics"]
        .as_array()
        .expect("workspace diagnostics")
        .iter()
        .any(|diagnostic| {
            diagnostic["kind"] == "skipped-large-file" && diagnostic["path"] == "src/huge.ts"
        })
}

#[test]
fn the_max_file_size_flag_wins_over_the_env_variable() {
    let dir = project();
    let root = canonical_root(&dir);
    std::fs::write(root.join("src/huge.ts"), "// filler\n".repeat(150_000)).unwrap();
    assert!(!skips_the_large_file(&root, &[], &[]));
    assert!(skips_the_large_file(
        &root,
        &[],
        &[("FALLOW_MAX_FILE_SIZE", " 1 ")]
    ));
    assert!(!skips_the_large_file(
        &root,
        &["--max-file-size", "10"],
        &[("FALLOW_MAX_FILE_SIZE", "1")]
    ));
    assert!(skips_the_large_file(
        &root,
        &["--max-file-size", "1"],
        &[("FALLOW_MAX_FILE_SIZE", "10")]
    ));
    assert!(!skips_the_large_file(
        &root,
        &["--max-file-size", "0"],
        &[("FALLOW_MAX_FILE_SIZE", "1")]
    ));
    assert!(!skips_the_large_file(
        &root,
        &[],
        &[("FALLOW_MAX_FILE_SIZE", "one")]
    ));
}

#[derive(Clone, Copy, Debug)]
enum Flag {
    None,
    Production,
    NoProduction,
    Own(usize),
    NoProductionWithOwn(usize),
}

impl Flag {
    const ALL: [Self; 8] = [
        Self::None,
        Self::Production,
        Self::NoProduction,
        Self::Own(0),
        Self::Own(1),
        Self::Own(2),
        Self::NoProductionWithOwn(1),
        Self::NoProductionWithOwn(2),
    ];

    fn args(self) -> Vec<&'static str> {
        const OWN: [&str; 3] = [
            "--production-dead-code",
            "--production-health",
            "--production-dupes",
        ];
        match self {
            Self::None => vec![],
            Self::Production => vec!["--production"],
            Self::NoProduction => vec!["--no-production"],
            Self::Own(analysis) => vec![OWN[analysis]],
            Self::NoProductionWithOwn(analysis) => vec!["--no-production", OWN[analysis]],
        }
    }
}

/// The env variable of each analysis, in the order dead code, health, dupes.
const ANALYSIS_ENV: [&str; 3] = [
    "FALLOW_PRODUCTION_DEAD_CODE",
    "FALLOW_PRODUCTION_HEALTH",
    "FALLOW_PRODUCTION_DUPES",
];

/// Env values per analysis. `maybe` is not a boolean, so it sets nothing.
const ANALYSIS_ENV_SETS: [[Option<&str>; 3]; 4] = [
    [None, None, None],
    [Some("yes"), Some("0"), None],
    [None, Some("yes"), Some("OFF")],
    [Some("0"), Some("maybe"), Some("on")],
];

const GLOBAL_ENV: [Option<&str>; 4] = [None, Some("TRUE"), Some(" off "), Some("maybe")];

/// The config files, with the production setting of each analysis.
const CONFIGS: [(Option<&str>, [bool; 3]); 4] = [
    (None, [false; 3]),
    (Some(r#"{"production":true}"#), [true; 3]),
    (
        Some(r#"{"production":{"deadCode":true,"health":false,"dupes":true}}"#),
        [true, false, true],
    ),
    (
        Some(r#"{"production":{"deadCode":false,"health":true,"dupes":false}}"#),
        [false, true, false],
    ),
];

fn env_bool(value: Option<&str>) -> Option<bool> {
    match value?.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Some(true),
        "0" | "false" | "no" | "off" => Some(false),
        _ => None,
    }
}

/// The precedence of the production mode of one analysis: the flag of the
/// analysis, then `--production` or `--no-production`, then the env variable
/// of the analysis, then `FALLOW_PRODUCTION`, then the config.
fn expected_mode(
    flag: Flag,
    analysis: usize,
    own_env: Option<&str>,
    global_env: Option<&str>,
    config: bool,
) -> bool {
    match flag {
        Flag::Production => true,
        Flag::Own(own) | Flag::NoProductionWithOwn(own) if own == analysis => true,
        Flag::NoProduction | Flag::NoProductionWithOwn(_) => false,
        Flag::None | Flag::Own(_) => env_bool(own_env)
            .or_else(|| env_bool(global_env))
            .unwrap_or(config),
    }
}

/// The production mode of each analysis that a combined JSON run shows.
fn observed_modes(output: &CommandOutput) -> [bool; 3] {
    let json = parse_json(output);
    let dead_code = json["check"]["unused_dev_dependencies"]
        .as_array()
        .expect("unused dev dependencies")
        .is_empty();
    let health = !json["health"]["findings"]
        .as_array()
        .expect("health findings")
        .iter()
        .any(|finding| finding["path"] == "src/complex.test.ts");
    let dupes = !json["dupes"]["clone_groups"]
        .as_array()
        .expect("clone groups")
        .iter()
        .flat_map(|group| group["instances"].as_array().expect("instances"))
        .any(|instance| instance["file"] == "src/copy.stories.ts");
    [dead_code, health, dupes]
}

struct ProductionCase {
    flag: Flag,
    analysis_env: [Option<&'static str>; 3],
    global_env: Option<&'static str>,
    config: (Option<&'static str>, [bool; 3]),
}

fn check_production_case(root: &Path, case: &ProductionCase) -> Option<String> {
    let mut envs: Vec<(&str, &str)> = Vec::new();
    for (name, value) in ANALYSIS_ENV.iter().zip(case.analysis_env) {
        if let Some(value) = value {
            envs.push((name, value));
        }
    }
    if let Some(value) = case.global_env {
        envs.push(("FALLOW_PRODUCTION", value));
    }
    let mut args = vec!["--format", "json", "--quiet"];
    args.extend(case.flag.args());
    if case.config.0.is_some() {
        args.extend(["--config", "production.json"]);
    }
    let output = run(root, &args, &envs);
    if output.code != 0 && output.code != 1 {
        return Some(format!(
            "{args:?} {envs:?}: exit {}: {}",
            output.code, output.stderr
        ));
    }
    let expected: [bool; 3] = std::array::from_fn(|analysis| {
        expected_mode(
            case.flag,
            analysis,
            case.analysis_env[analysis],
            case.global_env,
            case.config.1[analysis],
        )
    });
    let observed = observed_modes(&output);
    (observed != expected).then(|| {
        format!(
            "{args:?} {envs:?} config {:?}: expected {expected:?}, observed {observed:?}",
            case.config.0
        )
    })
}

/// The production mode of each analysis over flag, env and config
/// combinations. Each config gets its own project copy, so the runs of one
/// config share one project and the configs run in parallel.
#[test]
fn production_mode_precedence_table() {
    let failures: Vec<String> = std::thread::scope(|scope| {
        let mut handles = Vec::with_capacity(CONFIGS.len());
        for config in CONFIGS {
            handles.push(scope.spawn(move || production_cases_of_config(config)));
        }
        handles
            .into_iter()
            .flat_map(|handle| handle.join().expect("table thread"))
            .collect()
    });
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// The failures of each table case of one config.
fn production_cases_of_config(config: (Option<&'static str>, [bool; 3])) -> Vec<String> {
    let dir = project();
    let root = canonical_root(&dir);
    if let Some(body) = config.0 {
        std::fs::write(root.join("production.json"), body).unwrap();
    }
    let mut failures = Vec::new();
    for flag in Flag::ALL {
        for analysis_env in ANALYSIS_ENV_SETS {
            for global_env in GLOBAL_ENV {
                let case = ProductionCase {
                    flag,
                    analysis_env,
                    global_env,
                    config,
                };
                failures.extend(check_production_case(&root, &case));
            }
        }
    }
    failures
}
