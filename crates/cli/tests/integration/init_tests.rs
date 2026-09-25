#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests and benches use unwrap and expect to keep fixture setup concise"
)]

use crate::common::run_fallow_raw;
use std::fs;
#[cfg(unix)]
use std::path::{Path, PathBuf};
use std::process::Command;

/// Create a unique temp dir for init tests.
fn init_temp_dir(suffix: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "fallow-init-test-{}-{}",
        std::process::id(),
        suffix
    ));
    if dir.exists() {
        let _ = fs::remove_dir_all(&dir);
    }
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join("package.json"),
        r#"{"name": "init-test", "main": "index.ts"}"#,
    )
    .unwrap();
    dir
}

/// Clean up a temp dir after a test.
fn cleanup(dir: &std::path::Path) {
    let _ = fs::remove_dir_all(dir);
}

#[cfg(unix)]
struct LefthookExecutionFixture {
    _dir: tempfile::TempDir,
    root: PathBuf,
    shell: PathBuf,
    restricted_bin: PathBuf,
    run_script: String,
}

#[cfg(unix)]
impl LefthookExecutionFixture {
    fn new() -> Self {
        use std::os::unix::fs::symlink;

        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        fs::write(root.join("package.json"), r#"{"name":"lefthook-test"}"#).unwrap();
        fs::write(root.join("lefthook.yml"), "pre-commit: {}\n").unwrap();

        let git = executable_on_path("git");
        let shell = executable_on_path("sh");
        let status = Command::new(&git)
            .args(["init", "-q"])
            .current_dir(&root)
            .env_clear()
            .status()
            .expect("initialize temporary Git repository");
        assert!(status.success());

        let output = run_fallow_raw(&[
            "--root",
            root.to_str().unwrap(),
            "hooks",
            "install",
            "--target",
            "git",
            "--branch",
            "develop",
        ]);
        assert_eq!(
            output.code, 0,
            "hooks install should generate a Lefthook job, stderr: {}",
            output.stderr
        );

        let restricted_bin = root.join("restricted-bin");
        fs::create_dir(&restricted_bin).unwrap();
        symlink(git, restricted_bin.join("git")).unwrap();

        Self {
            run_script: lefthook_run_script(&output.stderr),
            _dir: dir,
            root,
            shell,
            restricted_bin,
        }
    }

    fn run(&self, environment: &[(&str, &Path)]) -> std::process::ExitStatus {
        let mut command = Command::new(&self.shell);
        command
            .args(["-c", &self.run_script])
            .current_dir(&self.root)
            .env_clear()
            .env("PATH", &self.restricted_bin);
        for (name, value) in environment {
            command.env(name, value);
        }
        command.status().expect("execute generated Lefthook job")
    }
}

#[cfg(unix)]
fn executable_on_path(name: &str) -> PathBuf {
    let path = std::env::var_os("PATH").expect("test process must have PATH");
    std::env::split_paths(&path)
        .map(|directory| directory.join(name))
        .find(|candidate| candidate.is_file())
        .and_then(|candidate| fs::canonicalize(candidate).ok())
        .unwrap_or_else(|| panic!("{name} must be available for the execution test"))
}

#[cfg(unix)]
fn lefthook_run_script(hint: &str) -> String {
    let yaml_start = hint
        .find("pre-commit:")
        .expect("generated hint must contain a Lefthook config");
    let config: serde_yaml_ng::Value = serde_yaml_ng::from_str(&hint[yaml_start..])
        .expect("generated Lefthook hint must be valid YAML");
    config["pre-commit"]["commands"]["fallow"]["run"]
        .as_str()
        .expect("generated Lefthook command must contain a run script")
        .to_string()
}

#[cfg(unix)]
fn write_test_executable(path: &Path, shell: &Path, body: &str) {
    use std::os::unix::fs::PermissionsExt;

    fs::create_dir_all(path.parent().expect("test executable has a parent")).unwrap();
    fs::write(path, format!("#!{}\n{body}\n", shell.display())).unwrap();
    let mut permissions = fs::metadata(path).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions).unwrap();
}

#[test]
fn init_creates_fallowrc_json() {
    let dir = init_temp_dir("json");
    let output = run_fallow_raw(&["init", "--root", dir.to_str().unwrap(), "--quiet"]);
    assert_eq!(
        output.code, 0,
        "init should succeed, stderr: {}",
        output.stderr
    );
    assert!(
        dir.join(".fallowrc.json").exists(),
        "init should create .fallowrc.json"
    );
    cleanup(&dir);
}

