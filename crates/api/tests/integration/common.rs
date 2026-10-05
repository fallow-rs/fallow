//! Fixture helpers that more than one test module uses.

#![expect(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests use unwrap and expect to keep fixture setup concise"
)]

use std::fs;
use std::path::Path;
use std::process::Command;

/// Run `git` in `root` with a fixed identity and no signing, and fail the test
/// when the command fails.
pub fn git(root: &Path, args: &[&str]) {
    let output = Command::new("git")
        .args([
            "-c",
            "commit.gpgsign=false",
            "-c",
            "user.name=fallow",
            "-c",
            "user.email=fallow@example.invalid",
        ])
        .args(args)
        .current_dir(root)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .expect("run git");
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Write `content` to `path` below `root`, and create the parent directories.
pub fn write(root: &Path, path: &str, content: &str) {
    let target = root.join(path);
    fs::create_dir_all(target.parent().unwrap()).unwrap();
    fs::write(target, content).unwrap();
}

/// Stage every change in `root` and commit it.
pub fn commit(root: &Path, message: &str) {
    git(root, &["add", "-A"]);
    git(root, &["commit", "-q", "-m", message]);
}

/// Run one test again in a child process with the sidecar path in its
/// environment. The type-aware session reads the path only from the process
/// environment, and a test must not change the environment of its own process.
///
/// `test_name` is the full test name in the `integration` binary. The child
/// run filters on it with `--exact`, so it includes the module path.
pub fn rerun_with_type_aware_sidecar(test_name: &str) {
    let mut sidecar = std::path::PathBuf::from(
        std::env::var_os("CARGO_MANIFEST_DIR").expect("cargo sets CARGO_MANIFEST_DIR"),
    );
    sidecar.pop();
    sidecar.pop();
    sidecar.push("tools/type-aware-sidecar/fallow-type-aware.mjs");

    let mut command = Command::new(std::env::current_exe().expect("test binary path"));
    command.args(["--exact", test_name, "--nocapture", "--test-threads=1"]);
    #[cfg(windows)]
    {
        let path = std::env::var_os("PATH").expect("PATH must contain the Node.js runtime");
        let node = std::env::split_paths(&path)
            .map(|entry| entry.join("node.exe"))
            .find(|candidate| candidate.is_file())
            .expect("Node.js executable must be available for type-aware tests");
        command
            .env("FALLOW_TYPE_AWARE_BIN", node)
            .env("FALLOW_TYPE_AWARE_SCRIPT", &sidecar);
    }
    #[cfg(not(windows))]
    command.env("FALLOW_TYPE_AWARE_BIN", &sidecar);

    let output = command.output().expect("run the test in a child process");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success() && stdout.contains("1 passed"),
        "child run failed:\n{stdout}\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
}
