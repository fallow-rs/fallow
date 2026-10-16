//! Script blocks of Vue, Svelte and Astro component files.

#![allow(
    clippy::expect_used,
    reason = "the fixture offsets come from fixed source texts"
)]

mod common;

use common::assert_valid;
use fallow_mutation::{
    Instrumented, MutationOptions, Operator, ScriptBlock, ScriptRole, SourceKind, instrument_blocks,
};

/// The span of the body of the first `<open ...>...</close>` block.
fn body(source: &str, open: &str, close: &str) -> (u32, u32) {
    let tag = source.find(open).expect("open tag");
    let start = tag + source[tag..].find('>').expect("tag end") + 1;
    let end = start + source[start..].find(close).expect("close tag");
    (start as u32, end as u32)
}

const fn block(span: (u32, u32), kind: SourceKind, role: ScriptRole) -> ScriptBlock {
    ScriptBlock { span, kind, role }
}

/// Instruments the blocks and checks the output: each mutant matches the
/// original text at its span and has the file line and column, and each
/// splice segment copies the original text.
fn run(source: &str, blocks: &[ScriptBlock], options: &MutationOptions) -> Instrumented {
    let result = match instrument_blocks(source, "src/Widget.vue", blocks, options) {
        Ok(result) => result,
        Err(error) => panic!("instrument failed: {error}"),
    };
    for mutant in &result.mutants {
        let start = mutant.span.0 as usize;
        assert_eq!(&source[start..mutant.span.1 as usize], mutant.original);
        let line_start = source[..start].rfind('\n').map_or(0, |index| index + 1);
        let line = source[..start].matches('\n').count() + 1;
        assert_eq!(mutant.line as usize, line);
        assert_eq!(mutant.column as usize, start - line_start);
    }
    for segment in &result.map.segments {
        let generated = segment.generated_start as usize;
        let original = segment.original_start as usize;
        let len = segment.len as usize;
        assert_eq!(
            &result.code[generated..generated + len],
            &source[original..original + len]
        );
    }
    result
}

/// The instrumented text of each block, cut out of the output at the tags.
fn instrumented_bodies(code: &str, open: &str, close: &str) -> Vec<String> {
    let mut bodies = Vec::new();
    let mut rest = code;
    while let Some(tag) = rest.find(open) {
        let start = tag + rest[tag..].find('>').expect("tag end") + 1;
        let end = start + rest[start..].find(close).expect("close tag");
        bodies.push(rest[start..end].to_owned());
        rest = &rest[end..];
    }
    bodies
}

const VUE: &str = r#"<template>
  <button @click="inc">{{ count > 1 ? 'many' : 'one' }}</button>
</template>

<script setup lang="ts">
import { ref } from 'vue'
const props = withDefaults(defineProps<{ step?: number }>(), { step: 1 + 1 })
const emit = defineEmits(['change'])
const count = ref(0)
function inc() {
  count.value = count.value + props.step
  emit('change', count.value > 10)
}
</script>

<style scoped>
button { color: red; }
</style>
"#;

#[test]
fn vue_setup_block_is_instrumented_in_place() {
    let span = body(VUE, "<script setup", "</script>");
    let blocks = [block(span, SourceKind::Ts, ScriptRole::VueSetup)];
    let result = run(VUE, &blocks, &MutationOptions::default());

    assert!(!result.mutants.is_empty());
    for mutant in &result.mutants {
        assert!(span.0 <= mutant.span.0 && mutant.span.1 <= span.1);
    }
    let script_start = VUE.find("<script setup").expect("script");
    let script_end = VUE.find("</script>").expect("script end");
    assert!(result.code.starts_with(&VUE[..script_start]));
    assert!(result.code.ends_with(&VUE[script_end..]));
    let bodies = instrumented_bodies(&result.code, "<script setup", "</script>");
    assert_eq!(bodies.len(), 1);
    assert!(bodies[0].contains("function __fallow_m_a(id)"));
    assert_valid(&bodies[0], SourceKind::Ts);
}

