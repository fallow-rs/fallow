//! Parity of the facts of an expression in script and in markup.
//!
//! Each case puts one expression once in a script function and once in the
//! markup of the same host. Extraction must give the same facts for both,
//! minus the facts that apply only in script. The field list below names
//! every `ModuleInfo` field, so a new fact fails to compile here until it is
//! sorted into a compared or an ignored group.

use std::collections::BTreeMap;
use std::sync::LazyLock;

use fallow_types::extract::{ModuleInfo, SemanticFact};

use super::parse_at_path;

/// The facts of a module, one entry per compared field. Each value is the
/// pretty `Debug` text with every number replaced and the lines sorted, so
/// spans and list order do not count.
type FactView = BTreeMap<&'static str, String>;

/// The global objects of the env read shapes.
const GLOBAL_OBJECTS: &[&str] = &["import.meta", "process"];

static NUMBER_RE: LazyLock<regex::Regex> = LazyLock::new(|| crate::static_regex(r"\d+"));

fn normalize(value: &dyn std::fmt::Debug) -> String {
    let text = format!("{value:#?}");
    let text = NUMBER_RE.replace_all(&text, "N");
    let mut lines: Vec<&str> = text.lines().map(str::trim).collect();
    lines.sort_unstable();
    lines.join("\n")
}

macro_rules! fact_view {
    ($info:expr; compared: [$($cmp:ident),* $(,)?]; ignored: [$($ign:ident),* $(,)?] $(;)?) => {{
        let ModuleInfo { $($cmp,)* $($ign: _,)* } = $info;
        let mut view = FactView::new();
        $(view.insert(stringify!($cmp), normalize($cmp));)*
        view
    }};
}

/// Give the script and the markup form of a whole-object read of an import
/// one form. The semantic pass of a script records it as a namespace member
/// use of the import. The markup scan records it as a whole-object use.
/// The markup scan records a member access only on a template-visible name
/// (an import or a bound target), so a member read of a global
/// (`import.meta.env.X`, `process.env.X`) counts only in its env read fact.
/// The top-level runtime statement fact is script-only
/// (`ModuleInfoExtractor::clear_script_only_facts`), so it goes too.
fn canonical(info: &ModuleInfo) -> ModuleInfo {
    let mut info = info.clone();
    info.member_accesses = info
        .member_accesses
        .iter()
        .filter(|access| {
            !GLOBAL_OBJECTS.iter().any(|global| {
                access.object == *global
                    || access
                        .object
                        .strip_prefix(global)
                        .is_some_and(|rest| rest.starts_with('.'))
            })
        })
        .cloned()
        .collect();
    let mut whole_object_uses = info.whole_object_uses.to_vec();
    let semantic_facts: Vec<SemanticFact> = info
        .semantic_facts
        .iter()
        .filter(|fact| match fact {
            SemanticFact::NamespaceMemberUse(fact) => {
                if fact.whole && fact.path.is_empty() {
                    whole_object_uses.push(fact.object.clone());
                }
                false
            }
            SemanticFact::TopLevelRuntimeStatement => false,
            _ => true,
        })
        .cloned()
        .collect();
    whole_object_uses.sort_unstable();
    whole_object_uses.dedup();
    info.whole_object_uses = whole_object_uses.into();
    info.semantic_facts = semantic_facts.into();
    info
}

