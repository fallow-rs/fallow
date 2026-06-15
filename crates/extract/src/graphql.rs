//! GraphQL document parsing.
//!
//! Supports the widely-used `#import "./fragment.graphql"` convention by
//! turning relative document imports into side-effect module edges.

use std::path::Path;
use std::sync::LazyLock;

use oxc_span::Span;

use crate::{ImportInfo, ImportedName, ModuleInfo};
use fallow_types::discover::FileId;

static GRAPHQL_IMPORT_RE: LazyLock<regex::Regex> =
    LazyLock::new(|| crate::static_regex(r#"(?m)^[ \t]*#\s*import\s+["']([^"'\r\n]+)["']"#));

pub(crate) fn is_graphql_file(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|ext| ext == "graphql" || ext == "gql")
}

fn is_relative_graphql_import(source: &str) -> bool {
    source.starts_with("./") || source.starts_with("../")
}

#[expect(
    clippy::cast_possible_truncation,
    reason = "source spans are bounded by source file size, which is practically below u32::MAX"
)]
fn span_from_usize(start: usize, end: usize) -> Span {
    Span::new(start as u32, end as u32)
}

#[must_use]
pub(crate) fn extract_graphql_imports(source: &str) -> Vec<ImportInfo> {
    let mut imports = Vec::new();

    for cap in GRAPHQL_IMPORT_RE.captures_iter(source) {
        let Some(source_match) = cap.get(1) else {
            continue;
        };
        let import_source = source_match.as_str().trim();
        if import_source.is_empty() || !is_relative_graphql_import(import_source) {
            continue;
        }

        imports.push(ImportInfo {
            source: import_source.to_string(),
            imported_name: ImportedName::SideEffect,
            local_name: String::new(),
            is_type_only: false,
            from_style: false,
            span: cap
                .get(0)
                .map_or_else(Span::default, |m| span_from_usize(m.start(), m.end())),
            source_span: span_from_usize(source_match.start(), source_match.end()),
        });
    }

    imports.sort_unstable_by(|a, b| {
        a.source
            .cmp(&b.source)
            .then(a.source_span.start.cmp(&b.source_span.start))
    });
    imports.dedup_by(|a, b| a.source == b.source);
    imports
}

pub(crate) fn parse_graphql_to_module(
    file_id: FileId,
    source: &str,
    content_hash: u64,
) -> ModuleInfo {
    let parsed_suppressions = crate::suppress::parse_suppressions_from_source(source);
    ModuleInfo {
        file_id,
        exports: Vec::new(),
        imports: extract_graphql_imports(source),
        re_exports: Vec::new(),
        dynamic_imports: Vec::new(),
        dynamic_import_patterns: Vec::new(),
        require_calls: Vec::new(),
        package_path_references: Vec::new(),
        member_accesses: Vec::new(),
        whole_object_uses: Vec::new(),
        has_cjs_exports: false,
        has_angular_component_template_url: false,
        content_hash,
        suppressions: parsed_suppressions.suppressions,
        unknown_suppression_kinds: parsed_suppressions.unknown_kinds,
        unused_import_bindings: Vec::new(),
        type_referenced_import_bindings: Vec::new(),
        value_referenced_import_bindings: Vec::new(),
        line_offsets: fallow_types::extract::compute_line_offsets(source),
        complexity: Vec::new(),
        flag_uses: Vec::new(),
        class_heritage: Vec::new(),
        injection_tokens: Vec::new(),
        local_type_declarations: Vec::new(),
        public_signature_type_references: Vec::new(),
        namespace_object_aliases: Vec::new(),
        iconify_prefixes: Vec::new(),
        iconify_icon_names: Vec::new(),
        auto_import_candidates: Vec::new(),
        directives: Vec::new(),
        client_only_dynamic_import_spans: Vec::new(),
        security_sinks: Vec::new(),
        security_sinks_skipped: 0,
        security_unresolved_callee_sites: Vec::new(),
        tainted_bindings: Vec::new(),
        sanitized_sink_args: Vec::new(),
        security_control_sites: Vec::new(),
        callee_uses: Vec::new(),
        misplaced_directives: Vec::new(),
        di_key_sites: Vec::new(),
        has_dynamic_provide: false,
        referenced_import_bindings: Vec::new(),
        component_props: Vec::new(),
        has_props_attrs_fallthrough: false,
        has_define_expose: false,
        has_define_model: false,
        has_unharvestable_props: false,
        component_emits: Vec::new(),
        has_unharvestable_emits: false,
        has_dynamic_emit: false,
        has_emit_whole_object_use: false,
        load_return_keys: Vec::new(),
        has_unharvestable_load: false,
        has_load_data_whole_use: false,
        has_page_data_store_whole_use: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn graphql_file_extensions_are_supported() {
        assert!(is_graphql_file(Path::new("schema.graphql")));
        assert!(is_graphql_file(Path::new("fragment.gql")));
        assert!(!is_graphql_file(Path::new("query.ts")));
    }

    #[test]
    fn extracts_relative_hash_imports() {
        let imports = extract_graphql_imports(
            r#"
            #import "./content.graphql"
            # import '../shared/leaf.gql'
            #import "package/schema.graphql"
            fragment Story on Story { id }
            "#,
        );

        let sources: Vec<&str> = imports
            .iter()
            .map(|import| import.source.as_str())
            .collect();
        assert_eq!(sources, vec!["../shared/leaf.gql", "./content.graphql"]);
        assert!(
            imports
                .iter()
                .all(|import| matches!(import.imported_name, ImportedName::SideEffect))
        );
    }

    #[test]
    fn parse_graphql_to_module_sets_imports_and_offsets() {
        let info = parse_graphql_to_module(
            FileId(7),
            "#import \"./content.graphql\"\nfragment Story on Story { id }\n",
            42,
        );

        assert_eq!(info.file_id, FileId(7));
        assert_eq!(info.content_hash, 42);
        assert_eq!(info.imports.len(), 1);
        assert_eq!(info.imports[0].source, "./content.graphql");
        assert_eq!(info.line_offsets, vec![0, 28, 59]);
    }
}
