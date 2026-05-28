use std::path::Path;

use fallow_types::discover::FileId;
use fallow_types::extract::ModuleInfo;

use crate::parse::parse_source_to_module;

fn parse_sfc(source: &str, filename: &str) -> ModuleInfo {
    parse_source_to_module(FileId(0), Path::new(filename), source, 0, false)
}

fn parse_sfc_with_complexity(source: &str, filename: &str) -> ModuleInfo {
    parse_source_to_module(FileId(0), Path::new(filename), source, 0, true)
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
fn vue_script_import_spans_are_original_source_offsets() {
    let source = r#"<template><div /></template>

<script lang="ts">
import { helper } from './utils';
export const value = helper();
</script>
"#;
    let info = parse_sfc(source, "App.vue");
    let import = info
        .imports
        .iter()
        .find(|i| i.source == "./utils")
        .expect("script import extracted");
    let export = info
        .exports
        .iter()
        .find(|e| matches!(&e.name, crate::ExportName::Named(name) if name == "value"))
        .expect("script export extracted");
    let (import_line, _) =
        fallow_types::extract::byte_offset_to_line_col(&info.line_offsets, import.span.start);
    let (export_line, _) =
        fallow_types::extract::byte_offset_to_line_col(&info.line_offsets, export.span.start);
    assert_eq!(import_line, 4);
    assert_eq!(export_line, 5);
    assert_eq!(
        &source[import.source_span.start as usize..import.source_span.end as usize],
        "'./utils'"
    );
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
fn svelte_attach_tag_clears_unused_import_binding() {
    let info = parse_sfc(
        r#"
<script lang="ts">
import { myAttach } from './attachments';
</script>
<div {@attach myAttach}></div>
"#,
        "App.svelte",
    );

    assert!(
        !info
            .unused_import_bindings
            .contains(&"myAttach".to_string()),
        "attach tag usage should mark myAttach as used, got: {:?}",
        info.unused_import_bindings
    );
}

#[test]
fn svelte_event_handler_arrow_member_call_marks_bound_class_member_usage() {
    let info = parse_sfc(
        r#"
<script lang="ts">
import { Counter } from './counter';
const counter = new Counter();
</script>
<button onclick={() => counter.bump()}>Increment</button>
"#,
        "App.svelte",
    );

    assert!(
        info.member_accesses
            .iter()
            .any(|access| access.object == "Counter" && access.member == "bump"),
        "event handler method call should be resolved to Counter.bump, got: {:?}",
        info.member_accesses
    );
}

#[test]
fn svelte_derived_rune_member_call_marks_bound_class_member_usage() {
    let info = parse_sfc(
        r#"
<script lang="ts">
import { Counter } from './counter';
const counter = $derived(new Counter());
</script>
<button onclick={() => counter.bump()}>Increment</button>
"#,
        "App.svelte",
    );

    assert!(
        info.member_accesses
            .iter()
            .any(|access| access.object == "Counter" && access.member == "bump"),
        "$derived(new Counter()) should still resolve template member usage, got: {:?}",
        info.member_accesses
    );
}

#[test]
fn svelte_effect_and_inspect_runes_credit_import_usage() {
    let info = parse_sfc(
        r#"
<script lang="ts">
import { track, debug } from './utils';
$effect(() => track());
$inspect(debug());
</script>
"#,
        "App.svelte",
    );

    assert!(
        !info.unused_import_bindings.contains(&"track".to_string()),
        "$effect callback should credit track, got: {:?}",
        info.unused_import_bindings
    );
    assert!(
        !info.unused_import_bindings.contains(&"debug".to_string()),
        "$inspect argument should credit debug, got: {:?}",
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

// Imports referenced ONLY inside `generic="T extends Test<boolean>"` (not
// inside the script body itself) must still be classified as
// type-referenced, otherwise the upstream `export type Test` is falsely
// reported as `unused_types`. The augmented-source probe in
// `merge_script_into_module` is what makes this work.
#[test]
fn vue_generic_attr_marks_type_only_import_as_type_referenced() {
    let info = parse_sfc(
        r#"
<script setup lang="ts" generic="T extends Test<boolean>">
import type { Test } from './types';
defineProps<{ item: T }>();
</script>
"#,
        "Parent.vue",
    );
    assert!(
        info.type_referenced_import_bindings
            .contains(&"Test".to_string()),
        "Test referenced only via generic=\"...\" must be type-referenced, got: {:?}",
        info.type_referenced_import_bindings,
    );
    assert!(
        !info.unused_import_bindings.contains(&"Test".to_string()),
        "Test referenced only via generic=\"...\" must not be unused, got: {:?}",
        info.unused_import_bindings,
    );
}

// Multi-parameter generic constraints must credit every distinct identifier
// in the constraint, not just the first one.
#[test]
fn vue_generic_attr_marks_each_constraint_identifier() {
    let info = parse_sfc(
        r#"
<script setup lang="ts" generic="K extends keyof Foo, V extends Bar">
import type { Foo, Bar } from './shapes';
defineProps<{ key: K; value: V }>();
</script>
"#,
        "MultiGeneric.vue",
    );
    for name in ["Foo", "Bar"] {
        assert!(
            info.type_referenced_import_bindings
                .contains(&name.to_string()),
            "{name} from a multi-param generic= must be type-referenced, got: {:?}",
            info.type_referenced_import_bindings,
        );
    }
}

// Svelte's `generics="..."` attribute is the equivalent knob and must walk
// the same augmented-source path.
#[test]
fn svelte_generics_attr_marks_type_only_import_as_type_referenced() {
    let info = parse_sfc(
        r#"
<script lang="ts" generics="T extends Item">
import type { Item } from './types';
export let items: T[] = [];
</script>
"#,
        "List.svelte",
    );
    assert!(
        info.type_referenced_import_bindings
            .contains(&"Item".to_string()),
        "Item referenced only via generics=\"...\" must be type-referenced, got: {:?}",
        info.type_referenced_import_bindings,
    );
    assert!(
        !info.unused_import_bindings.contains(&"Item".to_string()),
        "Item referenced only via generics=\"...\" must not be unused, got: {:?}",
        info.unused_import_bindings,
    );
}

// Single-quoted attribute values must be recognised the same as double.
#[test]
fn vue_generic_attr_handles_single_quotes() {
    let info = parse_sfc(
        "<script setup lang=\"ts\" generic='T extends Test'>\nimport type { Test } from './types';\ndefineProps<{ item: T }>();\n</script>",
        "SingleQuotedGeneric.vue",
    );
    assert!(
        info.type_referenced_import_bindings
            .contains(&"Test".to_string()),
        "single-quoted generic= must still be scanned, got: {:?}",
        info.type_referenced_import_bindings,
    );
}

// Empty / whitespace-only generic attributes must not produce a parse error
// or otherwise mark unrelated imports as type-referenced.
#[test]
fn vue_generic_attr_empty_value_is_inert() {
    let info = parse_sfc(
        r#"
<script setup lang="ts" generic="">
import { ref } from 'vue';
const x = ref(0);
</script>
"#,
        "EmptyGeneric.vue",
    );
    // ref is value-referenced (used as `ref(0)`) and must not be type-referenced
    assert!(
        !info
            .type_referenced_import_bindings
            .contains(&"ref".to_string()),
        "empty generic= attribute must not introduce spurious type references",
    );
    assert!(
        info.value_referenced_import_bindings
            .contains(&"ref".to_string()),
        "ref must remain value-referenced, got: {:?}",
        info.value_referenced_import_bindings,
    );
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

// Regression for the knip #1670 reproduction shape: a type-only import in a
// `<script lang="ts">` block whose binding is only consumed as a type
// annotation. Knip's "real svelte compiler" mode strips types before
// analysis, so the import vanishes from its view; fallow extracts the raw
// script body and parses it with oxc, so type-position bindings stay
// classified as type-referenced and the upstream `export type` is not
// flagged unused.
#[test]
fn svelte_script_keeps_type_only_imports_used_as_annotations() {
    let info = parse_sfc(
        r#"
<script lang="ts">
import type { TestType } from '../lib/types';

const test: TestType = { a: true };

export { test };
</script>
"#,
        "+page.svelte",
    );
    let import = info
        .imports
        .iter()
        .find(|i| i.source == "../lib/types")
        .expect("type-only import survives the SFC boundary");
    assert!(
        import.is_type_only,
        "import marked `import type` must keep is_type_only=true",
    );
    assert!(
        info.type_referenced_import_bindings
            .contains(&"TestType".to_string()),
        "TestType used as a type annotation must be tracked as type-referenced, got: {:?}",
        info.type_referenced_import_bindings,
    );
    assert!(
        !info
            .unused_import_bindings
            .contains(&"TestType".to_string()),
        "TestType referenced as a type annotation must not appear in unused_import_bindings",
    );
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

// Regression for the knip #1714 reproduction shape: a `generic` attribute
// whose constraint references a single user type with a primitive type
// argument. Knip's parser-driven path drops the entire script body in that
// case, fallow's regex-based attrs scan keeps the body intact and oxc
// parses it normally.
#[test]
fn vue_script_with_generic_type_argument() {
    let info = parse_sfc(
        r#"
<script setup lang="ts" generic="T extends Test<boolean>">
import type { Test } from './types';
import ChildComponent from './ChildComponent.vue';
defineProps<{ item: T }>();
</script>
<template>
  <ChildComponent label="hello" />
</template>
"#,
        "ParentComponent.vue",
    );
    assert!(
        info.imports.iter().any(|i| i.source == "./types"),
        "type-only import inside the script body must survive the generic attr",
    );
    assert!(
        info.imports
            .iter()
            .any(|i| i.source == "./ChildComponent.vue"),
        "value import inside the script body must survive the generic attr",
    );
    assert!(
        info.imports
            .iter()
            .find(|i| i.source == "./ChildComponent.vue")
            .is_some_and(|i| !i.is_type_only),
        "ChildComponent must remain a value import",
    );
    assert!(
        info.imports
            .iter()
            .find(|i| i.source == "./types")
            .is_some_and(|i| i.is_type_only),
        "Test must remain a type-only import",
    );
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
fn vue_script_setup_records_split_type_and_value_usage() {
    let info = parse_sfc(
        r#"
<script setup lang="ts">
import { Status } from './status';

const current = 'open' as Status;
const options = Object.values(Status);
</script>
"#,
        "SplitUsage.vue",
    );

    assert_eq!(
        info.type_referenced_import_bindings,
        vec!["Status".to_string()]
    );
    assert_eq!(
        info.value_referenced_import_bindings,
        vec!["Status".to_string()]
    );
}

#[test]
fn vue_script_setup_complexity_maps_to_sfc_lines() {
    let info = parse_sfc_with_complexity(
        r#"<template />
<script setup lang="ts">
const helper = () => {
  if (flag) return 1;
  return 0;
};
</script>
"#,
        "Complexity.vue",
    );

    assert_eq!(info.complexity.len(), 1);
    let function = &info.complexity[0];
    assert_eq!(function.name, "helper");
    assert_eq!(function.line, 3);
    assert_eq!(function.col, 15);
    assert_eq!(function.cyclomatic, 2);
}

#[test]
fn vue_inline_script_complexity_maps_columns_to_sfc_source() {
    let info = parse_sfc_with_complexity(
        r"<script>const helper = () => { if (flag) return 1; return 0; };</script>",
        "InlineComplexity.vue",
    );

    assert_eq!(info.complexity.len(), 1);
    let function = &info.complexity[0];
    assert_eq!(function.name, "helper");
    assert_eq!(function.line, 1);
    assert_eq!(function.col, 23);
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

/// Issue #475: a `.vue` file with a leading UTF-8 BOM must produce the same
/// `<script>` body line numbers as the same file without the BOM. The BOM is
/// stripped by `parse_source_to_module` before the SFC dispatcher sees the
/// source, so byte-range slicing of the `<script>` block lines up the same
/// way in both runs. Tech-lead Maeve flagged this as the one corner the plan
/// did not explicitly trace.
#[test]
fn sfc_vue_with_leading_bom_produces_same_script_line_numbers_as_without_bom() {
    let body = "<script lang=\"ts\">\n\
                import { ref } from 'vue';\n\
                export const counter = ref(0);\n\
                </script>\n\
                <template><div>{{ counter }}</div></template>\n";
    let with_bom = format!("\u{FEFF}{body}");

    let plain = parse_sfc(body, "Comp.vue");
    let bom = parse_sfc(&with_bom, "Comp.vue");

    assert_eq!(
        plain.exports.len(),
        bom.exports.len(),
        "BOM must not change the export count",
    );
    let plain_counter = plain
        .exports
        .iter()
        .find(|e| e.name.matches_str("counter"))
        .expect("plain Vue source exports `counter`");
    let bom_counter = bom
        .exports
        .iter()
        .find(|e| e.name.matches_str("counter"))
        .expect("BOM-bearing Vue source exports `counter`");
    assert_eq!(
        (plain_counter.span.start, plain_counter.span.end),
        (bom_counter.span.start, bom_counter.span.end),
        "Vue `<script>` body byte span must be identical with or without leading BOM",
    );
    assert_eq!(
        plain.imports.len(),
        bom.imports.len(),
        "BOM must not change the import count",
    );
}

// --- convention auto-import candidate capture (issue #704) ---

#[test]
fn captures_unresolved_pascal_tags_with_zero_imports() {
    // A Nuxt page may reference only convention auto-imported components, with
    // no `import` statement at all.
    let info = parse_sfc(
        r#"
<script setup lang="ts"></script>
<template>
  <Card001 />
  <BaseButton>hi</BaseButton>
  <div><span /></div>
</template>
"#,
        "pages/index.vue",
    );
    assert!(info.imports.is_empty(), "fixture has no imports");
    assert!(info.auto_import_candidates.contains(&"Card001".to_string()));
    assert!(
        info.auto_import_candidates
            .contains(&"BaseButton".to_string())
    );
    // Native lowercase HTML elements are never captured.
    assert!(!info.auto_import_candidates.contains(&"div".to_string()));
    assert!(!info.auto_import_candidates.contains(&"span".to_string()));
}

#[test]
fn captures_kebab_tag_as_pascal_candidate() {
    let info = parse_sfc(
        r#"
<script setup lang="ts"></script>
<template><base-button /></template>
"#,
        "pages/about.vue",
    );
    // Only the canonical Pascal form is captured, not the raw kebab or camel form.
    assert!(
        info.auto_import_candidates
            .contains(&"BaseButton".to_string())
    );
    assert!(
        !info
            .auto_import_candidates
            .contains(&"base-button".to_string())
    );
    assert!(
        !info
            .auto_import_candidates
            .contains(&"baseButton".to_string())
    );
}

#[test]
fn imported_component_tag_is_not_an_auto_import_candidate() {
    let info = parse_sfc(
        r#"
<script setup lang="ts">
import Card001 from './Card001.vue';
</script>
<template><Card001 /></template>
"#,
        "pages/index.vue",
    );
    // An explicitly imported tag is credited as a used import, not captured as
    // an auto-import candidate.
    assert!(!info.auto_import_candidates.contains(&"Card001".to_string()));
}
