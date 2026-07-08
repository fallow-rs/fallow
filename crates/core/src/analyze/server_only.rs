//! Shared server-only module predicate for the Next.js RSC detectors.
//!
//! A module is SERVER-ONLY when it carries a `"use server"` directive, imports
//! the `server-only` poison package (or another known server-only package), or
//! imports a named server-only Next.js API from `next/headers`. The predicate
//! is deliberately conservative (false-negative-preferring): a generic "DB
//! client" or any package-name guess is NOT here, because there is no clean
//! syntactic sink for it.
//!
//! This single predicate is shared between the `security` client/server-leak
//! detector and the `mixed_client_server_barrel` detector so the server-only
//! definition (the [`SERVER_ONLY_PACKAGES`] list, `next/headers` named-import
//! handling, and the `"use server"` directive check) never drifts.

use fallow_types::extract::{ImportedName, ModuleInfo};

/// The React Server Components / Server Actions directive marking a module as
/// server-only.
const USE_SERVER: &str = "use server";

/// The canonical "poison" package: importing it makes a module fail the build
/// if it is ever bundled for the client. Its presence is a strong server-only
/// signal.
const SERVER_ONLY_POISON_PACKAGE: &str = "server-only";

/// Server-only package specifiers that mark a module as a server-only sink no
/// matter which name is imported. Deliberately conservative and narrow: a
/// generic "DB client" or any package-name guess is NOT here. The `node:` and
/// bare forms BOTH count, per the import-shape rule, so both are listed
/// explicitly. `next/headers` is handled separately ([`NEXT_HEADERS_SOURCE`] +
/// [`NEXT_HEADERS_SERVER_NAMES`]) because the sink is the specific server-only
/// named API, not the module path.
const SERVER_ONLY_PACKAGES: &[&str] = &[
    SERVER_ONLY_POISON_PACKAGE,
    "next/server",
    "node:fs",
    "fs",
    "node:fs/promises",
    "fs/promises",
    "node:child_process",
    "child_process",
];

/// The `next/headers` module specifier, gated on a named server-only import.
const NEXT_HEADERS_SOURCE: &str = "next/headers";

/// Server-only NAMED imports from `next/headers`. Importing any of these (named
/// or namespace member) is reaching server-only runtime APIs Next.js refuses to
/// run on the client. A bare side-effect `import "next/headers"` (no name) is
/// NOT flagged: without a named server-only binding it is just a module load,
/// the conservative no-false-positive choice.
const NEXT_HEADERS_SERVER_NAMES: &[&str] = &["cookies", "headers", "draftMode"];

/// Whether a single module qualifies as a server-only sink. Conservative: a
/// `"use server"` directive, an import of a known server-only package, or a
/// named server-only API from `next/headers`. The package check matches the
/// import specifier directly, so `node:fs` and bare `fs` both count.
#[must_use]
pub fn is_server_only_module(module: &ModuleInfo) -> bool {
    // 1. A "use server" directive (Server Actions / server-only module).
    if module.directives.iter().any(|d| d == USE_SERVER) {
        return true;
    }
    module.imports.iter().any(|import| {
        // 2/3. An import of the `server-only` poison package, a Node server
        // runtime module, or `next/server`. The specifier is matched directly
        // so both the `node:` and bare forms count.
        if SERVER_ONLY_PACKAGES.contains(&import.source.as_str()) {
            return true;
        }
        // A server-only named API from `next/headers` (cookies / headers /
        // draftMode), in named (`import { cookies }`) or namespace
        // (`import * as h`) form. A bare side-effect import does not match.
        import.source == NEXT_HEADERS_SOURCE && is_next_headers_server_import(&import.imported_name)
    })
}