#[test]
fn init_creates_toml_with_flag() {
    let dir = init_temp_dir("toml");
    let output = run_fallow_raw(&["init", "--toml", "--root", dir.to_str().unwrap(), "--quiet"]);
    assert_eq!(
        output.code, 0,
        "init --toml should succeed, stderr: {}",
        output.stderr
    );
    assert!(
        dir.join("fallow.toml").exists(),
        "init --toml should create fallow.toml"
    );
    cleanup(&dir);
}

#[test]
fn init_exits_nonzero_if_config_exists() {
    let dir = init_temp_dir("exists");
    run_fallow_raw(&["init", "--root", dir.to_str().unwrap(), "--quiet"]);
    let output = run_fallow_raw(&["init", "--root", dir.to_str().unwrap(), "--quiet"]);
    assert_ne!(
        output.code, 0,
        "init should fail when config already exists"
    );
    cleanup(&dir);
}

/// Issue #1794: without a local `node_modules/fallow/schema.json`, `fallow
/// init` writes the remote GitHub URL fallback.
#[test]
fn init_schema_falls_back_to_remote_without_local_schema() {
    let dir = init_temp_dir("schema-remote");
    let output = run_fallow_raw(&["init", "--root", dir.to_str().unwrap(), "--quiet"]);
    assert_eq!(
        output.code, 0,
        "init should succeed, stderr: {}",
        output.stderr
    );
    let config_path = dir.join(".fallowrc.json");
    let content = fs::read_to_string(&config_path).unwrap();
    assert!(
        content.contains(
            "\"$schema\": \"https://raw.githubusercontent.com/fallow-rs/fallow/main/schema.json\""
        ),
        "expected remote schema fallback without node_modules/fallow, got: {content}"
    );
    fallow_config::FallowConfig::load(&config_path)
        .unwrap_or_else(|e| panic!("init output must load as FallowConfig: {e:?}"));
    cleanup(&dir);
}

/// Issue #1794: with a local `node_modules/fallow/schema.json` present
/// (simulating an npm install of fallow), `fallow init` writes the local,
/// version-aligned schema path instead of the remote URL, and the resulting
/// config still loads through the real config loader.
#[test]
fn init_schema_prefers_local_when_node_modules_fallow_present() {
    let dir = init_temp_dir("schema-local");
    fs::create_dir_all(dir.join("node_modules/fallow")).unwrap();
    fs::write(dir.join("node_modules/fallow/schema.json"), "{}").unwrap();

    let output = run_fallow_raw(&["init", "--root", dir.to_str().unwrap(), "--quiet"]);
    assert_eq!(
        output.code, 0,
        "init should succeed, stderr: {}",
        output.stderr
    );
    let config_path = dir.join(".fallowrc.json");
    let content = fs::read_to_string(&config_path).unwrap();
    assert!(
        content.contains("\"$schema\": \"./node_modules/fallow/schema.json\""),
        "expected local schema path with node_modules/fallow present, got: {content}"
    );
    assert!(!content.contains("raw.githubusercontent.com"));

    fallow_config::FallowConfig::load(&config_path)
        .unwrap_or_else(|e| panic!("init output with local schema must load: {e:?}"));
    cleanup(&dir);
}

#[test]
fn init_created_config_is_valid_json() {
    let dir = init_temp_dir("valid");
    run_fallow_raw(&["init", "--root", dir.to_str().unwrap(), "--quiet"]);
    let content = fs::read_to_string(dir.join(".fallowrc.json")).unwrap();
    let _: serde_json::Value =
        jsonc_parser::parse_to_serde_value(&content, &jsonc_parser::ParseOptions::default())
            .unwrap_or_else(|e| {
                panic!("init should produce valid JSONC: {e}\ncontent: {content}");
            });
    cleanup(&dir);
}

#[test]
fn hooks_namespace_installs_and_uninstalls_git_hook() {
    let dir = init_temp_dir("hooks-namespace-git");
    let git = Command::new("git")
        .arg("init")
        .arg("-q")
        .current_dir(&dir)
        .status()
        .expect("git init should run");
    assert!(git.success());

    let root = dir.to_str().unwrap();
    let install = run_fallow_raw(&[
        "--root", root, "hooks", "install", "--target", "git", "--branch", "develop",
    ]);
    assert_eq!(
        install.code, 0,
        "hooks install --target git should succeed, stderr: {}",
        install.stderr
    );

    let hook_path = dir.join(".git/hooks/pre-commit");
    let hook = fs::read_to_string(&hook_path).unwrap();
    assert!(hook.contains("Generated by fallow hooks install --target git"));
    assert!(hook.contains("BASE=\"develop\""));

    let uninstall = run_fallow_raw(&["--root", root, "hooks", "uninstall", "--target", "git"]);
    assert_eq!(
        uninstall.code, 0,
        "hooks uninstall --target git should succeed, stderr: {}",
        uninstall.stderr
    );
    assert!(!hook_path.exists());
    cleanup(&dir);
}

