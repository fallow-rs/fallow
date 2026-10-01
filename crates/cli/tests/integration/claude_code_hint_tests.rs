//! Process-level tests for the Claude Code plugin hint on stderr.
//!
//! Each test runs the real binary with a clean environment: the CI markers,
//! the Claude Code session markers and the opt-out are removed first, and
//! `HOME` and `CLAUDE_CONFIG_DIR` point into a temporary directory, so the
//! machine that runs the tests cannot change the result.

use std::path::Path;
use std::process::Command;

use super::common::{CommandOutput, copy_fixture, fallow_bin};

const HINT: &str =
    r#"<claude-code-hint v="1" type="plugin" value="fallow@claude-plugins-official" />"#;

const AMBIENT_ENV: &[&str] = &[
    "CI",
    "GITHUB_ACTIONS",
    "GITLAB_CI",
    "CLAUDECODE",
    "CLAUDE_CODE_CHILD_SESSION",
    "CLAUDE_CONFIG_DIR",
    "FALLOW_CLAUDE_CODE_HINT",
    "FALLOW_SUGGESTIONS",
    "FALLOW_FORMAT",
    "FALLOW_QUIET",
];

fn run(root: &Path, home: &Path, args: &[&str], env: &[(&str, &str)]) -> CommandOutput {
    let mut cmd = Command::new(fallow_bin());
    for key in AMBIENT_ENV {
        cmd.env_remove(key);
    }
    cmd.env("RUST_LOG", "")
        .env("NO_COLOR", "1")
        .env("HOME", home)
        .env("USERPROFILE", home)
        .env("CLAUDE_CONFIG_DIR", home.join(".claude"))
        .env("FALLOW_UPDATE_CHECK", "off")
        .env("FALLOW_TELEMETRY", "off")
        .envs(env.iter().copied())
        .arg("dead-code")
        .arg("--root")
        .arg(root)
        .arg("--no-cache")
        .args(args);
    let output = cmd.output().expect("run fallow binary");
    CommandOutput {
        stdout: String::from_utf8_lossy(&output.stdout).to_string(),
        stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        code: output.status.code().unwrap_or(-1),
    }
}

fn hint_count(text: &str) -> usize {
    text.lines().filter(|line| line.trim() == HINT).count()
}

fn assert_no_hint(output: &CommandOutput, case: &str) {
    assert_eq!(hint_count(&output.stdout), 0, "{case}: hint on stdout");
    assert_eq!(
        hint_count(&output.stderr),
        0,
        "{case}: hint on stderr:\n{}",
        output.stderr
    );
}

#[test]
fn claude_code_session_gets_one_hint_line_on_stderr() {
    let project = copy_fixture("basic-project");
    let home = tempfile::tempdir().expect("temp home");
    let output = run(project.path(), home.path(), &[], &[("CLAUDECODE", "1")]);
    assert!(output.code == 0 || output.code == 1, "{}", output.stderr);
    assert_eq!(hint_count(&output.stderr), 1, "stderr:\n{}", output.stderr);
    assert_eq!(
        hint_count(&output.stdout),
        0,
        "the hint never goes to stdout"
    );
}

#[test]
fn claude_code_child_session_marker_also_enables_the_hint() {
    let project = copy_fixture("basic-project");
    let home = tempfile::tempdir().expect("temp home");
    let output = run(
        project.path(),
        home.path(),
        &[],
        &[("CLAUDE_CODE_CHILD_SESSION", "1")],
    );
    assert_eq!(hint_count(&output.stderr), 1, "stderr:\n{}", output.stderr);
}

#[test]
fn no_hint_without_a_claude_code_session() {
    let project = copy_fixture("basic-project");
    let home = tempfile::tempdir().expect("temp home");
    let output = run(project.path(), home.path(), &[], &[]);
    assert_no_hint(&output, "no session");
    let output = run(project.path(), home.path(), &[], &[("CLAUDECODE", "")]);
    assert_no_hint(&output, "empty CLAUDECODE");
}

#[test]
fn no_hint_for_any_machine_readable_format() {
    let project = copy_fixture("basic-project");
    let home = tempfile::tempdir().expect("temp home");
    for format in [
        "json",
        "sarif",
        "codeclimate",
        "compact",
        "markdown",
        "badge",
    ] {
        let output = run(
            project.path(),
            home.path(),
            &["--format", format],
            &[("CLAUDECODE", "1")],
        );
        assert_no_hint(&output, format);
    }
}

#[test]
fn no_hint_under_quiet() {
    let project = copy_fixture("basic-project");
    let home = tempfile::tempdir().expect("temp home");
    let output = run(
        project.path(),
        home.path(),
        &["--quiet"],
        &[("CLAUDECODE", "1")],
    );
    assert_no_hint(&output, "--quiet");
}