#[expect(clippy::too_many_lines, reason = "one line per ModuleInfo field")]
fn facts(info: &ModuleInfo) -> FactView {
    let info = &canonical(info);
    fact_view!(info;
        compared: [
            dynamic_imports,
            dynamic_import_patterns,
            require_calls,
            package_path_references,
            type_package_references,
            bin_path_references,
            package_resolve_sites,
            member_accesses,
            semantic_facts,
            whole_object_uses,
            has_cjs_exports,
            has_angular_component_template_url,
            unused_import_bindings,
            flag_uses,
            flag_registry_facts,
            exported_factory_returns,
            exported_factory_return_object_shapes,
            type_member_types,
            injection_tokens,
            namespace_object_aliases,
            iconify_prefixes,
            iconify_icon_names,
            client_only_dynamic_import_spans,
            env_reads,
            security_sinks,
            security_sinks_skipped,
            security_unresolved_callee_sites,
            tainted_bindings,
            sanitized_sink_args,
            security_control_sites,
            callee_uses,
            command_invocations,
            di_key_sites,
            has_dynamic_provide,
            is_server_action_module,
            has_global_declarations,
            triple_slash_reference_paths,
            jsx_runtime_from_config,
            component_props,
            effect_facts,
            has_props_attrs_fallthrough,
            has_define_expose,
            has_define_model,
            has_unharvestable_props,
            component_emits,
            angular_inputs,
            angular_outputs,
            angular_component_selectors,
            registered_custom_elements,
            used_custom_element_tags,
            angular_used_selectors,
            angular_entry_component_refs,
            has_dynamic_component_render,
            has_unharvestable_emits,
            has_dynamic_emit,
            has_emit_whole_object_use,
            has_load_data_whole_use,
            has_page_data_store_whole_use,
            has_route_loader_data_whole_use,
            component_functions,
            react_props,
            hook_uses,
            render_edges,
            svelte_dispatched_events,
            svelte_listened_events,
            has_dynamic_dispatch,
        ];
        ignored: [
            // Host facts: the two sources differ in text and layout.
            file_id,
            content_hash,
            line_offsets,
            complexity,
            parse_error_count,
            parse_panicked,
            parse_nesting_too_deep,
            suppressions,
            unknown_suppression_kinds,
            range_marker_problems,
            inline_threshold_overrides,
            // A markup snippet has no import declarations, and an HTML page
            // adds its asset references.
            imports,
            // Script-only facts (`ModuleInfoExtractor::clear_script_only_facts`).
            exports,
            re_exports,
            local_type_declarations,
            public_signature_type_references,
            function_contracts,
            class_heritage,
            directives,
            misplaced_directives,
            inline_server_action_exports,
            load_return_keys,
            has_unharvestable_load,
            inert_function_exports,
            // Facts of the semantic pass over a whole script: the import
            // references and the unresolved names. A markup snippet has no
            // import declarations, so the template credit goes through the
            // `unused_import_bindings` list that is compared above.
            auto_import_candidates,
            component_contracts,
            type_referenced_import_bindings,
            value_referenced_import_bindings,
            referenced_import_bindings,
            imported_call_sites,
            import_binding_references,
        ];
    )
}

struct Shape {
    name: &'static str,
    expression: &'static str,
}

const IMPORTS: &str = "import { helper, ns, code } from './helpers';\nconst literal = '1 + 1';";

const SHAPES: &[Shape] = &[
    Shape {
        name: "call",
        expression: "helper(1)",
    },
    Shape {
        name: "member read",
        expression: "ns.value",
    },
    Shape {
        name: "dynamic import",
        expression: "import('./lazy')",
    },
    Shape {
        name: "sink call",
        expression: "eval(code)",
    },
    Shape {
        name: "sink call with a script literal",
        expression: "eval(literal)",
    },
    Shape {
        name: "import.meta env read",
        expression: "helper(import.meta.env.VITE_API_URL)",
    },
    Shape {
        name: "process env read",
        expression: "helper(process.env.API_URL)",
    },
];

struct Host {
    name: &'static str,
    path: &'static str,
    script: fn(&str) -> String,
    markup: fn(&str) -> String,
    /// Fields that the host computes in its own way, not from the expression.
    own_fields: &'static [&'static str],
}

fn svelte_script(expression: &str) -> String {
    format!(
        "<script>\nimport {{ createEventDispatcher }} from 'svelte';\n{IMPORTS}\n\
         const dispatch = createEventDispatcher();\n\
         function handler() {{ {expression}; }}\n</script>\n<button on:click={{handler}}>x</button>\n"
    )
}

