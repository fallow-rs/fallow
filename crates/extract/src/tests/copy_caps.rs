//! Extraction must copy and count a bounded amount of data for each source
//! shape below.
//!
//! Each shape repeats a value that extraction copies, or doubles a value at
//! each declaration. Without a cap, the copied bytes grow with the product of
//! two source counts, or with a power of two. Each test extracts one shape and
//! checks the bytes that the thread allocates against a fixed bound. The bound
//! is far above the work of the capped code and far below the work of the
//! uncapped code. The meters count bytes, not time, so the check is
//! deterministic.

use std::fmt::Write as _;
use std::path::Path;

use fallow_types::discover::FileId;

use crate::cost::{Work, measure};
use crate::parse_source_to_module;

/// Stack of the thread that extracts a shape. The thread registers it, so the
/// parse runs on this thread and the meters see all of its work.
const STACK: usize = 1 << 30;

/// One mebibyte, the unit of the byte bounds below.
const MIB: usize = 1 << 20;

/// Run `run` on a thread with a large registered stack and return its work.
fn work_on_large_stack<R: Send + 'static>(run: impl FnOnce() -> R + Send + 'static) -> Work {
    std::thread::Builder::new()
        .stack_size(STACK)
        .spawn(move || {
            crate::stack::register_thread_stack(STACK);
            // The first extraction on a thread fills caches that later ones reuse.
            parse_source_to_module(
                FileId(0),
                Path::new("warm.ts"),
                "export const a = 1;",
                0,
                true,
            );
            measure(run).1
        })
        .expect("spawn the extraction thread")
        .join()
        .expect("extraction must not panic")
}

/// The work of one extraction of `source` at `path`, with complexity metrics.
fn extraction_work(path: &'static str, source: String) -> Work {
    work_on_large_stack(move || {
        parse_source_to_module(FileId(0), Path::new(path), &source, 0, true);
    })
}

fn assert_bytes_below(name: &str, work: Work, bound: usize) {
    assert!(
        work.allocated_bytes < bound,
        "{name}: extraction allocated {} bytes, bound {bound} ({work:?})",
        work.allocated_bytes
    );
}

/// `const s1 = \`${s0}${s0}\`` doubles the folded value at each declaration.
#[test]
fn folded_template_literals_stay_bounded() {
    let mut source = String::from("const s0 = \"abcdefgh\";\n");
    for index in 1..=20 {
        let previous = index - 1;
        let _ = writeln!(
            source,
            "const s{index} = `${{s{previous}}}${{s{previous}}}`;"
        );
    }
    source.push_str("fetch(s20);\n");
    assert_bytes_below(
        "folded template literals",
        extraction_work("a.ts", source),
        8 * MIB,
    );
}

/// A call with a long static callee and many arguments records the callee
/// path once for each argument.
#[test]
fn long_callee_path_is_not_copied_per_argument() {
    let callee = format!("a{}", ".bbbbbbbbbb".repeat(900));
    let arguments = (0..2_000)
        .map(|index| format!("x{index}"))
        .collect::<Vec<_>>()
        .join(", ");
    let source = format!("{callee}({arguments});\n");
    assert_bytes_below(
        "long callee path",
        extraction_work("a.ts", source),
        16 * MIB,
    );
}

/// One local class with many members, exported under many names.
#[test]
fn class_members_are_not_copied_to_every_export_alias() {
    let mut source = String::from("class Service {\n");
    for index in 0..1_000 {
        let _ = writeln!(source, "  method{index}() {{}}");
    }
    source.push_str("}\nexport {\n");
    for index in 0..1_000 {
        let _ = writeln!(source, "  Service as alias{index},");
    }
    source.push_str("};\n");
    assert_bytes_below(
        "class export aliases",
        extraction_work("a.ts", source),
        16 * MIB,
    );
}

/// A Glimmer mustache with a long `this.` chain.
#[test]
fn glimmer_chain_member_accesses_stay_bounded() {
    let chain = format!("this{}", ".aaaa".repeat(3_000));
    let source = format!("<template>{{{{{chain}}}}}</template>\n");
    let work = work_on_large_stack(move || {
        let ranges = crate::glimmer::find_template_ranges(&source);
        crate::sfc_template::glimmer::collect_glimmer_template_usage(
            &source,
            &ranges,
            &rustc_hash::FxHashSet::default(),
        )
    });
    assert_bytes_below("glimmer chain", work, MIB);
}