/// A clone without `origin/HEAD` still knows its default branch through the
/// remote-tracking ref. The hook must fall back to that branch, not to `main`
/// (issue #2758).
#[test]
fn hooks_install_uses_origin_master_when_origin_head_is_missing() {
    let upstream = init_temp_dir("hooks-default-branch-upstream");
    crate::common::git(&upstream, &["init", "-q", "-b", "master"]);
    crate::common::commit_all(&upstream, "initial");

    let parent = init_temp_dir("hooks-default-branch-clone");
    let upstream_path = upstream.to_str().unwrap();
    crate::common::git(&parent, &["clone", "-q", upstream_path, "clone"]);
    let clone = parent.join("clone");
    crate::common::git(&clone, &["remote", "set-head", "origin", "-d"]);

    let install = run_fallow_raw(&[
        "--root",
        clone.to_str().unwrap(),
        "hooks",
        "install",
        "--target",
        "git",
    ]);
    assert_eq!(install.code, 0, "stderr: {}", install.stderr);

    let hook = fs::read_to_string(clone.join(".git/hooks/pre-commit")).unwrap();
    assert!(
        hook.contains("BASE=\"master\""),
        "the hook must fall back to origin/master: {hook}"
    );
    cleanup(&parent);
    cleanup(&upstream);
}

#[test]
fn hooks_namespace_agent_dry_run_uses_setup_hooks_engine() {
    let dir = init_temp_dir("hooks-namespace-agent");
    let output = run_fallow_raw(&[
        "--root",
        dir.to_str().unwrap(),
        "hooks",
        "install",
        "--target",
        "agent",
        "--agent",
        "claude",
        "--dry-run",
    ]);
    assert_eq!(
        output.code, 0,
        "hooks install --target agent should succeed, stderr: {}",
        output.stderr
    );
    assert!(
        output
            .stderr
            .contains("fallow hooks install --target agent (install) (dry run)"),
        "expected hooks namespace summary, stderr: {}",
        output.stderr
    );
    assert!(!dir.join(".claude").exists());
    cleanup(&dir);
}

#[test]
fn hooks_namespace_validation_respects_json_format() {
    let output = run_fallow_raw(&[
        "--format", "json", "hooks", "install", "--target", "git", "--agent", "claude",
    ]);
    assert_eq!(
        output.code, 2,
        "target mismatch should exit 2, stderr: {}",
        output.stderr
    );

    assert!(
        output.stderr.is_empty(),
        "json errors should not emit human stderr: {}",
        output.stderr
    );
    let json: serde_json::Value =
        serde_json::from_str(&output.stdout).expect("stdout should be structured JSON");
    assert_eq!(json["error"], true);
    assert!(
        json["message"]
            .as_str()
            .unwrap_or_default()
            .contains("--agent, --user, and --gitignore-claude"),
        "unexpected error payload: {json}"
    );
}

#[cfg(unix)]
#[test]
fn lefthook_job_executes_project_local_fallow_outside_path() {
    let fixture = LefthookExecutionFixture::new();
    let local_fallow = fixture.root.join("node_modules/.bin/fallow");
    let args_file = fixture.root.join("fallow-args");
    write_test_executable(
        &local_fallow,
        &fixture.shell,
        r#"printf '%s\n' "$@" > "$FALLOW_TEST_ARGS""#,
    );

    let lookup = Command::new(&fixture.shell)
        .args(["-c", "command -v fallow"])
        .current_dir(&fixture.root)
        .env_clear()
        .env("PATH", &fixture.restricted_bin)
        .status()
        .expect("probe restricted PATH");
    assert!(!lookup.success(), "fallow must not be available on PATH");

    assert!(fixture.run(&[("FALLOW_TEST_ARGS", &args_file)]).success());
    assert_eq!(
        fs::read_to_string(args_file).expect("project-local fallow must be invoked"),
        "audit\n--base\ndevelop\n--quiet\n--gate-marker\npre-commit\n"
    );
}

#[cfg(unix)]
#[test]
fn lefthook_job_propagates_project_local_fallow_failure() {
    let fixture = LefthookExecutionFixture::new();
    write_test_executable(
        &fixture.root.join("node_modules/.bin/fallow"),
        &fixture.shell,
        "exit 23",
    );

    assert_eq!(fixture.run(&[]).code(), Some(23));
}