#[test]
fn vue_setup_block_skips_compiler_macro_arguments() {
    let span = body(VUE, "<script setup", "</script>");
    let blocks = [block(span, SourceKind::Ts, ScriptRole::VueSetup)];
    let result = run(VUE, &blocks, &MutationOptions::default());
    let macros_end = VUE.find("const count").expect("count") as u32;
    let in_macros: Vec<&str> = result
        .mutants
        .iter()
        .filter(|mutant| mutant.span.0 < macros_end)
        .map(|mutant| mutant.original.as_str())
        .collect();
    assert_eq!(in_macros, Vec::<&str>::new());
    assert!(
        result
            .mutants
            .iter()
            .any(|mutant| mutant.original == "+" && mutant.span.0 > macros_end)
    );
}

/// The Vue compiler finds `defineExpose()` only as a plain call statement.
/// A `removed-call` switch around it hides the macro, and the component
/// throws `defineExpose is not defined` at run time.
#[test]
fn vue_setup_block_never_wraps_define_expose() {
    let source = "<script setup lang=\"ts\">\nfunction start() { return 1 + 1 }\ndefineExpose<{ start: () => number }>({ start })\n</script>\n";
    let span = body(source, "<script setup", "</script>");
    let blocks = [block(span, SourceKind::Ts, ScriptRole::VueSetup)];
    let result = run(source, &blocks, &MutationOptions::default());
    let macro_start = source.find("defineExpose").expect("macro") as u32;
    let wrapping: Vec<&str> = result
        .mutants
        .iter()
        .filter(|mutant| mutant.span.1 > macro_start)
        .map(|mutant| mutant.original.as_str())
        .collect();
    assert_eq!(wrapping, Vec::<&str>::new());
    assert!(result.mutants.iter().any(|mutant| mutant.original == "+"));
    let bodies = instrumented_bodies(&result.code, "<script setup", "</script>");
    assert!(
        bodies[0].contains("\ndefineExpose<{ start: () => number }>({ start })\n"),
        "{}",
        bodies[0]
    );
}

#[test]
fn vue_setup_block_skips_props_destructure_defaults() {
    let source = "<script setup lang=\"ts\">\nconst { size = 'md' } = defineProps<{ size?: string }>()\nconst label = size + '!'\n</script>\n";
    let span = body(source, "<script setup", "</script>");
    let blocks = [block(span, SourceKind::Ts, ScriptRole::VueSetup)];
    let result = run(source, &blocks, &MutationOptions::default());
    let originals: Vec<&str> = result
        .mutants
        .iter()
        .map(|mutant| mutant.original.as_str())
        .collect();
    assert_eq!(originals, vec!["'!'"]);
}

#[test]
fn vue_plain_script_holds_the_only_prelude() {
    let source = "<script lang=\"ts\">\nexport const limit = (n: number) => n > 2\n</script>\n<script setup lang=\"ts\">\nconst a = limit(1) && true\n</script>\n";
    let first = body(source, "<script lang", "</script>");
    let second = body(source, "<script setup", "</script>");
    let blocks = [
        block(second, SourceKind::Ts, ScriptRole::VueSetup),
        block(first, SourceKind::Ts, ScriptRole::VueScript),
    ];
    let result = run(source, &blocks, &MutationOptions::default());
    assert_eq!(result.code.matches("function __fallow_m_a(id)").count(), 1);
    let bodies = instrumented_bodies(&result.code, "<script", "</script>");
    assert_eq!(bodies.len(), 2);
    assert!(bodies[0].contains("function __fallow_m_a(id)"));
    assert!(bodies[1].contains("__fallow_m_a(\""));
    assert!(bodies[1].starts_with("// @ts-nocheck\n"));
}

const SVELTE: &str = r#"<script module>
  export const LIMIT = 3 + 1;
</script>

<script lang="ts">
  let { start = 'a' } = $props();
  let count = $state(1 + 1);
  let double = $derived(count * 2);
  $effect(() => {
    if (count > LIMIT) count = 0;
  });
  function inc() {
    count += 1;
  }
