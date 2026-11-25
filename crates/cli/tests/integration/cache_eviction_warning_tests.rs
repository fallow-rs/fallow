//! A parse cache cap below the working set makes each run parse part of an
//! unchanged tree again. The save warns once on stderr, so the cache does not
//! look broken. A quiet run does not print the warning, and the machine output
//! on stdout stays one document.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests and benches use unwrap and expect to keep fixture setup concise"
)]

use std::fmt::Write as _;

use crate::common::{CommandOutput, fallow_command, parse_json};
use tempfile::TempDir;

const WARNING: &str = "Parse cache is full";

/// Files and exports for each file. The cache entries of this project encode
/// to more than 1 MiB, the smallest cap that `FALLOW_CACHE_MAX_SIZE` takes.
const FILE_COUNT: usize = 300;
const EXPORTS_PER_FILE: usize = 120;

fn project() -> TempDir {
    let dir = TempDir::new().expect("temp project");
    let root = dir.path();
    std::fs::create_dir_all(root.join("src")).expect("src dir");
    std::fs::write(
        root.join("package.json"),
        r#"{"name":"cache-eviction-warning","version":"1.0.0","private":true,"main":"src/index.ts"}"#,
    )
    .expect("package.json");
    let mut index = String::new();
    for file in 0..FILE_COUNT {
        let mut source = String::new();
        for export in 0..EXPORTS_PER_FILE {
            writeln!(source, "export const value_{file}_{export} = {export};").expect("write");
        }
        std::fs::write(root.join(format!("src/m{file}.ts")), source).expect("module");
        writeln!(index, "import './m{file}';").expect("write");
    }
    std::fs::write(root.join("src/index.ts"), index).expect("entry");
    dir
}

/// Run `dead-code` with tracing at its default level, which the harness
/// otherwise turns off with an empty `RUST_LOG`.
fn run(dir: &TempDir, cap_mb: &str, args: &[&str]) -> CommandOutput {
    let mut cmd = fallow_command();
    cmd.env_remove("RUST_LOG")
        .env("FALLOW_CACHE_MAX_SIZE", cap_mb)
        .env("FALLOW_CACHE_DIR", dir.path().join(".cache-under-test"))
        .arg("dead-code")
        .arg("--root")
        .arg(dir.path())
        .args(args);
    CommandOutput::from_process(cmd.output().expect("run fallow"))
}

#[test]
fn a_cap_below_the_working_set_warns_once_on_stderr() {
    let dir = project();
    run(&dir, "1", &["--format", "json", "--quiet"]);

    let output = run(&dir, "1", &[]);
    assert_eq!(
        output.stderr.matches(WARNING).count(),
        1,
        "one warning for each run: {}",
        output.stderr
    );
    assert!(
        output.stderr.contains("`cache.maxSizeMb`"),
        "{}",
        output.stderr
    );
    assert!(
        output.stderr.contains("FALLOW_CACHE_MAX_SIZE"),
        "{}",
        output.stderr
    );
    assert!(output.stderr.contains("the 1 MiB cap"), "{}", output.stderr);
}

#[test]
fn json_output_stays_one_document_and_quiet_hides_the_warning() {
    let dir = project();
    run(&dir, "1", &["--format", "json", "--quiet"]);

    let loud = run(&dir, "1", &["--format", "json"]);
    assert!(loud.stderr.contains(WARNING), "{}", loud.stderr);
    assert!(!loud.stdout.contains(WARNING));
    let _ = parse_json(&loud);

    let quiet = run(&dir, "1", &["--format", "json", "--quiet"]);
    assert!(!quiet.stderr.contains(WARNING), "{}", quiet.stderr);
    let _ = parse_json(&quiet);
}

#[test]
fn a_cap_that_holds_the_working_set_does_not_warn() {
    let dir = project();
    run(&dir, "64", &["--format", "json", "--quiet"]);

    let output = run(&dir, "64", &[]);
    assert!(!output.stderr.contains(WARNING), "{}", output.stderr);
}
