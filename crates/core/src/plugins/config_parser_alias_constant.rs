//! Read alias pairs whose replacement can be a top-level constant.

use oxc_ast::ast::{Expression, ObjectPropertyKind};

use super::alias_entry::object_alias_pair;
use super::{
    MAX_ALIAS_RESOLVE_DEPTH, PathScope, alias_replacement_kinded, find_stable_binding_init,
    property_key_to_string,
};

/// Convert an alias object or `{ find, replacement }` array to alias tuples.
///
/// Each tuple carries a `replacement_is_bare_string_literal` flag. See
/// [`extract_config_aliases_kinded`](super::extract_config_aliases_kinded).
pub(super) fn expression_to_alias_pairs_kinded(
    expr: &Expression,
    scope: PathScope<'_>,
) -> Vec<(String, String, bool)> {
    match expr {
        Expression::ObjectExpression(obj) => obj
            .properties
            .iter()
            .filter_map(|prop| {
                let ObjectPropertyKind::ObjectProperty(prop) = prop else {
                    return None;
                };
                let find = property_key_to_string(&prop.key)?;
                let (replacement, is_bare) = alias_replacement_kinded(&prop.value, scope)?;
                Some((find, replacement, is_bare))
            })
            .collect(),
        Expression::ArrayExpression(arr) => arr
            .elements
            .iter()
            .filter_map(|element| {
                let Expression::ObjectExpression(obj) = element.as_expression()? else {
                    return None;
                };
                object_alias_pair(obj, scope)
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// The alias replacement that a stable top-level constant holds, as in
/// `const STUB = path.resolve(__dirname, "stub.ts")` with `replacement: STUB`.
///
/// A chain of constants (`const A = B`) resolves up to
/// [`MAX_ALIAS_RESOLVE_DEPTH`] hops. An array value gives no replacement, so
/// that a constant that holds itself (`const A = [A]`) can not recurse.
pub(super) fn constant_replacement(name: &str, scope: PathScope<'_>) -> Option<(String, bool)> {
    let PathScope::Program(program) = scope else {
        return None;
    };
    let mut name = name;
    for _ in 0..MAX_ALIAS_RESOLVE_DEPTH {
        let init = find_stable_binding_init(program, name, false)?;
        match init.get_inner_expression() {
            Expression::Identifier(next) => name = next.name.as_str(),
            Expression::ArrayExpression(_) => return None,
            value => return alias_replacement_kinded(value, scope),
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::super::{
        extract_config_aliases_kinded, extract_config_array_nested_aliases_kinded,
        extract_default_export_array_aliases_kinded,
    };

    fn ts_path() -> PathBuf {
        PathBuf::from("config.ts")
    }

    #[test]
    fn array_nested_alias_replacement_reads_a_path_constant() {
        let source = r#"
            import path from "node:path";
            const I18N_STUB = path.resolve(__dirname, "test/stubs/i18n.ts");
            export default {
                test: {
                    projects: [
                        { resolve: { alias: [{ find: "@/i18n/i18n", replacement: I18N_STUB }] } },
                        { resolve: { alias: { "@/i18n": I18N_STUB } } }
                    ]
                }
            };
        "#;
        let aliases = extract_config_array_nested_aliases_kinded(
            source,
            &ts_path(),
            &["test", "projects"],
            &["resolve", "alias"],
        );
        assert_eq!(
            aliases,
            vec![
                (
                    "@/i18n/i18n".to_string(),
                    "test/stubs/i18n.ts".to_string(),
                    false
                ),
                (
                    "@/i18n".to_string(),
                    "test/stubs/i18n.ts".to_string(),
                    false
                ),
            ]
        );
    }

    #[test]
    fn alias_replacement_reads_a_top_level_constant() {
        let source = r#"
            import path from "node:path";
            import { join } from "node:path";
            const LITERAL_STUB = "./test/stubs/literal.ts";
            const RESOLVE_STUB = path.resolve(__dirname, "test/stubs/resolve.ts");
            const JOIN_STUB = join(__dirname, "test/stubs/join.ts");
            const URL_STUB = new URL("./test/stubs/url.ts", import.meta.url).pathname;
            const CHAINED_STUB = RESOLVE_STUB;
            export default {
                resolve: {
                    alias: [
                        { find: "literal", replacement: LITERAL_STUB },
                        { find: "resolve", replacement: RESOLVE_STUB },
                        { find: "join", replacement: JOIN_STUB },
                        { find: "url", replacement: URL_STUB },
                        { find: "chained", replacement: CHAINED_STUB },
                    ]
                }
            };
        "#;
        let aliases = extract_config_aliases_kinded(source, &ts_path(), &["resolve", "alias"]);
        assert_eq!(
            aliases,
            vec![
                (
                    "literal".to_string(),
                    "./test/stubs/literal.ts".to_string(),
                    false
                ),
                (
                    "resolve".to_string(),
                    "test/stubs/resolve.ts".to_string(),
                    false
                ),
                ("join".to_string(), "test/stubs/join.ts".to_string(), false),
                ("url".to_string(), "./test/stubs/url.ts".to_string(), false),
                (
                    "chained".to_string(),
                    "test/stubs/resolve.ts".to_string(),
                    false
                ),
            ]
        );
    }

    #[test]
    fn alias_replacement_reads_a_constant_with_a_type_assertion() {
        let source = r#"
            import path from "node:path";
            export const STUB = path.resolve(__dirname, "test/stubs/a.ts") as string;
            export default { resolve: { alias: { a: STUB } } };
        "#;
        let aliases = extract_config_aliases_kinded(source, &ts_path(), &["resolve", "alias"]);
        assert_eq!(
            aliases,
            vec![("a".to_string(), "test/stubs/a.ts".to_string(), false)]
        );
    }

    #[test]
    fn alias_replacement_skips_a_constant_of_a_config_callback() {
        let source = r#"
            import path from "node:path";
            export default defineConfig(() => {
                const STUB = path.resolve(__dirname, "test/stubs/a.ts");
                return { resolve: { alias: { a: STUB } } };
            });
        "#;
        let aliases = extract_config_aliases_kinded(source, &ts_path(), &["resolve", "alias"]);
        assert!(aliases.is_empty(), "{aliases:?}");
    }

    #[test]
    fn workspace_array_alias_replacement_reads_a_path_constant() {
        let source = r#"
            import path from "node:path";
            const STUB = path.resolve(__dirname, "test/stubs/a.ts");
            export default [{ resolve: { alias: [{ find: "a", replacement: STUB }] } }];
        "#;
        let aliases =
            extract_default_export_array_aliases_kinded(source, &ts_path(), &["resolve", "alias"]);
        assert_eq!(
            aliases,
            vec![("a".to_string(), "test/stubs/a.ts".to_string(), false)]
        );
    }

    #[test]
    fn array_nested_alias_replacement_reads_a_local_path_helper() {
        let source = r#"
            import { fileURLToPath } from "node:url";
            const p = (rel) => fileURLToPath(new URL(rel, import.meta.url));
            export default {
                test: { projects: [{ resolve: { alias: { a: p("./test/stubs/a.ts") } } }] }
            };
        "#;
        let aliases = extract_config_array_nested_aliases_kinded(
            source,
            &ts_path(),
            &["test", "projects"],
            &["resolve", "alias"],
        );
        assert_eq!(
            aliases,
            vec![("a".to_string(), "./test/stubs/a.ts".to_string(), false)]
        );
    }

    #[test]
    fn alias_replacement_skips_a_constant_that_can_name_another_value() {
        let source = r#"
            import path from "node:path";
            let REASSIGNED = path.resolve(__dirname, "test/stubs/a.ts");
            REASSIGNED = path.resolve(__dirname, "test/stubs/b.ts");
            const SHADOWED = path.resolve(__dirname, "test/stubs/c.ts");
            function local(SHADOWED) { return SHADOWED; }
            const LOOP_A = LOOP_B;
            const LOOP_B = LOOP_A;
            const SELF = [SELF];
            const MUTATED = { path: path.resolve(__dirname, "test/stubs/d.ts") };
            MUTATED.path = "elsewhere";
            var LEGACY = path.resolve(__dirname, "test/stubs/e.ts");
            export default {
                resolve: {
                    alias: [
                        { find: "reassigned", replacement: REASSIGNED },
                        { find: "shadowed", replacement: SHADOWED },
                        { find: "loop", replacement: LOOP_A },
                        { find: "self", replacement: SELF },
                        { find: "mutated", replacement: MUTATED.path },
                        { find: "legacy", replacement: LEGACY },
                        { find: "unknown", replacement: NOT_DECLARED },
                    ]
                }
            };
        "#;
        let aliases = extract_config_aliases_kinded(source, &ts_path(), &["resolve", "alias"]);
        assert!(aliases.is_empty(), "{aliases:?}");
    }
}
