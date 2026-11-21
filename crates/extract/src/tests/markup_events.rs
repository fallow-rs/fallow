//! Svelte dispatch and Vue emit calls in markup, and the spans of markup
//! snippet facts.

use fallow_types::extract::ModuleInfo;

use super::parse_at_path;

fn svelte(source: &str) -> ModuleInfo {
    parse_at_path("Component.svelte", source)
}

fn dispatched_names(info: &ModuleInfo) -> Vec<&str> {
    let mut names: Vec<&str> = info
        .svelte_dispatched_events
        .iter()
        .map(|event| event.name.as_str())
        .collect();
    names.sort_unstable();
    names
}

#[test]
fn svelte_markup_dispatch_is_harvested_at_its_tag() {
    let source = "<script>import { createEventDispatcher } from 'svelte';\n\
                  const dispatch = createEventDispatcher();</script>\n\
                  <button on:click={() => dispatch('save')}>Save</button>";
    let info = &svelte(source);
    assert_eq!(dispatched_names(info), vec!["save"]);
    let tag_start = source.find("<button").expect("fixture has a button") as u32;
    assert_eq!(info.svelte_dispatched_events[0].span_start, tag_start);
    assert!(!info.has_dynamic_dispatch);
}

#[test]
fn svelte_markup_dispatch_shadowed_by_each_local_is_not_harvested() {
    let info = &svelte(
        r"<script>import { createEventDispatcher } from 'svelte';
          const dispatch = createEventDispatcher();
          const handlers = [];</script>
          {#each handlers as dispatch}<button on:click={() => dispatch('save')}>x</button>{/each}",
    );
    assert!(
        info.svelte_dispatched_events.is_empty(),
        "an each local named dispatch is not the dispatcher: {:?}",
        info.svelte_dispatched_events
    );
    assert!(!info.has_dynamic_dispatch);
}

#[test]
fn svelte_dynamic_markup_dispatch_sets_abstain() {
    let info = &svelte(
        r"<script>import { createEventDispatcher } from 'svelte';
          const dispatch = createEventDispatcher();
          let name = 'x';</script>
          <button on:click={() => dispatch(name)}>x</button>",
    );
    assert!(info.has_dynamic_dispatch);
}

fn vue_emits(source: &str) -> ModuleInfo {
    parse_at_path("Comp.vue", source)
}

fn used_emit_names(info: &ModuleInfo) -> Vec<&str> {
    info.component_emits
        .iter()
        .filter(|emit| emit.used)
        .map(|emit| emit.name.as_str())
        .collect()
}

#[test]
fn vue_template_emit_calls_mark_emits_used() {
    let info = vue_emits(
        r#"<script setup>const fire = defineEmits(['close', 'open', 'idle']);</script>
<template><button @click="fire('close')" @dblclick="$emit(`open`)">x</button></template>"#,
    );
    assert_eq!(used_emit_names(&info), vec!["close", "open"]);
    assert!(!info.has_dynamic_emit);
}

#[test]
fn vue_template_emit_with_dynamic_name_sets_abstain() {
    let info = vue_emits(
        r#"<script setup>const emit = defineEmits(['close']); const name = 'close';</script>
<template><button @click="emit(name)">x</button></template>"#,
    );
    assert!(info.has_dynamic_emit);
}

#[test]
fn vue_template_emit_in_a_v_on_object_marks_the_emit_used() {
    let info = vue_emits(
        r#"<script setup>const emit = defineEmits(['close']);</script>
<template><button v-on="{ click: () => emit('close') }">x</button></template>"#,
    );
    assert_eq!(used_emit_names(&info), vec!["close"]);
}

#[test]
fn vue_template_emit_in_an_unparsed_expression_sets_abstain() {
    let info = vue_emits(
        r#"<script setup>const emit = defineEmits(['close']);</script>
<template><button @click="emit('close') +* )">x</button></template>"#,
    );
    assert!(
        info.has_dynamic_emit,
        "an expression that does not parse can hide an emit call"
    );
}

#[test]
fn vue_style_v_bind_call_points_at_the_v_bind() {
    let source = r"<script setup>import { helper } from './helper';</script>
<template><p>x</p></template>
<style>.a { color: v-bind(helper(1)); }</style>";
    let info = vue_emits(source);
    let anchor = source.find("v-bind(").expect("fixture has a v-bind") as u32;
    let use_site = info
        .callee_uses
        .iter()
        .find(|callee| callee.callee_path == "helper")
        .expect("the style v-bind call is a callee use");
    assert_eq!(use_site.span_starts.as_slice(), &[anchor]);
}

#[test]
fn vue_template_member_emit_call_is_not_an_emit() {
    let info = vue_emits(
        r#"<script setup>const emit = defineEmits(['close']);</script>
<template><button @click="bus.emit('close')">x</button></template>"#,
    );
    assert!(used_emit_names(&info).is_empty());
    assert!(!info.has_dynamic_emit);
}
