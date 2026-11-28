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

/// A long member chain on an imported design token binding.
#[test]
fn css_in_js_token_chain_stays_bounded() {
    let source = format!(
        "import {{ vars }} from './tokens';\nexport const x = vars{};\n",
        ".aaaa".repeat(1_000)
    );
    let work = work_on_large_stack(move || {
        let leaf_paths: rustc_hash::FxHashSet<String> =
            std::iter::once("color.primary".to_string()).collect();
        let queries = [crate::css_in_js::ConsumerQuery::MemberBinding {
            alias: "vars",
            leaf_paths: &leaf_paths,
        }];
        crate::css_in_js::css_in_js_consumer_scan(&source, Path::new("a.ts"), &queries)
    });
    assert_bytes_below("css-in-js token chain", work, 12 * MIB);
}

/// A string-keyed call with a long callee and many callback arguments. The
/// complexity context name of each callback holds the callee text.
#[test]
fn complexity_context_name_of_a_long_callee_stays_bounded() {
    let callee = format!("a{}", ".bbbbbbbbbb".repeat(900));
    let callbacks = (0..1_000)
        .map(|index| format!("() => {index}"))
        .collect::<Vec<_>>()
        .join(", ");
    let source = format!("{callee}(\"route\", {callbacks});\n");
    assert_bytes_below(
        "complexity context name",
        extraction_work("a.ts", source),
        16 * MIB,
    );
}

/// Each fixture type alias names the previous alias twice, so the fixture
/// bindings double at each declaration.
#[test]
fn fixture_type_alias_bindings_stay_bounded() {
    let mut source = String::from("type F0 = { a: Page; b: Page };\n");
    for index in 1..=16 {
        let previous = index - 1;
        let _ = writeln!(
            source,
            "type F{index} = {{ a: F{previous}; b: F{previous} }};"
        );
    }
    assert_bytes_below(
        "fixture type aliases",
        extraction_work("a.ts", source),
        16 * MIB,
    );
}

/// A Vue slot with many props and many markup snippets inside it. Each
/// snippet names one import and no slot prop.
#[test]
fn markup_snippets_declare_only_the_locals_they_name() {
    let props = (0..500)
        .map(|index| format!("prop{index}"))
        .collect::<Vec<_>>()
        .join(", ");
    let mut source = format!(
        "<script setup>\nimport {{ helper }} from './helper';\nimport Comp from './Comp.vue';\n\
         </script>\n<template>\n<Comp v-slot=\"{{ {props} }}\">\n"
    );
    for index in 0..500 {
        let _ = writeln!(source, "<p>{{{{ helper({index}) }}}}</p>");
    }
    source.push_str("</Comp>\n</template>\n");
    assert_bytes_below(
        "markup snippet locals",
        extraction_work("a.vue", source),
        48 * MIB,
    );
}

/// Template locals of the outer scope in the scope copy tests below.
const SCOPE_LOCALS: usize = 1_000;

/// Elements inside the outer scope in the scope copy tests below.
const SCOPE_ELEMENTS: usize = 2_000;

/// `SCOPE_ELEMENTS` empty paragraphs.
fn plain_elements() -> String {
    "<p></p>\n".repeat(SCOPE_ELEMENTS)
}

/// `local0, local1, ...` with `SCOPE_LOCALS` names.
fn local_names() -> String {
    (0..SCOPE_LOCALS)
        .map(|index| format!("local{index}"))
        .collect::<Vec<_>>()
        .join(", ")
}

/// A Vue slot with many props and many plain elements inside it. The scan
/// reads the template locals at each element.
#[test]
fn vue_scan_does_not_copy_scope_locals_per_element() {
    let source = format!(
        "<script setup>\nimport Comp from './Comp.vue';\n</script>\n<template>\n\
         <Comp v-slot=\"{{ {} }}\">\n{}</Comp>\n</template>\n",
        local_names(),
        plain_elements()
    );
    assert_bytes_below(
        "vue scope locals",
        extraction_work("a.vue", source),
        16 * MIB,
    );
}

/// A Svelte each block with many item names and many plain elements inside it.
#[test]
fn svelte_scan_does_not_copy_scope_locals_per_element() {
    let source = format!(
        "<script>\nimport Item from './Item.svelte';\nlet items = [];\n</script>\n\
         {{#each items as {{ {} }}}}\n{}{{/each}}\n",
        local_names(),
        plain_elements()
    );
    assert_bytes_below(
        "svelte scope locals",
        extraction_work("a.svelte", source),
        16 * MIB,
    );
}

/// An Angular template with many `@let` locals and many plain elements.
#[test]
fn angular_scan_does_not_copy_scope_locals_per_element() {
    let mut template = String::new();
    for index in 0..SCOPE_LOCALS {
        let _ = writeln!(template, "@let local{index} = {index};");
    }
    template.push_str(&plain_elements());
    let work = work_on_large_stack(move || {
        crate::sfc_template::angular::collect_angular_template_refs(&template)
    });
    // Most of the bound is the parse of each `@let` expression, which is linear.
    assert_bytes_below("angular scope locals", work, 32 * MIB);
}
