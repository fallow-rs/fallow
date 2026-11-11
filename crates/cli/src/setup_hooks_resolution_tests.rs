//! Tests for the install lookup of the generated gate script.
//!
//! Each test runs the rendered script in bash with a reduced `PATH` and stub
//! launchers. A stub below the version floor makes the lookup visible: the
//! block message names the install that the script selected.

use std::path::Path;

use tempfile::tempdir;

use super::rendered_gate_script;

/// Runs the rendered gate from inside `project`, with `PATH` reduced to
/// `extra_bin` plus the system directories, so no globally installed fallow
/// can satisfy the resolution the test is probing. A fake fallow below the
/// version floor makes resolution observable: reaching the floor check at
/// all proves the binary was found.
fn probe_gate_resolution(project: &Path, extra_bin: &Path) -> std::process::Output {
    use std::io::Write;

    // jq is linked INTO the probe directory rather than having its own
    // directory added to PATH: on a developer machine that directory also
    // holds the globally installed fallow, which would satisfy the very
    // resolution step under test.
    if let Some(jq) = std::process::Command::new("sh")
        .args(["-c", "command -v jq"])
        .output()
        .ok()
        .filter(|out| out.status.success())
        .map(|out| String::from_utf8_lossy(&out.stdout).trim().to_string())
        .filter(|path| !path.is_empty())
    {
        let _ = std::os::unix::fs::symlink(jq, extra_bin.join("jq"));
    }
    let path = format!("{}:/usr/bin:/bin", extra_bin.display());

    let script_path = project.join("fallow-gate.sh");
    std::fs::write(&script_path, rendered_gate_script()).unwrap();

    let mut child = std::process::Command::new("bash")
        .arg(&script_path)
        .current_dir(project)
        .env("PATH", &path)
        // An unreachable floor makes resolution observable: the block
        // message names the runner the script chose.
        .env("FALLOW_GATE_MIN_VERSION", "999.0.0")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("spawn bash");
    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(br#"{"tool_input":{"command":"git commit -m test"}}"#)
        .unwrap();
    child.wait_with_output().expect("wait")
}

fn write_stale_fallow(path: &Path, body: &str) {
    use std::os::unix::fs::PermissionsExt;

    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, body).unwrap();
    let mut perms = std::fs::metadata(path).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(path, perms).unwrap();
}

/// A real installed Git hook keeps its caller's PATH, so a project-local
/// install is invisible to `command -v`. Reaching the version floor proves
/// the launcher was resolved rather than the audit silently skipped.
#[test]
fn gate_resolves_a_project_local_launcher_absent_from_path() {
    let tmp = tempdir().unwrap();
    let project = tmp.path();
    write_stale_fallow(
        &project.join("node_modules/.bin/fallow"),
        "#!/bin/sh\nif [ \"$1\" = \"--version\" ]; then echo 'fallow 2.30.0'; exit 0; fi\nexit 0\n",
    );
    std::fs::create_dir_all(project.join("empty-bin")).unwrap();

    let output = probe_gate_resolution(project, &project.join("empty-bin"));
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        stderr.contains("./node_modules/.bin/fallow is fallow 2.30.0"),
        "the project-local launcher must be the runner the script chose: {stderr}"
    );
}

/// Yarn Plug'n'Play has no `node_modules/.bin` at all, so neither the
/// launcher check nor `npx --no-install` can see the install.
#[test]
fn gate_resolves_a_yarn_plug_n_play_install() {
    let tmp = tempdir().unwrap();
    let project = tmp.path();
    let bin = project.join("bin");
    write_stale_fallow(
        &bin.join("yarn"),
        "#!/bin/sh\ncase \"$1 $2\" in\n  'bin fallow') echo \"$PWD/.yarn/fallow\"; exit 0 ;;\nesac\nif [ \"$1\" = exec ]; then\n  shift 3\n  if [ \"$1\" = \"--version\" ]; then echo 'fallow 2.30.0'; exit 0; fi\n  exit 0\nfi\nexit 1\n",
    );
    assert!(!project.join("node_modules").exists());

    let output = probe_gate_resolution(project, &bin);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        stderr.contains("yarn exec fallow is fallow 2.30.0"),
        "the Plug'n'Play install must be the runner the script chose: {stderr}"
    );
}

