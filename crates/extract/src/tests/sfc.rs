use std::path::Path;

use fallow_types::discover::FileId;
use fallow_types::extract::ModuleInfo;

use crate::parse::parse_source_to_module;

fn parse_sfc(source: &str, filename: &str) -> ModuleInfo {
    parse_source_to_module(FileId(0), Path::new(filename), source, 0, false)
}

#[test]
fn extracts_vue_script_imports() {
    let info = parse_sfc(
        r#"
<script lang="ts">
import { ref } from 'vue';
import { helper } from './utils';
export default {};
</script>
<template><div></div></template>
"#,
        "App.vue",
    );
    assert_eq!(info.imports.len(), 2);
    assert!(info.imports.iter().any(|i| i.source == "vue"));
    assert!(info.imports.iter().any(|i| i.source == "./utils"));
}

#[test]
fn extracts_vue_script_setup_imports() {
    let info = parse_sfc(
        r#"
<script setup lang="ts">
import { ref } from 'vue';
const count = ref(0);
</script>
"#,
        "Comp.vue",
    );
    assert_eq!(info.imports.len(), 1);
    assert_eq!(info.imports[0].source, "vue");
}

#[test]
fn vue_script_setup_template_usage_clears_unused_import_binding() {
    let info = parse_sfc(
        r#"
<script setup lang="ts">
import { formatDate } from './utils';
</script>
<template><p>{{ formatDate(value) }}</p></template>
"#,
        "Comp.vue",
    );

    assert!(
        !info
            .unused_import_bindings
            .contains(&"formatDate".to_string()),
        "script setup template usage should mark formatDate as used, got: {:?}",
        info.unused_import_bindings
    );
}

#[test]
fn vue_normal_script_import_is_not_visible_to_template() {
    let info = parse_sfc(
        r#"
<script lang="ts">
import { formatDate } from './utils';
export default {};
</script>
<template><p>{{ formatDate(value) }}</p></template>
"#,
        "Comp.vue",
    );

    assert!(
        info.unused_import_bindings
            .contains(&"formatDate".to_string()),
        "normal script imports should not get template credit, got: {:?}",
        info.unused_import_bindings
    );
}

#[test]
fn vue_v_for_alias_shadows_import_name() {
    let info = parse_sfc(
        r#"
<script setup lang="ts">
import { item } from './utils';
</script>
<template><li v-for="item in items">{{ item }}</li></template>
"#,
        "Comp.vue",
    );

    assert!(
        info.unused_import_bindings.contains(&"item".to_string()),
        "v-for alias should shadow imported item, got: {:?}",
        info.unused_import_bindings
    );
}

#[test]
fn vue_template_namespace_access_marks_member_usage() {
    let info = parse_sfc(
        r#"
<script setup lang="ts">
import * as utils from './utils';
</script>
<template><p>{{ utils.formatDate(value) }}</p></template>
"#,
        "Comp.vue",
    );

    assert!(
        info.member_accesses
            .iter()
            .any(|access| access.object == "utils" && access.member == "formatDate"),
        "template namespace access should be recorded, got: {:?}",
        info.member_accesses
    );
}

#[test]
fn vue_component_tag_usage_clears_unused_import_binding() {
    let info = parse_sfc(
        r#"
<script setup lang="ts">
import FancyCard from './FancyCard.vue';
</script>
<template><FancyCard /><fancy-card /></template>
"#,
        "Comp.vue",
    );

    assert!(
        !info
            .unused_import_bindings
            .contains(&"FancyCard".to_string()),
        "component tag usage should mark FancyCard as used, got: {:?}",
        info.unused_import_bindings
    );
}

#[test]
fn vue_custom_directive_usage_clears_unused_import_binding() {
    let info = parse_sfc(
        r#"
<script setup lang="ts">
import { vFocusTrap } from './directives';
</script>
<template><input v-focus-trap /></template>
"#,
        "Comp.vue",
    );

    assert!(
        !info
            .unused_import_bindings
            .contains(&"vFocusTrap".to_string()),
        "custom directive usage should mark vFocusTrap as used, got: {:?}",
        info.unused_import_bindings
    );
}