#[cfg(unix)]
#[test]
fn lefthook_job_prefers_fallow_on_path() {
    let fixture = LefthookExecutionFixture::new();
    let source_file = fixture.root.join("fallow-source");
    let args_file = fixture.root.join("fallow-args");

    write_test_executable(
        &fixture.root.join("node_modules/.bin/fallow"),
        &fixture.shell,
        "printf '%s\\n' local > \"$FALLOW_TEST_SOURCE\"",
    );
    write_test_executable(
        &fixture.restricted_bin.join("fallow"),
        &fixture.shell,
        "printf '%s\\n' path > \"$FALLOW_TEST_SOURCE\"\nprintf '%s\\n' \"$@\" > \"$FALLOW_TEST_ARGS\"",
    );

    assert!(
        fixture
            .run(&[
                ("FALLOW_TEST_ARGS", &args_file),
                ("FALLOW_TEST_SOURCE", &source_file),
            ])
            .success()
    );
    assert_eq!(fs::read_to_string(source_file).unwrap(), "path\n");
    assert_eq!(
        fs::read_to_string(args_file).unwrap(),
        "audit\n--base\ndevelop\n--quiet\n--gate-marker\npre-commit\n"
    );
}

#[cfg(unix)]
#[test]
fn lefthook_job_uses_yarn_for_plug_and_play_dependency() {
    let fixture = LefthookExecutionFixture::new();
    let source_file = fixture.root.join("fallow-source");
    let args_file = fixture.root.join("fallow-args");
    write_test_executable(
        &fixture.restricted_bin.join("yarn"),
        &fixture.shell,
        r#"if [ "$1" = bin ] && [ "$2" = fallow ]; then
  printf '%s\n' /virtual/fallow
  exit 0
fi
if [ "$1" = exec ] && [ "$2" = fallow ]; then
  shift 2
  # Real yarn parses what follows the command name as its own flags and
  # forwards only what comes after the separator. Measured on yarn 1.22.22:
  # `yarn exec fallow audit --base HEAD` reaches the binary as `audit` alone,
  # so a stand-in that forwards everything would pass while the hook audits
  # the wrong base.
  if [ "$1" = -- ]; then
    shift
  else
    set --
  fi
  printf '%s\n' yarn > "$FALLOW_TEST_SOURCE"
  printf '%s\n' "$@" > "$FALLOW_TEST_ARGS"
  exit 0
fi
exit 1"#,
    );

    assert!(
        fixture
            .run(&[
                ("FALLOW_TEST_ARGS", &args_file),
                ("FALLOW_TEST_SOURCE", &source_file),
            ])
            .success()
    );
    assert_eq!(fs::read_to_string(source_file).unwrap(), "yarn\n");
    assert_eq!(
        fs::read_to_string(args_file).unwrap(),
        "audit\n--base\ndevelop\n--quiet\n--gate-marker\npre-commit\n"
    );
}

#[cfg(unix)]
#[test]
fn lefthook_job_skips_yarn_without_fallow_binary() {
    let fixture = LefthookExecutionFixture::new();
    let source_file = fixture.root.join("fallow-source");
    write_test_executable(
        &fixture.restricted_bin.join("yarn"),
        &fixture.shell,
        r#"if [ "$1" = bin ] && [ "$2" = fallow ]; then
  exit 0
fi
if [ "$1" = exec ]; then
  printf '%s\n' invoked > "$FALLOW_TEST_SOURCE"
fi
exit 1"#,
    );

    assert!(
        fixture
            .run(&[("FALLOW_TEST_SOURCE", &source_file)])
            .success()
    );
    assert!(
        !source_file.exists(),
        "empty yarn bin output must not select yarn exec"
    );
}

#[cfg(unix)]
#[test]
fn lefthook_job_skips_failed_yarn_lookup_with_stdout() {
    let fixture = LefthookExecutionFixture::new();
    let source_file = fixture.root.join("fallow-source");
    write_test_executable(
        &fixture.restricted_bin.join("yarn"),
        &fixture.shell,
        r#"if [ "$1" = bin ] && [ "$2" = fallow ]; then
  printf '%s\n' "Usage Error: Couldn't find a binary named fallow"
  exit 1
fi
if [ "$1" = exec ]; then
  printf '%s\n' invoked > "$FALLOW_TEST_SOURCE"
fi
exit 1"#,
    );

    assert!(
        fixture
            .run(&[("FALLOW_TEST_SOURCE", &source_file)])
            .success()
    );
    assert!(
        !source_file.exists(),
        "failed yarn bin lookup must not select yarn exec"
    );
}