/// Whether an import binding from `next/headers` names a server-only API. A
/// named import of `cookies` / `headers` / `draftMode` matches; a namespace
/// import (`import * as headers from "next/headers"`) matches conservatively
/// because any member access could be a server-only API. A side-effect or
/// default import does not match.
fn is_next_headers_server_import(name: &ImportedName) -> bool {
    match name {
        ImportedName::Named(named) => NEXT_HEADERS_SERVER_NAMES.contains(&named.as_str()),
        ImportedName::Namespace => true,
        ImportedName::Default | ImportedName::SideEffect => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::discover::FileId;
    use fallow_types::extract::{ImportInfo, ModuleInfo};

    fn empty_module() -> ModuleInfo {
        ModuleInfo {
            file_id: FileId(0),
            exports: vec![],
            imports: vec![],
            re_exports: vec![],
            dynamic_imports: vec![],
            dynamic_import_patterns: vec![],
            require_calls: vec![],
            package_path_references: Box::default(),
            member_accesses: vec![],
            semantic_facts: Box::default(),
            whole_object_uses: Box::default(),
            has_cjs_exports: false,
            has_angular_component_template_url: false,
            content_hash: 0,
            suppressions: vec![],
            unknown_suppression_kinds: vec![],
            unused_import_bindings: vec![],
            type_referenced_import_bindings: vec![],
            value_referenced_import_bindings: vec![],
            line_offsets: vec![],
            complexity: vec![],
            flag_uses: vec![],
            class_heritage: vec![],
            exported_factory_returns: Box::default(),
            type_member_types: Box::default(),
            injection_tokens: vec![],
            local_type_declarations: vec![],
            public_signature_type_references: vec![],
            namespace_object_aliases: vec![],
            iconify_prefixes: vec![],
            iconify_icon_names: vec![],
            auto_import_candidates: vec![],
            directives: vec![],
            client_only_dynamic_import_spans: vec![],
            security_sinks: vec![],
            security_sinks_skipped: 0,
            security_unresolved_callee_sites: Vec::new(),
            tainted_bindings: vec![],
            sanitized_sink_args: vec![],
            security_control_sites: vec![],
            callee_uses: vec![],
            misplaced_directives: vec![],
            inline_server_action_exports: Vec::new(),
            di_key_sites: Vec::new(),
            has_dynamic_provide: false,
            referenced_import_bindings: Vec::new(),
            component_props: Vec::new(),
            has_props_attrs_fallthrough: false,
            has_define_expose: false,
            has_define_model: false,
            has_unharvestable_props: false,
            component_emits: Vec::new(),
            angular_inputs: Vec::new(),
            angular_outputs: Vec::new(),
            has_unharvestable_emits: false,
            has_dynamic_emit: false,
            has_emit_whole_object_use: false,
            load_return_keys: Vec::new(),
            has_unharvestable_load: false,
            has_load_data_whole_use: false,
            has_page_data_store_whole_use: false,
            component_functions: Vec::new(),
            react_props: Vec::new(),
            hook_uses: Vec::new(),
            render_edges: Vec::new(),
            svelte_dispatched_events: Vec::new(),
            svelte_listened_events: Vec::new(),
            angular_component_selectors: Vec::new(),
            registered_custom_elements: Vec::new(),
            used_custom_element_tags: Vec::new(),
            angular_used_selectors: Vec::new(),
            angular_entry_component_refs: Vec::new(),
            has_dynamic_component_render: false,
            has_dynamic_dispatch: false,
        }
    }

    fn module_with_import(source: &str, imported: ImportedName) -> ModuleInfo {
        let mut module = empty_module();
        module.imports.push(ImportInfo {
            source: source.to_string(),
            imported_name: imported,
            local_name: "x".to_string(),
            is_type_only: false,
            from_style: false,
            span: oxc_span::Span::new(0, 10),
            source_span: oxc_span::Span::new(0, 10),
        });
        module
    }

    #[test]
    fn use_server_directive_is_server_only() {
        let mut module = empty_module();
        module.directives.push(USE_SERVER.to_string());
        assert!(is_server_only_module(&module));
    }

    #[test]
    fn server_only_poison_package_is_server_only() {
        let module = module_with_import(SERVER_ONLY_POISON_PACKAGE, ImportedName::SideEffect);
        assert!(is_server_only_module(&module));
    }

    #[test]
    fn node_fs_and_bare_fs_both_count() {
        assert!(is_server_only_module(&module_with_import(
            "node:fs",
            ImportedName::Named("readFileSync".to_string()),
        )));
        assert!(is_server_only_module(&module_with_import(
            "fs",
            ImportedName::Named("readFileSync".to_string()),
        )));
    }

    #[test]
    fn next_headers_named_server_api_counts() {
        assert!(is_server_only_module(&module_with_import(
            NEXT_HEADERS_SOURCE,
            ImportedName::Named("cookies".to_string()),
        )));
    }

    #[test]
    fn next_headers_side_effect_import_does_not_count() {
        assert!(!is_server_only_module(&module_with_import(
            NEXT_HEADERS_SOURCE,
            ImportedName::SideEffect,
        )));
    }

    #[test]
    fn plain_utility_module_is_not_server_only() {
        let module = module_with_import("./format", ImportedName::Named("formatDate".to_string()));
        assert!(!is_server_only_module(&module));
    }
}