fn svelte_markup(expression: &str) -> String {
    format!(
        "<script>\nimport {{ createEventDispatcher }} from 'svelte';\n{IMPORTS}\n\
         const dispatch = createEventDispatcher();\n</script>\n\
         <button on:click={{() => {{ {expression}; }}}}>x</button>\n"
    )
}

fn vue_script(expression: &str) -> String {
    format!(
        "<script setup>\n{IMPORTS}\nconst emit = defineEmits(['close', 'open']);\n\
         function handler() {{ {expression}; }}\n</script>\n\
         <template><button @click=\"handler\">x</button></template>\n"
    )
}

fn vue_markup(expression: &str) -> String {
    format!(
        "<script setup>\n{IMPORTS}\nconst emit = defineEmits(['close', 'open']);\n</script>\n\
         <template><button @click=\"{expression}\">x</button></template>\n"
    )
}

fn astro_script(expression: &str) -> String {
    format!(
        "---\n{IMPORTS}\nconst answer = 1;\nfunction handler() {{ {expression}; }}\n---\n<p>{{answer}}</p>\n"
    )
}

fn astro_markup(expression: &str) -> String {
    format!("---\n{IMPORTS}\nconst answer = 1;\n---\n<p>{{answer}}{{{expression}}}</p>\n")
}

fn html_script(expression: &str) -> String {
    format!("{IMPORTS}\nfunction handler() {{ {expression}; }}\n")
}

fn html_markup(expression: &str) -> String {
    format!(
        "<!doctype html>\n<html><body>\n<script type=\"module\">\n{IMPORTS}\n\
         function handler() {{ {expression}; }}\n</script>\n</body></html>\n"
    )
}

const HOSTS: &[Host] = &[
    Host {
        name: "svelte",
        path: "Component.svelte",
        script: svelte_script,
        markup: svelte_markup,
        own_fields: &[],
    },
    Host {
        name: "vue",
        path: "Component.vue",
        script: vue_script,
        markup: vue_markup,
        own_fields: &[],
    },
    Host {
        name: "astro",
        path: "Component.astro",
        script: astro_script,
        markup: astro_markup,
        // The Astro text scan credits each frontmatter import that a template
        // region names as a whole-object use, on top of the visitor facts.
        own_fields: &["whole_object_uses"],
    },
];

/// The facts that differ between the script and the markup source.
fn fact_diff(host: &Host, path_for_script: &str, expression: &str) -> Vec<String> {
    let script = facts(&parse_at_path(path_for_script, &(host.script)(expression)));
    let markup = facts(&parse_at_path(host.path, &(host.markup)(expression)));
    script
        .iter()
        .filter(|(field, _)| !host.own_fields.contains(field))
        .filter(|(field, value)| markup.get(*field) != Some(*value))
        .map(|(field, value)| {
            format!(
                "{field}:\n  script: {}\n  markup: {}",
                value.replace('\n', " "),
                markup
                    .get(field)
                    .map_or(String::new(), |v| v.replace('\n', " "))
            )
        })
        .collect()
}

fn parity_failure(
    host: &Host,
    path_for_script: &str,
    shape_name: &str,
    expression: &str,
) -> Option<String> {
    let diff = fact_diff(host, path_for_script, expression);
    (!diff.is_empty()).then(|| {
        format!(
            "{} {shape_name} `{expression}`: script and markup facts differ\n{}",
            host.name,
            diff.join("\n")
        )
    })
}

fn assert_parity(cases: &[(&Host, &str, &str, &str)]) {
    let failures: Vec<String> = cases
        .iter()
        .filter_map(|(host, path, shape, expression)| parity_failure(host, path, shape, expression))
        .collect();
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
}

#[test]
fn markup_expressions_give_the_script_facts() {
    let cases: Vec<(&Host, &str, &str, &str)> = HOSTS
        .iter()
        .flat_map(|host| {
            SHAPES
                .iter()
                .map(move |shape| (host, host.path, shape.name, shape.expression))
        })
        .collect();
    assert_parity(&cases);
}

