//! Extraction of a long chain must cost work that is linear in its length.
//!
//! A walk to the root at each link of a chain, or a path that is built again
//! at each link, costs O(N^2) for N links. Each shape below is extracted at N
//! and at 4N links, and the work meters of `crate::cost` must grow by about 4,
//! not by 16. The meters count work, not time, so the check does not depend
//! on the machine or the build profile.

use std::fmt::Write as _;
use std::path::Path;

use fallow_types::discover::FileId;

use crate::cost::{Work, measure};
use crate::parse_source_to_module;

/// Links of the short chain. It is far past `MAX_RECORDED_CHAIN_LINKS`, so a
/// capped walk costs the same at each link of both chains.
const LINKS: usize = 1_000;

/// The long chain has this many times the links of the short chain.
const SCALE: usize = 4;

/// Largest growth of the work from the short to the long chain. Linear work
/// grows by `SCALE`, quadratic work by `SCALE * SCALE`. The margin above
/// `SCALE` takes the fixed work of a file and the doubling of buffers.
const MAX_GROWTH: f64 = 5.0;

/// Stack of the thread that extracts the chains. The thread registers it, so
/// the parse runs on this thread and the meters see all of its work.
const STACK: usize = 1 << 30;

/// A chain shape: its name, the file path, and the source for `n` links.
struct Shape {
    name: &'static str,
    path: &'static str,
    source: fn(usize) -> String,
}

/// `a.b.b...b` with `n` links.
fn member(root: &str, n: usize) -> String {
    format!("{root}{}", ".b".repeat(n))
}

/// `a0 + a1 + ... + an` with distinct operands.
fn distinct_sum(n: usize) -> String {
    (0..=n)
        .map(|index| format!("a{index}"))
        .collect::<Vec<_>>()
        .join(" + ")
}

/// `a.m0().m1()...` with distinct method names.
fn distinct_fluent(n: usize) -> String {
    let mut chain = String::from("a");
    for index in 0..n {
        let _ = write!(chain, ".m{index}()");
    }
    chain
}

const VUE: &str = "import { ref, computed, reactive, watch, defineEmits } from 'vue';\n";
const REACT: &str = "import { useState, useMemo, useCallback } from 'react';\n";

macro_rules! shape {
    ($name:literal, $path:literal, $source:expr) => {
        Shape {
            name: $name,
            path: $path,
            source: $source,
        }
    };
}