#[test]
fn vue_custom_directive_value_clears_unused_import_binding() {
    let info = parse_sfc(
        r#"
<script setup lang="ts">
import { tooltipText } from './utils';
</script>
<template><input v-tooltip="tooltipText" /></template>
"#,
        "Comp.vue",
    );

    assert!(
        !info
            .unused_import_bindings
            .contains(&"tooltipText".to_string()),
        "custom directive values should mark tooltipText as used, got: {:?}",
        info.unused_import_bindings
    );
}

#[test]
fn vue_v_on_object_syntax_clears_unused_import_binding() {
    let info = parse_sfc(
        r#"
<script setup lang="ts">
import { handlers } from './utils';
</script>
<template><button v-on="handlers">Add</button></template>
"#,
        "Comp.vue",
    );

    assert!(
        !info
            .unused_import_bindings
            .contains(&"handlers".to_string()),
        "v-on object syntax should mark handlers as used, got: {:?}",
        info.unused_import_bindings
    );
}

#[test]
fn vue_dynamic_directive_arguments_clear_unused_import_bindings() {
    let info = parse_sfc(
        r#"
<script setup lang="ts">
import { activeField, dynamicAttr, dynamicEvent, fieldMap, slotName } from './utils';
</script>
<template>
  <button v-on:[dynamicEvent]="handleClick" />
  <div v-bind:[dynamicAttr]="value" />
  <section v-bind:[fieldMap[activeField]]="value" />
  <List v-slot:[slotName]="{ slotName }">{{ slotName }}</List>
</template>
"#,
        "Comp.vue",
    );

    for binding in [
        "activeField",
        "dynamicAttr",
        "dynamicEvent",
        "fieldMap",
        "slotName",
    ] {
        assert!(
            !info.unused_import_bindings.contains(&binding.to_string()),
            "{binding} should be marked used via a dynamic directive argument, got: {:?}",
            info.unused_import_bindings
        );
    }
}

#[test]
fn vue_slot_default_initializer_clears_unused_import_binding() {
    let info = parse_sfc(
        r#"
<script setup lang="ts">
import { fallbackItem } from './utils';
</script>
<template><List v-slot="{ item = fallbackItem }">{{ item }}</List></template>
"#,
        "Comp.vue",
    );

    assert!(
        !info
            .unused_import_bindings
            .contains(&"fallbackItem".to_string()),
        "slot default initializers should mark fallbackItem as used, got: {:?}",
        info.unused_import_bindings
    );
}

#[test]
fn extracts_vue_both_scripts() {
    let info = parse_sfc(
        r#"
<script lang="ts">
import { defineComponent } from 'vue';
export default defineComponent({});
</script>
<script setup lang="ts">
import { ref } from 'vue';
const count = ref(0);
</script>
"#,
        "Dual.vue",
    );
    assert!(info.imports.len() >= 2);
}

#[test]
fn extracts_svelte_script_imports() {
    let info = parse_sfc(
        r#"
<script lang="ts">
import { onMount } from 'svelte';
import { helper } from './utils';
</script>
<p>Hello</p>
"#,
        "App.svelte",
    );
    assert_eq!(info.imports.len(), 2);
    assert!(info.imports.iter().any(|i| i.source == "svelte"));
    assert!(info.imports.iter().any(|i| i.source == "./utils"));
}

#[test]
fn svelte_template_usage_clears_unused_import_binding() {
    let info = parse_sfc(
        r#"
<script lang="ts">
import { formatDate } from './utils';
</script>
<p>{formatDate(value)}</p>
"#,
        "App.svelte",
    );

    assert!(
        !info
            .unused_import_bindings
            .contains(&"formatDate".to_string()),
        "template usage should mark formatDate as used, got: {:?}",
        info.unused_import_bindings
    );
}

#[test]
fn svelte_unused_import_binding_is_preserved() {
    let info = parse_sfc(
        r#"
<script lang="ts">
import { formatDate } from './utils';
</script>
<p>Hello</p>
"#,
        "App.svelte",
    );

    assert!(
        info.unused_import_bindings
            .contains(&"formatDate".to_string()),
        "unused script import should remain unused, got: {:?}",
        info.unused_import_bindings
    );
}

#[test]
fn svelte_module_context_import_is_not_visible_to_template() {
    let info = parse_sfc(
        r#"
<script context="module" lang="ts">
import { formatDate } from './utils';
</script>
<script lang="ts">
const value = new Date();
</script>
<p>{formatDate(value)}</p>
"#,
        "App.svelte",
    );

    assert!(
        info.unused_import_bindings
            .contains(&"formatDate".to_string()),
        "module-context import should not get template credit, got: {:?}",
        info.unused_import_bindings
    );
}

