#![cfg(unix)]

use std::io::Write;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

const GATE: &str = include_str!("../../src/setup_hooks/fallow-gate.sh");

fn write_executable(path: &Path, text: &str) {
    std::fs::write(path, text).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

fn no_jq_tools(bin: &Path) {
    std::fs::create_dir_all(bin).unwrap();
    for tool in [
        "bash", "git", "cat", "dirname", "basename", "pwd", "tr", "sort", "head", "sed", "grep",
        "mktemp", "mkdir", "mv", "rm",
    ] {
        let result = Command::new("/bin/sh")
            .args(["-c", &format!("command -v {tool}")])
            .output()
            .unwrap();
        assert!(result.status.success(), "missing test prerequisite {tool}");
        let target = String::from_utf8(result.stdout).unwrap();
        symlink(target.trim(), bin.join(tool)).unwrap();
    }
}

fn run_gate(root: &Path, script: &Path, bin: &Path, payload: &[u8]) -> Output {
    let mut child = Command::new(bin.join("bash"))
        .arg(script)
        .current_dir(root)
        .env("PATH", bin)
        .env("HOME", root)
        .env("FALLOW_GATE_MIN_VERSION", "")
        .env("FALLOW_TEST_NATIVE", crate::common::fallow_bin())
        .env("FALLOW_TEST_REPORT", root.join(".stub-report"))
        .env_remove("FALLOW_GATE_OUTPUT")
        .env_remove("FALLOW_GATE_REPORT")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(payload).unwrap();
    child.wait_with_output().unwrap()
}

fn fixture() -> (tempfile::TempDir, PathBuf, PathBuf, PathBuf) {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("project with spaces");
    std::fs::create_dir_all(&root).unwrap();
    assert!(
        Command::new("git")
            .args(["init", "-q"])
            .current_dir(&root)
            .status()
            .unwrap()
            .success()
    );
    let hooks = root.join(".codex/hooks");
    std::fs::create_dir_all(&hooks).unwrap();
    let script = hooks.join("fallow-gate.sh");
    std::fs::write(&script, GATE).unwrap();
    let bin = temp.path().join("bin");
    no_jq_tools(&bin);
    write_executable(
        &bin.join("fallow"),
        &format!(
            r#"#!/bin/sh
if [ "$1" = hooks ]; then exec "$FALLOW_TEST_NATIVE" "$@"; fi
if [ "$1" = --version ]; then echo 'fallow {}'; exit 0; fi
cat "$FALLOW_TEST_REPORT"
exit 1
"#,
            env!("CARGO_PKG_VERSION")
        ),
    );
    std::fs::write(root.join(".stub-report"), r#"{"verdict":"fail","unused_exports":[{"path":"src/api.ts","export_name":"unused","introduced":true,"actions":[]}]}"#).unwrap();
    (temp, root, script, bin)
}

#[test]
fn native_json_gate_blocks_without_jq_and_names_the_finding() {
    let (temp, root, script, bin) = fixture();
    let payload = serde_json::to_vec(
        &serde_json::json!({"cwd": root, "tool_input": {"command": "git commit -m test"}}),
    )
    .unwrap();
    let output = run_gate(&root, &script, &bin, &payload);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(2), "{stderr}");
    assert!(
        stderr.contains("unused_exports src/api.ts unused"),
        "{stderr}"
    );
    let report: serde_json::Value =
        serde_json::from_slice(&std::fs::read(root.join(".fallow/gate-report.json")).unwrap())
            .unwrap();
    assert_eq!(report["verdict"], "fail");

    let installed = Command::new(crate::common::fallow_bin())
        .args([
            "hooks", "install", "--target", "agent", "--agent", "codex", "--root",
        ])
        .arg(&root)
        .env("HOME", &root)
        .output()
        .unwrap();
    assert!(
        installed.status.success(),
        "{}",
        String::from_utf8_lossy(&installed.stderr)
    );
    let status_bin = temp.path().join("status-bin");
    no_jq_tools(&status_bin);
    symlink(crate::common::fallow_bin(), status_bin.join("fallow")).unwrap();
    let read_status = || {
        let result = Command::new(crate::common::fallow_bin())
            .args(["agent", "status", "--format", "json", "--root"])
            .arg(&root)
            .env("HOME", &root)
            .env("PATH", &status_bin)
            .env("FALLOW_TEST_NATIVE", crate::common::fallow_bin())
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        serde_json::from_slice::<serde_json::Value>(&result.stdout).unwrap()
    };
    let original = read_status();
    let row = original["surfaces"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["path"] == ".codex/hooks/fallow-gate.sh")
        .unwrap();
    assert_eq!(row["state"], "installed", "{original}");
    let sentinel = root.join("untrusted-hook-executed");
    let installed_script = std::fs::read_to_string(&script).unwrap();
    std::fs::write(
        &script,
        installed_script.replacen(
            "set -euo pipefail",
            &format!(
                "printf unsafe > '{}'\nset -euo pipefail",
                sentinel.display()
            ),
            1,
        ),
    )
    .unwrap();
    let status = read_status();

    assert!(
        !sentinel.exists(),
        "agent status executed an untrusted hook"
    );
    let row = status["surfaces"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["path"] == ".codex/hooks/fallow-gate.sh")
        .unwrap();
    assert_eq!(row["state"], "stale", "{status}");
    assert!(
        status["next_actions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|action| action["id"] == "gate-script-refresh")
    );
    assert!(
        status["next_actions"]
            .as_array()
            .unwrap()
            .iter()
            .all(|action| action["id"] != "gate-requires-jq"),
        "{status}"
    );
}

#[test]
fn native_json_gate_handles_safe_calls_and_invalid_external_data() {
    let (_temp, root, script, bin) = fixture();
    let payload = |command: &str| {
        serde_json::to_vec(&serde_json::json!({"cwd": root, "tool_input": {"command": command}}))
            .unwrap()
    };
    let skipped = run_gate(&root, &script, &bin, &payload("git status --short"));
    assert!(skipped.status.success());
    let blocked = run_gate(
        &root,
        &script,
        &bin,
        &payload("git commit -m 'escaped \\\"message\\\"'"),
    );
    assert_eq!(blocked.status.code(), Some(2));
    for verdict in ["pass", "warn"] {
        std::fs::write(
            root.join(".stub-report"),
            format!(r#"{{"verdict":"{verdict}"}}"#),
        )
        .unwrap();
        assert!(
            run_gate(&root, &script, &bin, &payload("git push"))
                .status
                .success()
        );
    }
    std::fs::write(
        root.join(".stub-report"),
        r#"{"error":true,"message":"missing config"}"#,
    )
    .unwrap();
    let error = run_gate(&root, &script, &bin, &payload("git commit -m test"));
    assert!(error.status.success());
    assert!(String::from_utf8_lossy(&error.stderr).contains("runtime error (missing config)"));
    std::fs::write(root.join(".stub-report"), "invalid JSON").unwrap();
    let invalid = run_gate(&root, &script, &bin, &payload("git push"));
    assert!(invalid.status.success());
    assert!(String::from_utf8_lossy(&invalid.stderr).contains("cannot read audit report"));
    for invalid in [
        br#"{"tool_input":{"command":["git","push"]}}"#.as_slice(),
        br#"{"tool_input":{"command":"git\u0000push"}}"#.as_slice(),
    ] {
        let result = run_gate(&root, &script, &bin, invalid);
        assert!(result.status.success());
        assert!(String::from_utf8_lossy(&result.stderr).contains("invalid tool input"));
    }
}

#[test]
fn native_json_findings_match_null_severity_fallback_and_sanitize_controls() {
    let mut child = Command::new(crate::common::fallow_bin())
        .args(["hooks", "gate-runtime", "findings"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(br#"{"attribution":{"gate":"all"},"unused_exports":[{"path":"src/warn.ts","actions":[],"effective_severity":null,"severity":"warn"},{"path":"src/\u001b[31merror.ts","export_name":"line\nname","actions":[],"severity":"error"}]}"#).unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "unused_exports src/ [31merror.ts line name\n"
    );
}

#[test]
fn native_json_gate_discovers_project_helper_from_script_root() {
    let (temp, root, script, bin) = fixture();
    let local = root.join("node_modules/.bin");
    std::fs::create_dir_all(&local).unwrap();
    std::fs::rename(bin.join("fallow"), local.join("fallow")).unwrap();
    let payload = serde_json::to_vec(
        &serde_json::json!({"cwd": root, "tool_input": {"command": "git commit -m test"}}),
    )
    .unwrap();
    let mut child = Command::new(bin.join("bash"))
        .arg(&script)
        .current_dir(temp.path())
        .env("PATH", &bin)
        .env("HOME", temp.path())
        .env("FALLOW_GATE_MIN_VERSION", "")
        .env("FALLOW_TEST_NATIVE", crate::common::fallow_bin())
        .env("FALLOW_TEST_REPORT", root.join(".stub-report"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(&payload).unwrap();
    let output = child.wait_with_output().unwrap();
    assert_eq!(
        output.status.code(),
        Some(2),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stderr).contains("unused_exports src/api.ts unused"));
}

#[test]
fn native_json_status_inventories_project_launchers_without_executing_them() {
    let (_temp, root, _script, bin) = fixture();
    let install = Command::new(crate::common::fallow_bin())
        .args([
            "hooks", "install", "--target", "agent", "--agent", "codex", "--root",
        ])
        .arg(&root)
        .env("HOME", &root)
        .output()
        .unwrap();
    assert!(install.status.success());
    let sentinel = root.join("project-code-executed");
    std::fs::remove_file(bin.join("fallow")).unwrap();
    let local = root.join("node_modules/.bin");
    std::fs::create_dir_all(&local).unwrap();
    std::fs::write(root.join("package.json"), r#"{"name":"local-reader"}"#).unwrap();
    let malicious = format!(
        "#!/bin/sh\nprintf unsafe > '{}'\nprintf 'fallow-gate-json-v1\\n'\n",
        sentinel.display()
    );
    write_executable(&local.join("fallow"), &malicious);
    for tool in ["yarn", "npx"] {
        write_executable(&bin.join(tool), &malicious);
    }
    for mode in ["project-local", "project-on-path", "package-manager"] {
        if mode == "project-on-path" {
            symlink(local.join("fallow"), bin.join("fallow")).unwrap();
        }
        if mode == "package-manager" {
            std::fs::remove_file(bin.join("fallow")).unwrap();
            std::fs::remove_file(local.join("fallow")).unwrap();
        }
        let result = Command::new(crate::common::fallow_bin())
            .args(["agent", "status", "--root"])
            .arg(&root)
            .args(["--format", "json"])
            .env("HOME", &root)
            .env("PATH", &bin)
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        assert!(!sentinel.exists(), "status executed a project launcher");
        let status: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
        assert!(
            status["next_actions"]
                .as_array()
                .unwrap()
                .iter()
                .any(|action| action["id"] == "gate-reader-unverified"),
            "{status}"
        );
        assert!(
            status["next_actions"]
                .as_array()
                .unwrap()
                .iter()
                .all(|action| action["id"] != "gate-requires-jq"
                    && action["id"] != "gate-json-reader"),
            "{status}"
        );
    }
}

#[test]
fn native_json_status_preserves_unverified_ancestor_project_launchers() {
    let (_temp, root, _script, bin) = fixture();
    let app = root.join("packages/app");
    std::fs::create_dir_all(&app).unwrap();
    let installed = Command::new(crate::common::fallow_bin())
        .args([
            "hooks", "install", "--target", "agent", "--agent", "codex", "--root",
        ])
        .arg(&app)
        .env("HOME", &root)
        .output()
        .unwrap();
    assert!(installed.status.success());
    let local = root.join("node_modules/.bin");
    std::fs::create_dir_all(&local).unwrap();
    let sentinel = root.join("ancestor-code-executed");
    write_executable(
        &local.join("fallow"),
        &format!(
            "#!/bin/sh\nprintf unsafe > '{}'\necho 'fallow 1.0.0'\n",
            sentinel.display()
        ),
    );
    let path = format!("{}:{}", local.display(), bin.display());
    let result = Command::new(crate::common::fallow_bin())
        .args(["agent", "status", "--root"])
        .arg(&app)
        .args(["--format", "json"])
        .env("HOME", &root)
        .env("PATH", path)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(
        !sentinel.exists(),
        "status executed the workspace ancestor launcher"
    );
    let status: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
    assert!(status["root"].as_str().unwrap().ends_with("/packages/app"));
    assert!(
        status["next_actions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|action| action["id"] == "gate-reader-unverified"),
        "{status}"
    );
}

#[test]
fn native_json_gate_keeps_blocking_when_findings_exceed_a_pipe_buffer() {
    const FINDINGS: usize = 512;
    const NAME_LENGTH: usize = 1024;
    const DISPLAY_LIMIT: usize = 50;

    let (_temp, root, script, bin) = fixture();
    let findings: Vec<serde_json::Value> = (0..FINDINGS)
        .map(|index| {
            serde_json::json!({
                "path": format!("src/item-{index}.ts"),
                "export_name": "x".repeat(NAME_LENGTH),
                "introduced": true,
                "actions": []
            })
        })
        .collect();
    let report = serde_json::json!({"verdict": "fail", "unused_exports": findings});
    std::fs::write(
        root.join(".stub-report"),
        serde_json::to_vec(&report).unwrap(),
    )
    .unwrap();
    let payload = serde_json::to_vec(
        &serde_json::json!({"cwd": root, "tool_input": {"command": "git commit -m test"}}),
    )
    .unwrap();

    let output = run_gate(&root, &script, &bin, &payload);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(2), "{stderr}");
    assert_eq!(
        stderr
            .lines()
            .filter(|line| line.starts_with("  unused_exports "))
            .count(),
        DISPLAY_LIMIT
    );
    assert!(stderr.contains("more in the full report"), "{stderr}");
    assert!(stderr.contains("full report:"), "{stderr}");
    let preserved: serde_json::Value =
        serde_json::from_slice(&std::fs::read(root.join(".fallow/gate-report.json")).unwrap())
            .unwrap();
    assert_eq!(preserved, report);
}

#[test]
fn native_json_status_does_not_certify_a_reader_outside_the_audit_root() {
    let (_temp, root, _script, bin) = fixture();
    let app = root.join("packages/app");
    std::fs::create_dir_all(&app).unwrap();
    assert!(
        Command::new("git")
            .args(["init", "-q"])
            .current_dir(&app)
            .status()
            .unwrap()
            .success()
    );
    let installed = Command::new(crate::common::fallow_bin())
        .args([
            "hooks", "install", "--target", "agent", "--agent", "codex", "--root",
        ])
        .arg(&app)
        .env("HOME", &root)
        .output()
        .unwrap();
    assert!(
        installed.status.success(),
        "{}",
        String::from_utf8_lossy(&installed.stderr)
    );
    let local = root.join("node_modules/.bin");
    std::fs::create_dir_all(&local).unwrap();
    symlink(crate::common::fallow_bin(), local.join("fallow")).unwrap();
    std::fs::remove_file(bin.join("fallow")).unwrap();
    let payload = serde_json::to_vec(
        &serde_json::json!({"cwd": app, "tool_input": {"command": "git commit -m test"}}),
    )
    .unwrap();
    let output = run_gate(
        &app,
        &app.join(".codex/hooks/fallow-gate.sh"),
        &bin,
        &payload,
    );
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("fallow binary not found"));
    let result = Command::new(crate::common::fallow_bin())
        .args(["agent", "status", "--root"])
        .arg(&app)
        .args(["--format", "json"])
        .env("HOME", &root)
        .env("PATH", &bin)
        .output()
        .unwrap();
    assert!(result.status.success());
    let status: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
    let gate = status["surfaces"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["path"] == ".codex/hooks/fallow-gate.sh")
        .unwrap();
    assert_eq!(gate["state"], "stale", "{status}");
    assert!(
        status["next_actions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|action| action["id"] == "gate-reader-unverified"),
        "{status}"
    );
}

#[test]
fn native_json_status_does_not_execute_ancestor_owned_tools() {
    for owner in ["repository", "workspace"] {
        for layout in [
            "relative-owned-symlink",
            "direct",
            "owned-symlink",
            "external-symlink",
        ] {
            let (temp, root, _script, bin) = fixture();
            let workspace = if owner == "repository" {
                root
            } else {
                temp.path().join("workspace")
            };
            let app = workspace.join("packages/app");
            std::fs::create_dir_all(&app).unwrap();
            if owner == "workspace" {
                std::fs::write(
                    workspace.join("package.json"),
                    r#"{"private":true,"workspaces":["packages/*"]}"#,
                )
                .unwrap();
            }
            let installed = Command::new(crate::common::fallow_bin())
                .args([
                    "hooks", "install", "--target", "agent", "--agent", "codex", "--root",
                ])
                .arg(&app)
                .env("HOME", &workspace)
                .output()
                .unwrap();
            assert!(installed.status.success());
            let tools = workspace.join("tools");
            std::fs::create_dir_all(&tools).unwrap();
            let sentinel = workspace.join("ancestor-tool-executed");
            let launcher = tools.join("fallow");
            let text = format!(
                "#!/bin/sh\nprintf unsafe > '{}'\necho 'fallow 1.0.0'\n",
                sentinel.display()
            );
            std::fs::remove_file(bin.join("fallow")).unwrap();
            if matches!(layout, "owned-symlink" | "relative-owned-symlink") {
                let external = temp.path().join("outside-fallow");
                write_executable(&external, &text);
                symlink(external, &launcher).unwrap();
            } else {
                write_executable(&launcher, &text);
            }
            let path = if layout == "relative-owned-symlink" {
                std::fs::create_dir(temp.path().join("launch")).unwrap();
                format!(
                    "launch/../{}/tools:{}",
                    workspace.strip_prefix(temp.path()).unwrap().display(),
                    bin.display()
                )
            } else if layout == "external-symlink" {
                symlink(&launcher, bin.join("fallow")).unwrap();
                bin.display().to_string()
            } else {
                format!("{}:{}", tools.display(), bin.display())
            };
            let result = Command::new(crate::common::fallow_bin())
                .args(["agent", "status", "--root"])
                .arg(&app)
                .args(["--format", "json"])
                .current_dir(temp.path())
                .env("HOME", &workspace)
                .env("PATH", path)
                .output()
                .unwrap();
            assert!(result.status.success());
            assert!(
                !sentinel.exists(),
                "status executed {owner} code through {layout}"
            );
            let status: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
            assert!(
                status["next_actions"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|action| action["id"] == "gate-reader-unverified"),
                "{status}"
            );
        }
    }
}

/// Replaces the fallow stub with `yarn` and `npx` stubs that record each
/// call and the stdin they receive, and adds a package.json so the gate
/// tries them as launchers.
fn launcher_probe_fixture() -> (tempfile::TempDir, PathBuf, PathBuf, PathBuf, PathBuf) {
    let (temp, root, script, bin) = fixture();
    std::fs::remove_file(bin.join("fallow")).unwrap();
    std::fs::write(root.join("package.json"), "{}").unwrap();
    let marker = temp.path().join("launcher-probe-calls");
    for launcher in ["yarn", "npx"] {
        write_executable(
            &bin.join(launcher),
            &format!(
                "#!/bin/sh\necho {launcher} >> '{marker}'\ncat >> '{marker}'\nexit 1\n",
                marker = marker.display()
            ),
        );
    }
    (temp, root, script, bin, marker)
}

#[test]
fn gate_skips_launcher_probes_for_a_command_that_is_not_a_git_write() {
    let (_temp, root, script, bin, marker) = launcher_probe_fixture();
    for command in ["ls -la", "cargo test", "git status --short"] {
        let payload = serde_json::to_vec(
            &serde_json::json!({"cwd": root, "tool_input": {"command": command}}),
        )
        .unwrap();
        let output = run_gate(&root, &script, &bin, &payload);
        assert_eq!(
            output.status.code(),
            Some(0),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            !marker.exists(),
            "`{command}` ran a launcher probe: {}",
            std::fs::read_to_string(&marker).unwrap_or_default()
        );
    }

    // A git write still searches for a reader, so the check above is not vacuous.
    let payload = serde_json::to_vec(
        &serde_json::json!({"cwd": root, "tool_input": {"command": "git commit -m test"}}),
    )
    .unwrap();
    let output = run_gate(&root, &script, &bin, &payload);
    assert_eq!(output.status.code(), Some(0));
    assert!(marker.exists(), "a git write must probe the launchers");
}

#[test]
fn gate_launcher_probes_never_read_the_hook_payload() {
    let (_temp, root, script, bin, marker) = launcher_probe_fixture();
    let payload = serde_json::to_vec(
        &serde_json::json!({"cwd": root, "tool_input": {"command": "git push origin main"}}),
    )
    .unwrap();
    let output = run_gate(&root, &script, &bin, &payload);
    assert_eq!(output.status.code(), Some(0));
    let calls = std::fs::read_to_string(&marker).unwrap();
    assert!(calls.contains("npx"), "{calls}");
    assert!(!calls.contains("tool_input"), "a probe read stdin: {calls}");
}
