//! Nesting that the bracket guard does not stop must parse without a stack
//! overflow, on any thread.

use std::path::Path;
use std::process::Command;

use fallow_types::discover::FileId;

use crate::parse_source_to_module;

/// Far past the depth at which these shapes overflow a 16 MiB stack.
const DEEP: usize = 20_000;

/// Depth for the chain shapes whose extraction time grows faster than
/// linear. It still overflows the probe thread many times over.
const DEEP_SLOW: usize = 2_000;

/// Stack of the thread that runs the probe. A parse that does not size its
/// own stack overflows it at a few hundred levels.
const PROBE_THREAD_STACK: usize = 1 << 20;

const STACK_PROBE_ENV: &str = "FALLOW_DEEP_SHAPES_PROBE_CHILD";
const STACK_PROBE_TEST: &str = "tests::deep_shapes::deep_shapes_parse_without_a_stack_overflow";

/// Shapes that nest without brackets, each with one export to assert on.
fn deep_shapes() -> Vec<(&'static str, &'static str, String)> {
    let r = |unit: &str| unit.repeat(DEEP);
    let slow = |unit: &str| unit.repeat(DEEP_SLOW);
    vec![
        (
            "generic",
            "a.ts",
            format!("export type T = {}B{};\n", r("A<"), r(">")),
        ),
        (
            "jsx",
            "a.jsx",
            format!("export const x = {}{};\n", r("<a>"), r("</a>")),
        ),
        (
            "class",
            "a.js",
            format!(
                "export const x = {}class {{}}{};\n",
                r("class extends "),
                r(" {}")
            ),
        ),
        (
            "assign",
            "a.js",
            format!("export let a = 0;\n{}1;\n", r("a=")),
        ),
        (
            "for",
            "a.js",
            format!("export const x = 1;\n{};\n", r("for(;;)")),
        ),
        (
            "else-if",
            "a.js",
            format!("export const x = 1;\n{};\n", r("if(x);else ")),
        ),
        (
            "ternary",
            "a.js",
            format!("export const x = {}c;\n", r("a?b:")),
        ),
        (
            "arrow",
            "a.js",
            format!("export const x = {}1;\n", slow("()=>")),
        ),
        (
            "member",
            "a.js",
            format!("export const x = a{};\n", slow(".b")),
        ),
        (
            "optional",
            "a.js",
            format!("export const x = a{};\n", slow("?.b")),
        ),
        (
            "call",
            "a.js",
            format!("export const x = a{};\n", slow("()")),
        ),
        (
            "binary",
            "a.js",
            format!("export const x = a{};\n", r("+a")),
        ),
        (
            "logical",
            "a.js",
            format!("export const x = {}a;\n", r("a&&")),
        ),
    ]
}

// A stack overflow aborts the process, so the probe runs the shapes in a
// child copy of this test binary and asserts on its exit status. The child
// drops RUST_MIN_STACK (pinned in .cargo/config.toml), so its threads have the
// stack sizes of a shipped binary.
#[test]
fn deep_shapes_parse_without_a_stack_overflow() {
    if std::env::var_os(STACK_PROBE_ENV).is_some() {
        run_deep_shapes();
        return;
    }

    let current_exe = std::env::current_exe().expect("current test binary should be known");
    let output = Command::new(current_exe)
        .arg("--exact")
        .arg(STACK_PROBE_TEST)
        .arg("--nocapture")
        .env(STACK_PROBE_ENV, "1")
        .env_remove("RUST_MIN_STACK")
        .output()
        .expect("deep shape probe child should start");

    assert!(
        output.status.success(),
        "deep shape probe child failed with status {:?}\nstdout:\n{}\nstderr:\n{}",
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn run_deep_shapes() {
    std::thread::Builder::new()
        .stack_size(PROBE_THREAD_STACK)
        .spawn(|| {
            for (shape, path, source) in deep_shapes() {
                let info = parse_source_to_module(FileId(0), Path::new(path), &source, 0, true);
                assert!(!info.parse_panicked, "{shape}: the file must parse");
                assert_eq!(info.exports.len(), 1, "{shape}: the export must be found");
            }
        })
        .expect("probe thread should start")
        .join()
        .expect("every deep shape should parse");
}