/// A `fallow` launcher on PATH can fail to start, for example a global npm
/// shim whose package is gone (`MODULE_NOT_FOUND`). The gate must then use
/// the working project-local install. Without the version check the gate
/// selects the broken launcher, reads an empty report and skips the audit.
#[test]
fn gate_skips_a_path_launcher_that_cannot_run() {
    let tmp = tempdir().unwrap();
    let project = tmp.path();
    let bin = project.join("bin");
    write_stale_fallow(
        &bin.join("fallow"),
        "#!/bin/sh\necho \"Error: Cannot find module 'fallow'\" >&2\nexit 1\n",
    );
    write_stale_fallow(
        &project.join("node_modules/.bin/fallow"),
        "#!/bin/sh\nif [ \"$1\" = \"--version\" ]; then echo 'fallow 2.30.0'; exit 0; fi\nexit 0\n",
    );

    let output = probe_gate_resolution(project, &bin);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert_eq!(output.status.code(), Some(2), "{stderr}");
    assert!(
        stderr.contains("./node_modules/.bin/fallow is fallow 2.30.0"),
        "the working local install must be the runner the script chose: {stderr}"
    );
}

/// pnpm often installs only a project-local launcher. When that launcher
/// exits with `MODULE_NOT_FOUND` and no other install works, the gate must
/// block and name the launcher and its error. Before, the gate printed
/// "binary not found", exited 0 and let the commit pass without an audit.
#[test]
fn gate_blocks_when_the_only_launcher_cannot_run() {
    let tmp = tempdir().unwrap();
    let project = tmp.path();
    write_stale_fallow(
        &project.join("node_modules/.bin/fallow"),
        "#!/bin/sh\necho \"Error: Cannot find module '@fallow-cli/darwin-arm64'\" >&2\necho 'Require stack:' >&2\nexit 1\n",
    );
    std::fs::create_dir_all(project.join("empty-bin")).unwrap();

    let output = probe_gate_resolution(project, &project.join("empty-bin"));
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert_eq!(output.status.code(), Some(2), "{stderr}");
    assert!(
        stderr.contains(
            "./node_modules/.bin/fallow cannot print its fallow version (Error: Cannot find module '@fallow-cli/darwin-arm64')"
        ),
        "the gate must name the broken launcher and its first error line: {stderr}"
    );
    assert!(!stderr.contains("Require stack"), "{stderr}");
    assert!(!stderr.contains("binary not found"), "{stderr}");
}

/// The gate reports the first launcher that failed, not a later one.
#[test]
fn gate_names_the_first_broken_launcher() {
    let tmp = tempdir().unwrap();
    let project = tmp.path();
    let bin = project.join("bin");
    write_stale_fallow(
        &bin.join("fallow"),
        "#!/bin/sh\necho 'path shim broken' >&2\nexit 1\n",
    );
    write_stale_fallow(
        &project.join("node_modules/.bin/fallow"),
        "#!/bin/sh\necho 'local launcher broken' >&2\nexit 1\n",
    );

    let output = probe_gate_resolution(project, &bin);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert_eq!(output.status.code(), Some(2), "{stderr}");
    assert!(stderr.contains("(path shim broken)"), "{stderr}");
    assert!(stderr.contains("bin/fallow cannot print"), "{stderr}");
}

/// With no launcher at all, the gate keeps the commit and names the skip.
#[test]
fn gate_names_the_skip_when_no_launcher_exists() {
    let tmp = tempdir().unwrap();
    let project = tmp.path();
    std::fs::create_dir_all(project.join("empty-bin")).unwrap();

    let output = probe_gate_resolution(project, &project.join("empty-bin"));
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert_eq!(output.status.code(), Some(0), "{stderr}");
    assert!(
        stderr.contains("fallow binary not found"),
        "the gate must name the skip: {stderr}"
    );
}
