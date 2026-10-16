#![allow(
    clippy::expect_used,
    reason = "tests use expect to keep fixture setup concise"
)]

//! Guard for the dependency features of the shipped binaries.
//!
//! regex-automata's `dfa-build` feature makes the regex engine build a full
//! dense DFA for each small pattern. Fallow compiles hundreds of globs and
//! regexes in a run, and the feature made each glob compile cost about three
//! times as much. Cargo unifies features, so one dependency that turns it on
//! (as tracing-subscriber's `env-filter` did through `matchers`) brings it
//! back for every regex in the binary. Dev-dependencies do not count: their
//! features do not reach a release build.

use std::path::Path;
use std::process::Command;

/// The features of `regex-automata` in the normal and build graph of `package`.
fn regex_automata_features(package: &str) -> String {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../Cargo.toml");
    let output = Command::new(env!("CARGO"))
        .arg("tree")
        .arg("--manifest-path")
        .arg(&manifest)
        .args(["--locked", "--offline", "--package", package])
        .args([
            "--edges",
            "normal,build,features",
            "--invert",
            "regex-automata",
        ])
        .output()
        .expect("cargo tree runs");
    assert!(
        output.status.success(),
        "cargo tree failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("cargo tree prints UTF-8")
}

#[test]
#[cfg_attr(miri, ignore = "runs cargo")]
fn shipped_binaries_do_not_build_a_full_regex_dfa() {
    for package in ["fallow-cli", "fallow-multicall"] {
        let tree = regex_automata_features(package);
        assert!(
            tree.contains("regex-automata"),
            "{package}: cargo tree did not list regex-automata:\n{tree}"
        );
        assert!(
            !tree.contains("feature \"dfa-build\""),
            "{package} turns on regex-automata's dfa-build feature. Find the \
             dependency that enables it in this tree and remove that path:\n{tree}"
        );
    }
}