#[test]
fn svelte_markup_dispatch_gives_the_script_facts() {
    let host = &HOSTS[0];
    assert_parity(&[
        (host, host.path, "dispatch", "dispatch('save')"),
        (host, host.path, "dynamic dispatch", "dispatch(`a${1}`)"),
    ]);
}

#[test]
fn vue_markup_statement_gives_the_script_facts() {
    let host = &HOSTS[1];
    assert_parity(&[(host, host.path, "statements", "helper(1); eval(code)")]);
}

#[test]
fn vue_markup_emit_gives_the_script_facts() {
    let host = &HOSTS[1];
    assert_parity(&[
        (host, host.path, "emit", "emit('close')"),
        (host, host.path, "dynamic emit", "emit(`a${1}`)"),
    ]);
}

#[test]
fn html_inline_module_script_gives_the_script_facts() {
    let host = Host {
        name: "html",
        path: "index.html",
        script: html_script,
        markup: html_markup,
        // A page credits each import of its inline scripts, and runs no
        // semantic pass that records a whole read of an import.
        own_fields: &["unused_import_bindings", "whole_object_uses"],
    };
    let cases: Vec<(&Host, &str, &str, &str)> = SHAPES
        .iter()
        .map(|shape| (&host, "module.js", shape.name, shape.expression))
        .collect();
    assert_parity(&cases);
}

#[test]
fn html_inline_script_require_is_no_module_load() {
    let info = parse_at_path(
        "index.html",
        "<!doctype html><script>if (typeof require === 'function') { module.exports = require('left-pad'); }</script>",
    );
    assert!(info.require_calls.is_empty(), "{:?}", info.require_calls);
    assert!(!info.has_cjs_exports);
}

/// A Svelte markup local (`{#each}` item) shadows the script binding of the
/// same name: a shadowed dispatcher dispatches nothing, and a shadowed script
/// literal is no literal, as for a script function parameter.
#[test]
fn markup_local_shadows_a_script_binding() {
    let head = "<script>\nimport { createEventDispatcher } from 'svelte';\n\
                const dispatch = createEventDispatcher();\nconst literal = '1 + 1';\n\
                const handlers = [];\n";
    let script = format!(
        "{head}function run(literal) {{ eval(literal); }}\n\
         </script>\n<button on:click={{run}}>x</button>\n"
    );
    let markup = format!(
        "{head}</script>\n{{#each handlers as dispatch}}{{#each handlers as literal}}\
         <button on:click={{() => {{ dispatch('save'); eval(literal); }}}}>x</button>\
         {{/each}}{{/each}}\n"
    );
    let script = parse_at_path("Component.svelte", &script);
    let markup = parse_at_path("Component.svelte", &markup);
    assert!(
        markup.svelte_dispatched_events.is_empty() && !markup.has_dynamic_dispatch,
        "a shadowed dispatch is no dispatcher: {:?}",
        markup.svelte_dispatched_events
    );
    assert!(!script.security_sinks.is_empty());
    assert_eq!(
        normalize(&script.security_sinks),
        normalize(&markup.security_sinks),
        "a shadowed script literal is no literal in markup either"
    );
}

/// The template scan reads only HTML tags. For a template in another
/// language, the component must never report an emit that the same call in
/// script marks as used.
#[test]
fn non_html_template_never_loses_an_emit_that_script_marks_used() {
    let script = parse_at_path("Component.vue", &vue_script("emit('close')"));
    assert!(
        script
            .component_emits
            .iter()
            .any(|emit| emit.name == "close" && emit.used)
    );
    for lang in ["pug", "slm"] {
        let markup = parse_at_path(
            "Component.vue",
            &format!(
                "<script setup>\nconst emit = defineEmits(['close', 'open']);\n</script>\n\
                 <template lang=\"{lang}\">\nbutton(@click=\"emit('close')\") x\n</template>\n"
            ),
        );
        let close_used = markup
            .component_emits
            .iter()
            .any(|emit| emit.name == "close" && emit.used);
        assert!(
            close_used || markup.has_dynamic_emit,
            "lang=\"{lang}\": `close` would be reported as unused"
        );
    }
}