#[test]
fn no_hint_in_ci() {
    let project = copy_fixture("basic-project");
    let home = tempfile::tempdir().expect("temp home");
    let output = run(
        project.path(),
        home.path(),
        &[],
        &[("CLAUDECODE", "1"), ("CI", "true")],
    );
    assert_no_hint(&output, "CI");
}

#[test]
fn opt_out_values_silence_the_hint() {
    let project = copy_fixture("basic-project");
    let home = tempfile::tempdir().expect("temp home");
    for value in ["off", "0", "false", "no", "disabled", "OFF"] {
        let output = run(
            project.path(),
            home.path(),
            &[],
            &[("CLAUDECODE", "1"), ("FALLOW_CLAUDE_CODE_HINT", value)],
        );
        assert_no_hint(&output, value);
    }
    let output = run(
        project.path(),
        home.path(),
        &[],
        &[("CLAUDECODE", "1"), ("FALLOW_SUGGESTIONS", "off")],
    );
    assert_no_hint(&output, "FALLOW_SUGGESTIONS=off");
}

#[test]
fn no_hint_when_the_project_enables_the_fallow_plugin() {
    let project = copy_fixture("basic-project");
    let home = tempfile::tempdir().expect("temp home");
    let claude = project.path().join(".claude");
    std::fs::create_dir_all(&claude).expect("create .claude");
    std::fs::write(
        claude.join("settings.json"),
        r#"{"enabledPlugins":{"fallow@fallow-skills":true}}"#,
    )
    .expect("write settings");
    let output = run(project.path(), home.path(), &[], &[("CLAUDECODE", "1")]);
    assert_no_hint(&output, "project enabledPlugins");
}

#[test]
fn no_hint_when_the_repository_root_enables_the_plugin_for_a_nested_root() {
    let project = copy_fixture("basic-project");
    let home = tempfile::tempdir().expect("temp home");
    std::fs::create_dir_all(project.path().join(".git")).expect("create .git");
    let claude = project.path().join(".claude");
    std::fs::create_dir_all(&claude).expect("create .claude");
    std::fs::write(
        claude.join("settings.json"),
        r#"{"enabledPlugins":{"fallow@fallow-skills":true}}"#,
    )
    .expect("write settings");
    let nested = project.path().join("src");
    let output = run(&nested, home.path(), &[], &[("CLAUDECODE", "1")]);
    assert_no_hint(
        &output,
        "repository-root enabledPlugins with a nested --root",
    );
}

#[test]
fn no_hint_on_an_input_error() {
    let project = copy_fixture("basic-project");
    let home = tempfile::tempdir().expect("temp home");
    let output = run(
        project.path(),
        home.path(),
        &["--config", "missing-config.json"],
        &[("CLAUDECODE", "1")],
    );
    assert_eq!(output.code, 2, "{}", output.stderr);
    assert_no_hint(&output, "exit 2");
}

#[test]
fn no_hint_when_the_plugin_is_installed_for_the_user() {
    let project = copy_fixture("basic-project");
    let home = tempfile::tempdir().expect("temp home");
    let plugins = home.path().join(".claude/plugins");
    std::fs::create_dir_all(&plugins).expect("create plugins dir");
    std::fs::write(
        plugins.join("installed_plugins.json"),
        r#"{"version":2,"plugins":{"fallow@fallow-skills":[{"scope":"user"}]}}"#,
    )
    .expect("write installed_plugins");
    let output = run(project.path(), home.path(), &[], &[("CLAUDECODE", "1")]);
    assert_no_hint(&output, "user-scope install");
}

#[test]
fn no_hint_from_the_agent_setup_commands() {
    let project = copy_fixture("basic-project");
    let home = tempfile::tempdir().expect("temp home");
    for args in [
        ["agent", "status"],
        ["agent", "uninstall"],
        ["hooks", "status"],
    ] {
        let mut cmd = Command::new(fallow_bin());
        for key in AMBIENT_ENV {
            cmd.env_remove(key);
        }
        let output = cmd
            .env("RUST_LOG", "")
            .env("NO_COLOR", "1")
            .env("HOME", home.path())
            .env("USERPROFILE", home.path())
            .env("CLAUDE_CONFIG_DIR", home.path().join(".claude"))
            .env("FALLOW_UPDATE_CHECK", "off")
            .env("FALLOW_TELEMETRY", "off")
            .env("CLAUDECODE", "1")
            .args(args)
            .arg("--root")
            .arg(project.path())
            .output()
            .expect("run fallow binary");
        let output = CommandOutput {
            stdout: String::from_utf8_lossy(&output.stdout).to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
            code: output.status.code().unwrap_or(-1),
        };
        assert!(
            output.code == 0 || output.code == 1,
            "{args:?}: {}",
            output.stderr
        );
        assert_no_hint(&output, &args.join(" "));
    }
}