#[test]
fn svelte_template_namespace_access_marks_member_usage() {
    let info = parse_sfc(
        r#"
<script lang="ts">
import * as utils from './utils';
</script>
<p>{utils.formatDate(value)}</p>
"#,
        "App.svelte",
    );

    assert!(
        info.member_accesses
            .iter()
            .any(|access| access.object == "utils" && access.member == "formatDate"),
        "template namespace access should be recorded, got: {:?}",
        info.member_accesses
    );
}

#[test]
fn svelte_component_tag_usage_clears_unused_import_binding() {
    let info = parse_sfc(
        r#"
<script lang="ts">
import FancyButton from './FancyButton.svelte';
</script>
<FancyButton />
"#,
        "App.svelte",
    );

    assert!(
        !info
            .unused_import_bindings
            .contains(&"FancyButton".to_string()),
        "component tag usage should mark FancyButton as used, got: {:?}",
        info.unused_import_bindings
    );
}

#[test]
fn svelte_directive_usage_clears_unused_import_binding() {
    let info = parse_sfc(
        r#"
<script lang="ts">
import { tooltip } from './actions';
</script>
<button use:tooltip>Hi</button>
"#,
        "App.svelte",
    );

    assert!(
        !info.unused_import_bindings.contains(&"tooltip".to_string()),
        "directive name usage should mark tooltip as used, got: {:?}",
        info.unused_import_bindings
    );
}

#[test]
fn svelte_attribute_value_usage_clears_unused_import_binding() {
    let info = parse_sfc(
        r#"
<script lang="ts">
import { isActive } from './utils';
</script>
<button class:active={isActive}>Hi</button>
"#,
        "App.svelte",
    );

    assert!(
        !info
            .unused_import_bindings
            .contains(&"isActive".to_string()),
        "attribute value expressions should mark isActive as used, got: {:?}",
        info.unused_import_bindings
    );
}

#[test]
fn svelte_store_subscription_clears_unused_import_binding() {
    let info = parse_sfc(
        r#"
<script lang="ts">
import { page } from './stores';
</script>
<p>{$page.url.pathname}</p>
"#,
        "App.svelte",
    );

    assert!(
        !info.unused_import_bindings.contains(&"page".to_string()),
        "store subscription usage should mark page as used, got: {:?}",
        info.unused_import_bindings
    );
}

#[test]
fn vue_no_script_returns_empty() {
    let info = parse_sfc(
        "<template><div></div></template><style>div {}</style>",
        "NoScript.vue",
    );
    assert!(info.imports.is_empty());
    assert!(info.exports.is_empty());
}

#[test]
fn vue_js_default_lang() {
    let info = parse_sfc(
        r"
<script>
import { createApp } from 'vue';
export default {};
</script>
",
        "JsVue.vue",
    );
    assert_eq!(info.imports.len(), 1);
}

#[test]
fn vue_script_lang_tsx() {
    let info = parse_sfc(
        r#"
<script lang="tsx">
import { defineComponent } from 'vue';
export default defineComponent({
    render() { return <div>Hello</div>; }
});
</script>
"#,
        "TsxVue.vue",
    );
    assert_eq!(info.imports.len(), 1);
    assert_eq!(info.imports[0].source, "vue");
}

#[test]
fn svelte_context_module_script() {
    let info = parse_sfc(
        r#"
<script context="module" lang="ts">
export const preload = () => {};
</script>
<script lang="ts">
import { onMount } from 'svelte';
let count = 0;
</script>
"#,
        "Module.svelte",
    );
    assert!(info.imports.iter().any(|i| i.source == "svelte"));
    assert!(!info.exports.is_empty());
}

#[test]
fn vue_script_with_generic_attr() {
    let info = parse_sfc(
        r#"
<script setup lang="ts" generic="T extends Record<string, unknown>">
import { ref } from 'vue';
const items = ref<T[]>([]);
</script>
"#,
        "Generic.vue",
    );
    assert_eq!(info.imports.len(), 1);
    assert_eq!(info.imports[0].source, "vue");
}

