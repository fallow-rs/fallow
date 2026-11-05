//! The runner-level read trace: the project files that a cold run of the
//! test runner reads or loads.
//!
//! The `read-trace` feature of `fallow-graph` sees only the reads of the
//! analysis. The tests of a mutant run in the runner process, so this trace
//! starts the runner itself, the same way as the `test` script of a project:
//!
//! - Vitest runs under Node with `--import` of `trace-node.mjs`. The hook
//!   records each `fs` read and each module that the Node loader loads, in
//!   the main process and in each worker. Vite resolves `tsconfig.json` in
//!   native code, so this trace does not see that read.
//! - `bun test` runs with `--preload` of `trace-bun.ts`. The preload records
//!   each module in `require.cache`, but no `fs` read of a test. Bun reads
//!   its config files in native code, so [`super::template::BUN_NATIVE_READS`]
//!   adds the config files that Bun documents. That part is a list, not a
//!   trace.

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;
use std::process::Command;

use super::template::{BUN_NATIVE_READS, Runner};

const NODE_HOOK: &str = include_str!("runner-trace/trace-node.mjs");
const BUN_PRELOAD: &str = include_str!("runner-trace/trace-bun.ts");

/// The project directories whose files are not inputs of a test: installed
/// packages (the lockfile part of the key stands for them), the Fallow cache
/// and the git store.
const NOT_INPUTS: [&str; 3] = ["node_modules/", ".fallow/", ".git/"];

/// The root-relative paths of the project files that a cold run of `runner`
/// reads, sorted.
pub fn trace(root: &Path, runner: Runner) -> BTreeSet<String> {
    let scratch = tempfile::tempdir().expect("dir");
    let out = scratch.path().join("trace.txt");
    let mut command = match runner {
        Runner::Vitest => {
            let hook = scratch.path().join("trace-node.mjs");
            fs::write(&hook, NODE_HOOK).expect("write");
            let mut command = Command::new("node");
            command
                .arg(format!("--import={}", hook.display()))
                .args(["node_modules/vitest/vitest.mjs", "run"]);
            command
        }
        Runner::Bun => {
            let preload = scratch.path().join("trace-bun.ts");
            fs::write(&preload, BUN_PRELOAD).expect("write");
            let mut command = Command::new("bun");
            command.args(["test", "--preload"]).arg(&preload);
            command
        }
    };
    let output = command
        .current_dir(root)
        .env("FALLOW_RUNNER_TRACE", &out)
        .env_remove("NODE_OPTIONS")
        .output()
        .expect("the runner starts");
    assert!(
        output.status.success(),
        "the traced {runner:?} run fails:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let text = fs::read_to_string(&out).unwrap_or_default();
    let mut paths: BTreeSet<String> = text
        .lines()
        .filter_map(|line| project_path(root, Path::new(line)))
        .collect();
    if runner == Runner::Bun {
        paths.extend(
            BUN_NATIVE_READS
                .iter()
                .filter(|path| root.join(path).is_file())
                .map(|&path| path.to_owned()),
        );
    }
    paths
}

/// The root-relative path of a traced file of the project, or `None` for a
/// file outside the project, in [`NOT_INPUTS`], or no longer on disk (a
/// temporary file of the runner).
fn project_path(root: &Path, path: &Path) -> Option<String> {
    let path = dunce::canonicalize(path).ok()?;
    let relative = path.strip_prefix(root).ok()?.to_str()?.replace('\\', "/");
    if relative.is_empty()
        || !path.is_file()
        || NOT_INPUTS.iter().any(|dir| relative.starts_with(dir))
    {
        return None;
    }
    Some(relative)
}