#[expect(clippy::too_many_lines, reason = "one entry for each chain shape")]
fn shapes() -> Vec<Shape> {
    vec![
        shape!("export const", "a.ts", |n| format!(
            "export const x = {};\n",
            member("a", n)
        )),
        shape!("export default", "a.ts", |n| format!(
            "export default {};\n",
            member("a", n)
        )),
        shape!("call", "a.ts", |n| format!("{}();\n", member("a", n))),
        shape!("argument", "a.ts", |n| format!("fn({});\n", member("a", n))),
        shape!("module.exports", "a.js", |n| format!(
            "module.exports = {};\n",
            member("a", n)
        )),
        shape!("exports property", "a.js", |n| format!(
            "exports.x = {};\n",
            member("a", n)
        )),
        shape!("require argument", "a.js", |n| format!(
            "const x = require({});\n",
            member("a", n)
        )),
        shape!("dynamic import", "a.ts", |n| format!(
            "const x = import({});\n",
            member("a", n)
        )),
        shape!("jsx tag", "a.tsx", |n| format!(
            "export const x = <{} />;\n",
            member("a", n)
        )),
        shape!("jsx attribute", "a.tsx", |n| format!(
            "export const x = <div title={{{}}} />;\n",
            member("a", n)
        )),
        shape!("new", "a.ts", |n| format!("new {}();\n", member("a", n))),
        shape!("assignment target", "a.ts", |n| format!(
            "{} = 1;\n",
            member("a", n)
        )),
        shape!("compound assignment", "a.ts", |n| format!(
            "{} += 1;\n",
            member("a", n)
        )),
        shape!("update", "a.ts", |n| format!("{}++;\n", member("a", n))),
        shape!("delete", "a.ts", |n| format!(
            "delete {};\n",
            member("a", n)
        )),
        shape!("innerHTML sink", "a.ts", |n| format!(
            "{}.innerHTML = x;\n",
            member("a", n)
        )),
        shape!("tainted sink value", "a.ts", |n| format!(
            "export function f(req) {{ document.body.innerHTML = {}; }}\n",
            member("req.query", n)
        )),
        shape!("tainted binding", "a.ts", |n| format!(
            "export function f(req) {{ const x = {}; eval(x); }}\n",
            member("req.query", n)
        )),
        shape!("process.env", "a.ts", |n| format!(
            "export const x = {};\n",
            member("process.env", n)
        )),
        shape!("import.meta", "a.ts", |n| format!(
            "export const x = {};\n",
            member("import.meta.env", n)
        )),
        shape!("this", "a.ts", |n| format!(
            "export class C {{ m() {{ return {}; }} }}\n",
            member("this", n)
        )),
        shape!("destructuring", "a.ts", |n| format!(
            "const {{ c }} = {};\nexport {{ c }};\n",
            member("a", n)
        )),
        shape!("optional chain", "a.ts", |n| format!(
            "export const x = a{};\n",
            "?.b".repeat(n)
        )),
        shape!("computed chain", "a.ts", |n| format!(
            "export const x = a{};\n",
            "['b']".repeat(n)
        )),
        shape!("tagged template", "a.ts", |n| format!(
            "export const x = {}`t`;\n",
            member("a", n)
        )),
        shape!("template literal", "a.ts", |n| format!(
            "export const x = `${{{}}}`;\n",
            member("a", n)
        )),
        shape!("spread", "a.ts", |n| format!(
            "export const x = {{ ...{} }};\n",
            member("a", n)
        )),
        shape!("ts as", "a.ts", |n| format!(
            "export const x = {} as T;\n",
            member("a", n)
        )),
        shape!("ts non-null", "a.ts", |n| format!(
            "export const x = {}!;\n",
            member("a", n)
        )),
        shape!("typeof type", "a.ts", |n| format!(
            "export type T = typeof {};\n",
            member("a", n)
        )),
        shape!("qualified type", "a.ts", |n| format!(
            "export type T = {};\n",
            member("A", n)
        )),
        shape!("plus concat", "a.ts", |n| format!(
            "export const x = {};\n",
            distinct_sum(n)
        )),
        shape!("tainted plus concat", "a.ts", |n| format!(
            "export function f(req) {{ const x = {}; eval(x); }}\n",
            (0..=n)
                .map(|index| format!("req.query.q{index}"))
                .collect::<Vec<_>>()
                .join(" + ")
        )),
        shape!("await chain", "a.ts", |n| format!(
            "export const x = {}a;\n",
            "await ".repeat(n)
        )),
        shape!("await member chain", "a.ts", |n| format!(
            "export async function f(req) {{ const x = {}req.query.id; eval(x); }}\n",
            "await ".repeat(n)
        )),
        shape!("fluent chain", "a.ts", |n| format!(
            "import {{ a }} from './a';\nexport const x = a{};\n",
            ".b()".repeat(n)
        )),
        shape!("fluent chain of distinct methods", "a.ts", |n| format!(
            "import {{ a }} from './a';\nexport const x = {};\n",
            distinct_fluent(n)
        )),
        shape!("new fluent chain", "a.ts", |n| format!(
            "import {{ A }} from './a';\nexport const x = new A(){};\n",
            ".b()".repeat(n)
        )),
        shape!("vue ref", "a.ts", |n| format!(
            "{VUE}export const x = ref({});\n",
            member("a", n)
        )),
        shape!("vue computed", "a.ts", |n| format!(
            "{VUE}export const x = computed(() => {});\n",
            member("a", n)
        )),
        shape!("vue emit", "a.ts", |n| format!(
            "{VUE}const emit = defineEmits();\nemit({});\n",
            member("a", n)
        )),
        shape!("react useState", "a.tsx", |n| format!(
            "{REACT}export function C() {{ const [s] = useState({}); return s; }}\n",
            member("a", n)
        )),
        shape!("extends", "a.ts", |n| format!(
            "export class C extends {} {{}}\n",
            member("a", n)
        )),
        shape!("implements", "a.ts", |n| format!(
            "export class C implements {} {{}}\n",
            member("A", n)
        )),
        shape!("interface extends", "a.ts", |n| format!(
            "export interface I extends {} {{}}\n",
            member("A", n)
        )),
        shape!("mdx member tag", "a.mdx", |n| format!(
            "import * as a from './a'\n\n<{0}>x</{0}>\n",
            member("a", n)
        )),
        shape!("astro member tag", "a.astro", |n| format!(
            "---\nimport * as a from './a';\n---\n<{} />\n",
            member("a", n)
        )),
        shape!("test callee", "a.test.ts", |n| format!(
            "{}('t', () => {{}});\n",
            member("test", n)
        )),
    ]
}

/// Extract `source` at `path` and return the work it did on this thread.
fn extraction_work(path: &str, source: &str) -> Work {
    measure(|| parse_source_to_module(FileId(0), Path::new(path), source, 0, false)).1
}

/// The growth of each meter from `LINKS` to `SCALE * LINKS` links.
fn growth(shape: &Shape) -> (f64, f64, Work, Work) {
    // The first extraction on a thread fills caches that later ones reuse.
    extraction_work(shape.path, &(shape.source)(1));
    let short = extraction_work(shape.path, &(shape.source)(LINKS));
    let long = extraction_work(shape.path, &(shape.source)(SCALE * LINKS));
    let ratio = |long: usize, short: usize| long as f64 / short.max(1) as f64;
    (
        ratio(long.walk_steps, short.walk_steps),
        ratio(long.allocated_bytes, short.allocated_bytes),
        short,
        long,
    )
}

#[test]
fn long_chains_cost_linear_extraction_work() {
    let failures = std::thread::Builder::new()
        .stack_size(STACK)
        .spawn(|| {
            crate::stack::register_thread_stack(STACK);
            let mut failures = Vec::new();
            for shape in shapes() {
                let (steps, bytes, short, long) = growth(&shape);
                if steps > MAX_GROWTH || bytes > MAX_GROWTH {
                    failures.push(format!(
                        "{}: walk steps grew {steps:.1}x, allocated bytes grew {bytes:.1}x \
                         ({short:?} -> {long:?})",
                        shape.name
                    ));
                }
            }
            failures
        })
        .expect("spawn the extraction thread")
        .join()
        .expect("extraction must not panic");
    assert!(
        failures.is_empty(),
        "extraction work grew faster than the chain:\n{}",
        failures.join("\n")
    );
}