#[test]
fn vue_empty_script_block() {
    let info = parse_sfc(
        r#"<script lang="ts"></script><template><div/></template>"#,
        "Empty.vue",
    );
    assert!(info.imports.is_empty());
    assert!(info.exports.is_empty());
}

#[test]
fn vue_whitespace_only_script() {
    let info = parse_sfc(
        "<script lang=\"ts\">\n  \n</script>\n<template><div/></template>",
        "Whitespace.vue",
    );
    assert!(info.imports.is_empty());
}

#[test]
fn vue_script_src_attribute() {
    let info = parse_sfc(
        r#"<script src="./component.ts" lang="ts"></script><template><div/></template>"#,
        "External.vue",
    );
    assert_eq!(info.imports.len(), 1);
    assert_eq!(info.imports[0].source, "./component.ts");
}

#[test]
fn vue_script_src_bare_filename_normalized() {
    // Vue/Svelte <script src="..."> resolves relative to the SFC file, so a
    // bare `logic.ts` must be normalized to `./logic.ts` and not reported as
    // an unlisted npm package. Same bug shape as Angular templateUrl (#99).
    let info = parse_sfc(
        r#"<script src="logic.ts" lang="ts"></script><template><div/></template>"#,
        "App.vue",
    );
    assert_eq!(info.imports.len(), 1);
    assert_eq!(info.imports[0].source, "./logic.ts");
}

#[test]
fn svelte_script_src_bare_filename_normalized() {
    let info = parse_sfc(
        r#"<script src="store.js"></script><div>hi</div>"#,
        "App.svelte",
    );
    assert_eq!(info.imports.len(), 1);
    assert_eq!(info.imports[0].source, "./store.js");
}

#[test]
fn vue_script_inside_html_comment() {
    let info = parse_sfc(
        r#"
<!-- <script lang="ts">
import { bad } from 'should-not-be-found';
</script> -->
<script lang="ts">
import { good } from 'vue';
</script>
<template><div/></template>
"#,
        "Commented.vue",
    );
    assert_eq!(info.imports.len(), 1);
    assert_eq!(info.imports[0].source, "vue");
}

#[test]
fn vue_script_setup_with_compiler_macros() {
    let info = parse_sfc(
        r#"
<script setup lang="ts">
import { ref } from 'vue';
const props = defineProps<{ msg: string }>();
const emit = defineEmits<{ change: [value: string] }>();
const count = ref(0);
</script>
"#,
        "Macros.vue",
    );
    assert_eq!(info.imports.len(), 1);
    assert_eq!(info.imports[0].source, "vue");
}

#[test]
fn vue_script_with_single_quoted_lang() {
    let info = parse_sfc(
        "<script lang='ts'>\nimport { ref } from 'vue';\n</script>",
        "SingleQuote.vue",
    );
    assert_eq!(info.imports.len(), 1);
    assert_eq!(info.imports[0].source, "vue");
}

#[test]
fn svelte_generics_attribute() {
    let info = parse_sfc(
        r#"
<script lang="ts" generics="T extends Record<string, unknown>">
import { onMount } from 'svelte';
export let items: T[] = [];
</script>
"#,
        "Generic.svelte",
    );
    assert_eq!(info.imports.len(), 1);
    assert_eq!(info.imports[0].source, "svelte");
}

#[test]
fn vue_script_with_extra_attributes() {
    let info = parse_sfc(
        r#"
<script lang="ts" id="app-script" type="module" data-custom="value">
import { ref } from 'vue';
</script>
"#,
        "ExtraAttrs.vue",
    );
    assert_eq!(info.imports.len(), 1);
}

#[test]
fn vue_multiple_script_setup_invalid() {
    let info = parse_sfc(
        r#"
<script setup lang="ts">
import { ref } from 'vue';
</script>
<script setup lang="ts">
import { computed } from 'vue';
</script>
"#,
        "DuplicateSetup.vue",
    );
    assert!(info.imports.len() >= 2);
}

#[test]
fn vue_script_case_insensitive() {
    let info = parse_sfc(
        "<SCRIPT lang=\"ts\">\nimport { ref } from 'vue';\n</SCRIPT>",
        "Upper.vue",
    );
    assert_eq!(info.imports.len(), 1);
}