</script>

<button on:click={inc}>{double}</button>
"#;

#[test]
fn svelte_runes_stay_in_place_and_their_arguments_mutate() {
    let module = body(SVELTE, "<script module", "</script>");
    let instance = body(SVELTE, "<script lang", "</script>");
    let blocks = [
        block(module, SourceKind::Js, ScriptRole::SvelteModule),
        block(instance, SourceKind::Ts, ScriptRole::SvelteInstance),
    ];
    let result = run(SVELTE, &blocks, &MutationOptions::default());
    let originals: Vec<&str> = result
        .mutants
        .iter()
        .map(|mutant| mutant.original.as_str())
        .collect();
    for rune in ["$state(", "$derived(", "$props(", "$effect("] {
        assert!(
            !originals.iter().any(|original| original.contains(rune)),
            "{rune}"
        );
    }
    assert!(
        !result
            .mutants
            .iter()
            .any(|mutant| mutant.operator == Operator::RemovedCall),
        "the $effect statement must stay"
    );
    for operator_text in ["*", ">", "+=", "'a'"] {
        assert!(originals.contains(&operator_text), "{operator_text}");
    }
    assert!(result.code.contains("$state((__fallow_m_a("));
    assert!(result.code.contains("$effect(() => {"));
    assert_eq!(result.code.matches("function __fallow_m_a(id)").count(), 1);
}

#[test]
fn astro_frontmatter_skips_exported_declarations() {
    let source = "---\nexport const prerender = true;\nexport function getStaticPaths() { return [{ params: { id: 'a' } }]; }\nconst title = Astro.props.title ?? 'none';\nconst big = title.length > 10;\n---\n<h1>{title}</h1>\n";
    let end = source.rfind("---").expect("fence") as u32;
    let blocks = [block(
        (4, end),
        SourceKind::Ts,
        ScriptRole::AstroFrontmatter,
    )];
    let result = run(source, &blocks, &MutationOptions::default());
    let exports_end = source.find("const title").expect("title") as u32;
    assert!(!result.mutants.is_empty());
    assert!(
        result
            .mutants
            .iter()
            .all(|mutant| mutant.span.0 >= exports_end)
    );
    assert!(result.code.contains("export const prerender = true;"));
}

#[test]
fn ranges_and_ids_use_file_coordinates() {
    let span = body(VUE, "<script setup", "</script>");
    let blocks = [block(span, SourceKind::Ts, ScriptRole::VueSetup)];
    let line = VUE.find("  emit('change'").expect("emit line") as u32;
    let options = MutationOptions {
        ranges: Some(vec![(line, line + 10)]),
        ..MutationOptions::default()
    };
    let narrowed = run(VUE, &blocks, &options);
    assert!(!narrowed.mutants.is_empty());
    assert!(narrowed.mutants.iter().all(|mutant| mutant.line == 12));

    // A longer template moves the block. The ids stay the same.
    let moved = VUE.replace("<template>\n", "<template>\n  <p>extra</p>\n");
    let moved_span = body(&moved, "<script setup", "</script>");
    let moved_blocks = [block(moved_span, SourceKind::Ts, ScriptRole::VueSetup)];
    let before = run(VUE, &blocks, &MutationOptions::default());
    let after = run(&moved, &moved_blocks, &MutationOptions::default());
    let ids = |result: &Instrumented| -> Vec<String> {
        result
            .mutants
            .iter()
            .map(|mutant| mutant.id.as_str().to_owned())
            .collect()
    };
    assert_eq!(ids(&before), ids(&after));
}

#[test]
fn a_parse_error_in_one_block_fails_the_file() {
    let source = "<script>\nconst a = ;\n</script>\n";
    let span = body(source, "<script", "</script>");
    let blocks = [block(span, SourceKind::Js, ScriptRole::VueScript)];
    let error = instrument_blocks(source, "src/A.vue", &blocks, &MutationOptions::default())
        .expect_err("a parse error");
    assert!(error.to_string().contains("does not parse"));
}