#[test]
fn svelte_script_with_context_and_generics() {
    let info = parse_sfc(
        r#"
<script context="module" lang="ts">
export function preload() { return {}; }
</script>
<script lang="ts" generics="T">
import { onMount } from 'svelte';
export let value: T;
</script>
"#,
        "ContextGenerics.svelte",
    );
    assert!(info.imports.iter().any(|i| i.source == "svelte"));
    assert!(!info.exports.is_empty());
}

#[test]
fn vue_script_with_nested_generics() {
    let info = parse_sfc(
        r#"
<script setup lang="ts" generic="T extends Map<string, Set<number>>">
import { ref } from 'vue';
const items = ref<T>();
</script>
"#,
        "NestedGeneric.vue",
    );
    assert_eq!(info.imports.len(), 1);
    assert_eq!(info.imports[0].source, "vue");
}

#[test]
fn vue_script_src_with_body_ignored() {
    let info = parse_sfc(
        r#"<script src="./external.ts" lang="ts">
import { unused } from 'should-not-matter';
</script>"#,
        "SrcWithBody.vue",
    );
    assert!(info.imports.iter().any(|i| i.source == "./external.ts"));
}

#[test]
fn vue_data_src_not_treated_as_src() {
    let info = parse_sfc(
        r#"<script lang="ts" data-src="./not-a-module.ts">
import { ref } from 'vue';
</script>"#,
        "DataSrc.vue",
    );
    assert_eq!(info.imports.len(), 1);
    assert_eq!(info.imports[0].source, "vue");
}

#[test]
fn vue_html_comment_string_not_corrupted() {
    let info = parse_sfc(
        r#"
<script setup lang="ts">
const htmlComment = "<!-- this is not a comment -->";
import { ref } from 'vue';
</script>
"#,
        "CommentString.vue",
    );
    assert_eq!(info.imports.len(), 1);
    assert_eq!(info.imports[0].source, "vue");
}

#[test]
fn vue_script_spanning_html_comment() {
    let info = parse_sfc(
        r#"
<!-- disabled:
<script lang="ts">
import { bad } from 'should-not-be-found';
</script>
-->
<script lang="ts">
import { good } from 'vue';
</script>
"#,
        "SpanningComment.vue",
    );
    assert_eq!(info.imports.len(), 1);
    assert_eq!(info.imports[0].source, "vue");
}

// --- TypeScript typed template bindings (regression: infinite recursion) ---

#[test]
fn vue_v_for_typed_destructure_full_pipeline() {
    let info = parse_sfc(
        r#"
<script setup lang="ts">
import { items } from './data';
import type { Item } from './types';
</script>
<template><li v-for="({ id, name }: Item) in items">{{ id }} {{ name }}</li></template>
"#,
        "TypedVFor.vue",
    );

    // items is used in the template (iterable), so it should NOT be in unused_import_bindings
    assert!(
        !info.unused_import_bindings.contains(&"items".to_string()),
        "items should be marked as used in v-for iterable, got unused: {:?}",
        info.unused_import_bindings
    );
}

#[test]
fn vue_v_slot_typed_destructure_full_pipeline() {
    let info = parse_sfc(
        r#"
<script setup lang="ts">
import { data } from './store';
import List from './List.vue';
</script>
<template><List v-slot="{ data, loading }: QueryResult">{{ data }}</List></template>
"#,
        "TypedSlot.vue",
    );

    // data is shadowed by the slot binding, so it should be in unused_import_bindings
    assert!(
        info.unused_import_bindings.contains(&"data".to_string()),
        "data should be shadowed by v-slot binding, got unused: {:?}",
        info.unused_import_bindings
    );
    // List component is used
    assert!(
        !info.unused_import_bindings.contains(&"List".to_string()),
        "List component should be used"
    );
}

#[test]
fn svelte_typed_snippet_full_pipeline() {
    let info = parse_sfc(
        r#"
<script lang="ts">
import { cn } from '$lib/utils';
type Props = { href?: string; content?: string };
</script>

{#snippet Link({ href, content }: Props)}
	<a {href}>{content}</a>
{/snippet}

{@render Link({ href: "/", content: "Home" })}
"#,
        "TypedSnippet.svelte",
    );

    // cn is imported but never used in the template
    assert!(
        info.unused_import_bindings.contains(&"cn".to_string()),
        "cn should be unused, got unused: {:?}",
        info.unused_import_bindings
    );
}
