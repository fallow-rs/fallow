//! TypeScript plugin.
//!
//! Detects TypeScript projects and parses `tsconfig.json` for references,
//! extended configs, type packages, language service plugins, and array extends.
#![expect(
    clippy::excessive_nesting,
    reason = "tsconfig AST parsing requires deep nesting"
)]

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use super::config_parser;
use super::registry::ConfigCandidateIndex;
use super::{Plugin, PluginResult};

const ENABLERS: &[&str] = &["typescript"];
const CONFIG_PATTERNS: &[&str] = &["tsconfig.json", "tsconfig.*.json"];
const ALWAYS_USED: &[&str] = &["tsconfig.json", "tsconfig.*.json"];
const TOOLING_DEPENDENCIES: &[&str] = &["typescript", "ts-node", "tsx", "ts-loader"];

pub struct TypeScriptPlugin;

impl Plugin for TypeScriptPlugin {
    fn name(&self) -> &'static str {
        "typescript"
    }

    fn enablers(&self) -> &'static [&'static str] {
        ENABLERS
    }

    fn config_patterns(&self) -> &'static [&'static str] {
        CONFIG_PATTERNS
    }

    fn always_used(&self) -> &'static [&'static str] {
        ALWAYS_USED
    }

    fn tooling_dependencies(&self) -> &'static [&'static str] {
        TOOLING_DEPENDENCIES
    }

    /// Activate on a discovered `tsconfig.json` / `tsconfig.*.json`, not only on
    /// a declared `typescript` dependency.
    ///
    /// The plugin is the sole registrar of `compilerOptions.paths` into the
    /// project-wide alias table (`PluginResult.path_aliases`). A project that
    /// configures `paths` in a tsconfig but does not list `typescript` in its
    /// package.json (or keeps `paths` in a `tsconfig.app.json` the per-file
    /// nearest-`tsconfig.json` chain never reads) would otherwise leave those
    /// aliases unregistered, so an aliased import like
    /// `@acme/internal/common/request-context` falls through to npm-package
    /// classification and surfaces as a false `unlisted-dependency` finding
    /// (issue #1911). Mirrors the config-file activation used by `danger` /
    /// `k6` / `browser-extension`.
    fn is_enabled_with_files(
        &self,
        deps: &[String],
        root: &Path,
        _discovered_files: &[PathBuf],
        candidate_index: Option<&ConfigCandidateIndex>,
    ) -> bool {
        self.is_enabled_with_deps(deps, root) || tsconfig_present(root, candidate_index)
    }

    fn resolve_config(&self, config_path: &Path, source: &str, root: &Path) -> PluginResult {
        let mut result = PluginResult::default();

        let is_json = config_path.extension().is_some_and(|ext| ext == "json");
        let (parse_source, parse_path_buf) = if is_json {
            (format!("({source})"), config_path.with_extension("js"))
        } else {
            (source.to_string(), config_path.to_path_buf())
        };
        let parse_path: &Path = &parse_path_buf;

        if let Some(extends) =
            config_parser::extract_config_string(&parse_source, parse_path, &["extends"])
        {
            if extends.starts_with('.') || extends.starts_with('/') {
                result
                    .setup_files
                    .push(root.join(extends.trim_start_matches("./")));
            } else {
                let dep = crate::resolve::extract_package_name(&extends);
                result.referenced_dependencies.push(dep);
            }
        }

        let extends_arr =
            config_parser::extract_config_string_array(&parse_source, parse_path, &["extends"]);
        for ext in &extends_arr {
            if ext.starts_with('.') || ext.starts_with('/') {
                result
                    .setup_files
                    .push(root.join(ext.trim_start_matches("./")));
            } else {
                let dep = crate::resolve::extract_package_name(ext);
                result.referenced_dependencies.push(dep);
            }
        }

        let types = config_parser::extract_config_string_array(
            &parse_source,
            parse_path,
            &["compilerOptions", "types"],
        );
        for ty in &types {
            let base = crate::resolve::extract_package_name(ty);
            if !base.starts_with('@') {
                result
                    .referenced_dependencies
                    .push(format!("@types/{base}"));
            }
            result.referenced_dependencies.push(base);
        }

        if let Some(jsx_source) = config_parser::extract_config_string(
            &parse_source,
            parse_path,
            &["compilerOptions", "jsxImportSource"],
        ) {
            result.referenced_dependencies.push(jsx_source);
        }

        for (find, replacement) in config_parser::extract_config_path_aliases(
            &parse_source,
            parse_path,
            &["compilerOptions", "paths"],
        ) {
            let Some((normalized_find, normalized_replacement)) =
                normalize_tsconfig_path_alias(&find, &replacement, parse_path, root)
            else {
                continue;
            };
            result
                .path_aliases
                .push((normalized_find, normalized_replacement));
        }

        parse_tsconfig_plugins(&parse_source, parse_path, &mut result);

        parse_tsconfig_references(&parse_source, parse_path, root, &mut result);

        result
    }
}

/// Whether a `tsconfig.json` / `tsconfig.*.json` is present under `root`.
///
/// tsconfig files are non-source config candidates, so they never appear in the
/// activation call's `discovered_files`; outside production mode the discovery
/// walk's config index carries them (nested anywhere under `root`), and in
/// production (`candidate_index` is `None`) a bounded root-level filesystem
/// probe is the fallback, matching the root-level probe posture the
/// dependency-gated plugins use when the index is unavailable.
fn tsconfig_present(root: &Path, candidate_index: Option<&ConfigCandidateIndex>) -> bool {
    match candidate_index {
        Some(index) => {
            index.any_descendant_contains(root, OsStr::new("tsconfig.json"))
                || tsconfig_variant_matcher()
                    .is_some_and(|matcher| index.any_descendant_matches(root, matcher))
        }
        None => root_has_tsconfig(root),
    }
}

/// Cached matcher for the wildcard config filename (`tsconfig.app.json`,
/// `tsconfig.base.json`). Excludes the exact `tsconfig.json`, handled separately.
/// The pattern is a compile-time constant, so `None` is unreachable in practice.
fn tsconfig_variant_matcher() -> Option<&'static globset::GlobMatcher> {
    static MATCHER: OnceLock<Option<globset::GlobMatcher>> = OnceLock::new();
    MATCHER
        .get_or_init(|| {
            globset::Glob::new("tsconfig.*.json")
                .ok()
                .map(|glob| glob.compile_matcher())
        })
        .as_ref()
}

/// Production-mode fallback: a `tsconfig.json` or `tsconfig.*.json` directly at
/// the project root. Bounded to a single root directory read so a non-TypeScript
/// project pays at most one `read_dir` during activation.
fn root_has_tsconfig(root: &Path) -> bool {
    if root.join("tsconfig.json").is_file() {
        return true;
    }
    let Some(matcher) = tsconfig_variant_matcher() else {
        return false;
    };
    std::fs::read_dir(root).is_ok_and(|entries| {
        entries
            .flatten()
            .any(|entry| matcher.is_match(Path::new(&entry.file_name())))
    })
}

fn normalize_tsconfig_path_alias(
    find: &str,
    replacement: &Path,
    config_path: &Path,
    root: &Path,
) -> Option<(String, String)> {
    let normalized_find = find.strip_suffix('*').unwrap_or(find).to_string();
    if normalized_find.is_empty() {
        return None;
    }
    let replacement = config_parser::path_to_config_string(replacement);
    let normalized_replacement = replacement
        .strip_suffix("/*")
        .or_else(|| replacement.strip_suffix('*'))
        .unwrap_or(&replacement);
    let normalized_replacement =
        config_parser::normalize_config_path(normalized_replacement, config_path, root)?;

    Some((normalized_find, normalized_replacement))
}

/// Extract `compilerOptions.plugins[].name` from a tsconfig as referenced dependencies.
fn parse_tsconfig_plugins(source: &str, path: &Path, result: &mut PluginResult) {
    use oxc_allocator::Allocator;
    use oxc_ast::ast::Expression;
    use oxc_parser::Parser;
    use oxc_span::SourceType;

    let source_type = SourceType::from_path(path).unwrap_or_default();
    let alloc = Allocator::default();
    let parsed = Parser::new(&alloc, source, source_type).parse();

    let Some(obj) = config_parser::find_config_object_pub(&parsed.program) else {
        return;
    };

    let Some(compiler_opts) = find_object_property_object(obj, "compilerOptions") else {
        return;
    };

    let plugins_arr = compiler_opts.properties.iter().find_map(|prop| {
        use oxc_ast::ast::ObjectPropertyKind;
        if let ObjectPropertyKind::ObjectProperty(p) = prop
            && object_property_key_is(&p.key, "plugins")
            && let Expression::ArrayExpression(arr) = &p.value
        {
            return Some(arr);
        }
        None
    });
    let Some(plugins_arr) = plugins_arr else {
        return;
    };

    for el in &plugins_arr.elements {
        if let Some(Expression::ObjectExpression(plugin_obj)) = el.as_expression() {
            collect_tsconfig_plugin_name(plugin_obj, result);
        }
    }
}

/// True when an object-property key is the static identifier or string literal `name`.
fn object_property_key_is(key: &oxc_ast::ast::PropertyKey, name: &str) -> bool {
    use oxc_ast::ast::PropertyKey;
    match key {
        PropertyKey::StaticIdentifier(id) => id.name == name,
        PropertyKey::StringLiteral(s) => s.value == name,
        _ => false,
    }
}

/// Find a named object-valued property inside `obj`.
fn find_object_property_object<'a>(
    obj: &'a oxc_ast::ast::ObjectExpression<'a>,
    name: &str,
) -> Option<&'a oxc_ast::ast::ObjectExpression<'a>> {
    use oxc_ast::ast::{Expression, ObjectPropertyKind};
    obj.properties.iter().find_map(|prop| {
        if let ObjectPropertyKind::ObjectProperty(p) = prop
            && object_property_key_is(&p.key, name)
            && let Expression::ObjectExpression(inner) = &p.value
        {
            return Some(&**inner);
        }
        None
    })
}

/// Push the `name` field of a single tsconfig plugin object as a referenced dependency.
fn collect_tsconfig_plugin_name(
    plugin_obj: &oxc_ast::ast::ObjectExpression,
    result: &mut PluginResult,
) {
    use oxc_ast::ast::{Expression, ObjectPropertyKind};
    for prop in &plugin_obj.properties {
        if let ObjectPropertyKind::ObjectProperty(p) = prop
            && object_property_key_is(&p.key, "name")
            && let Expression::StringLiteral(s) = &p.value
        {
            let dep = crate::resolve::extract_package_name(&s.value);
            result.referenced_dependencies.push(dep);
        }
    }
}

/// Extract `references[].path` from a tsconfig and add them as setup files.
fn parse_tsconfig_references(source: &str, path: &Path, root: &Path, result: &mut PluginResult) {
    use oxc_allocator::Allocator;
    use oxc_ast::ast::{Expression, ObjectPropertyKind, PropertyKey};
    use oxc_parser::Parser;
    use oxc_span::SourceType;

    let source_type = SourceType::from_path(path).unwrap_or_default();
    let alloc = Allocator::default();
    let parsed = Parser::new(&alloc, source, source_type).parse();

    let Some(obj) = config_parser::find_config_object_pub(&parsed.program) else {
        return;
    };

    for prop in &obj.properties {
        if let ObjectPropertyKind::ObjectProperty(p) = prop {
            let is_references = match &p.key {
                PropertyKey::StaticIdentifier(id) => id.name == "references",
                PropertyKey::StringLiteral(s) => s.value == "references",
                _ => false,
            };
            if !is_references {
                continue;
            }
            if let Expression::ArrayExpression(arr) = &p.value {
                for el in &arr.elements {
                    if let Some(Expression::ObjectExpression(ref_obj)) = el.as_expression() {
                        for ref_prop in &ref_obj.properties {
                            if let ObjectPropertyKind::ObjectProperty(rp) = ref_prop {
                                let is_path = match &rp.key {
                                    PropertyKey::StaticIdentifier(id) => id.name == "path",
                                    PropertyKey::StringLiteral(s) => s.value == "path",
                                    _ => false,
                                };
                                if is_path && let Expression::StringLiteral(s) = &rp.value {
                                    let ref_path = s.value.to_string();
                                    let ref_target = root.join(ref_path.trim_start_matches("./"));
                                    let tsconfig_path = if ref_target
                                        .extension()
                                        .is_some_and(|ext| ext == "json")
                                    {
                                        ref_target
                                    } else {
                                        ref_target.join("tsconfig.json")
                                    };
                                    result.setup_files.push(tsconfig_path);
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn activates_from_declared_typescript_dependency() {
        let plugin = TypeScriptPlugin;
        let deps = vec!["typescript".to_string()];

        assert!(plugin.is_enabled_with_deps(&deps, Path::new("/project")));
        assert!(plugin.is_enabled_with_files(&deps, Path::new("/project"), &[], None));
    }

    #[test]
    fn activates_from_nested_tsconfig_variant_via_index() {
        // A `tsconfig.app.json` (the issue #1911 shape) is a config candidate,
        // not a source file, so it reaches the plugin through the discovery
        // index rather than `discovered_files`. Activation must fire even with
        // no `typescript` dependency, so the plugin's `compilerOptions.paths`
        // are registered project-wide.
        let plugin = TypeScriptPlugin;
        let tsconfig = PathBuf::from("/repo/apps/web/tsconfig.app.json");
        let index = ConfigCandidateIndex::build(std::iter::once(tsconfig.as_path()));

        assert!(plugin.is_enabled_with_files(&[], Path::new("/repo"), &[], Some(&index)));
        // Scoping: a tsconfig under a different root does not activate this root.
        assert!(!plugin.is_enabled_with_files(&[], Path::new("/other"), &[], Some(&index)));
    }

    #[test]
    fn activates_from_root_tsconfig_json_via_index() {
        let plugin = TypeScriptPlugin;
        let tsconfig = PathBuf::from("/repo/tsconfig.json");
        let index = ConfigCandidateIndex::build(std::iter::once(tsconfig.as_path()));

        assert!(plugin.is_enabled_with_files(&[], Path::new("/repo"), &[], Some(&index)));
    }

    #[test]
    fn does_not_activate_without_tsconfig_or_dependency() {
        let plugin = TypeScriptPlugin;
        let unrelated = PathBuf::from("/repo/src/index.ts");
        let index = ConfigCandidateIndex::build(std::iter::once(unrelated.as_path()));

        assert!(!plugin.is_enabled_with_files(&[], Path::new("/repo"), &[], Some(&index)));
    }

    #[test]
    fn similarly_named_files_do_not_activate_wildcard_matcher() {
        let plugin = TypeScriptPlugin;
        // A `mytsconfig.app.json` / `tsconfig.app.jsonc` must not match the
        // `tsconfig.*.json` wildcard.
        for name in ["/repo/mytsconfig.app.json", "/repo/tsconfig.app.jsonc"] {
            let path = PathBuf::from(name);
            let index = ConfigCandidateIndex::build(std::iter::once(path.as_path()));
            assert!(
                !plugin.is_enabled_with_files(&[], Path::new("/repo"), &[], Some(&index)),
                "{name} should not activate the plugin"
            );
        }
    }

    #[test]
    fn activates_from_root_tsconfig_variant_filesystem_probe() {
        // Production mode passes `candidate_index: None`; a root-level tsconfig
        // variant is found via the bounded filesystem probe.
        let plugin = TypeScriptPlugin;
        let tmp = tempfile::tempdir().expect("temp dir");
        std::fs::write(tmp.path().join("tsconfig.base.json"), "{}").expect("write tsconfig");

        assert!(plugin.is_enabled_with_files(&[], tmp.path(), &[], None));
    }

    #[test]
    fn resolve_config_extends_package() {
        let source = r#"{"extends": "@tsconfig/node18/tsconfig.json"}"#;
        let plugin = TypeScriptPlugin;
        let result = plugin.resolve_config(
            std::path::Path::new("tsconfig.json"),
            source,
            std::path::Path::new("/project"),
        );
        assert!(
            result
                .referenced_dependencies
                .contains(&"@tsconfig/node18".to_string())
        );
    }

    #[test]
    fn resolve_config_extends_relative_path() {
        let source = r#"{"extends": "./tsconfig.base.json"}"#;
        let plugin = TypeScriptPlugin;
        let result = plugin.resolve_config(
            std::path::Path::new("tsconfig.json"),
            source,
            std::path::Path::new("/project"),
        );
        assert!(result.referenced_dependencies.is_empty());
        assert!(
            result
                .setup_files
                .contains(&std::path::PathBuf::from("/project/tsconfig.base.json"))
        );
    }

    #[test]
    fn resolve_config_extends_array() {
        let source = r#"{"extends": ["./tsconfig.base.json", "@tsconfig/node18/tsconfig.json"]}"#;
        let plugin = TypeScriptPlugin;
        let result = plugin.resolve_config(
            std::path::Path::new("tsconfig.json"),
            source,
            std::path::Path::new("/project"),
        );
        assert!(
            result
                .setup_files
                .contains(&std::path::PathBuf::from("/project/tsconfig.base.json"))
        );
        assert!(
            result
                .referenced_dependencies
                .contains(&"@tsconfig/node18".to_string())
        );
    }

    #[test]
    fn resolve_config_compiler_options_types() {
        let source = r#"{"compilerOptions": {"types": ["node", "jest"]}}"#;
        let plugin = TypeScriptPlugin;
        let result = plugin.resolve_config(
            std::path::Path::new("tsconfig.json"),
            source,
            std::path::Path::new("/project"),
        );
        let deps = &result.referenced_dependencies;
        assert!(deps.contains(&"@types/node".to_string()));
        assert!(deps.contains(&"node".to_string()));
        assert!(deps.contains(&"@types/jest".to_string()));
        assert!(deps.contains(&"jest".to_string()));
    }

    #[test]
    fn resolve_config_jsx_import_source() {
        let source = r#"{"compilerOptions": {"jsxImportSource": "react"}}"#;
        let plugin = TypeScriptPlugin;
        let result = plugin.resolve_config(
            std::path::Path::new("tsconfig.json"),
            source,
            std::path::Path::new("/project"),
        );
        assert!(
            result
                .referenced_dependencies
                .contains(&"react".to_string())
        );
    }

    #[test]
    fn resolve_config_extracts_path_aliases_from_paths() {
        let source = r#"{
            "compilerOptions": {
                "paths": {
                    "@/*": ["./src/*"],
                    "@shared/*": ["./shared/*", "./fallback/*"]
                }
            }
        }"#;
        let plugin = TypeScriptPlugin;
        let result = plugin.resolve_config(
            std::path::Path::new("/project/tsconfig.app.json"),
            source,
            std::path::Path::new("/project"),
        );

        assert_eq!(
            result.path_aliases,
            vec![
                ("@/".to_string(), "src".to_string()),
                ("@shared/".to_string(), "shared".to_string())
            ]
        );
    }

    #[test]
    fn resolve_config_drops_wildcard_only_path_alias() {
        let source = r#"{
            "compilerOptions": {
                "paths": {
                    "*": ["./src/*"],
                    "@/*": ["./src/*"]
                }
            }
        }"#;
        let plugin = TypeScriptPlugin;
        let result = plugin.resolve_config(
            std::path::Path::new("/project/tsconfig.json"),
            source,
            std::path::Path::new("/project"),
        );

        assert_eq!(
            result.path_aliases,
            vec![("@/".to_string(), "src".to_string())],
        );
    }

    #[test]
    fn resolve_config_compiler_options_plugins() {
        let source =
            r#"{"compilerOptions": {"plugins": [{"name": "typescript-plugin-css-modules"}]}}"#;
        let plugin = TypeScriptPlugin;
        let result = plugin.resolve_config(
            std::path::Path::new("tsconfig.json"),
            source,
            std::path::Path::new("/project"),
        );
        assert!(
            result
                .referenced_dependencies
                .contains(&"typescript-plugin-css-modules".to_string())
        );
    }

    #[test]
    fn resolve_config_references() {
        let source = r#"{"references": [{"path": "./packages/core"}, {"path": "./packages/ui"}]}"#;
        let plugin = TypeScriptPlugin;
        let result = plugin.resolve_config(
            std::path::Path::new("tsconfig.json"),
            source,
            std::path::Path::new("/project"),
        );
        assert!(result.setup_files.contains(&std::path::PathBuf::from(
            "/project/packages/core/tsconfig.json"
        )));
        assert!(result.setup_files.contains(&std::path::PathBuf::from(
            "/project/packages/ui/tsconfig.json"
        )));
    }

    #[test]
    fn resolve_config_references_accept_direct_tsconfig_files() {
        let source = r#"{
            "references": [
                {"path": "./tsconfig.app.json"},
                {"path": "./packages/ui"}
            ]
        }"#;
        let plugin = TypeScriptPlugin;
        let result = plugin.resolve_config(
            std::path::Path::new("tsconfig.json"),
            source,
            std::path::Path::new("/project"),
        );

        assert!(
            result
                .setup_files
                .contains(&std::path::PathBuf::from("/project/tsconfig.app.json"))
        );
        assert!(result.setup_files.contains(&std::path::PathBuf::from(
            "/project/packages/ui/tsconfig.json"
        )));
    }

    #[test]
    fn resolve_config_with_comments_and_trailing_commas() {
        let source = r#"{
            // Base config for all packages
            "extends": "@tsconfig/strictest",
            "compilerOptions": {
                "types": ["node"],
            },
        }"#;
        let plugin = TypeScriptPlugin;
        let result = plugin.resolve_config(
            std::path::Path::new("tsconfig.json"),
            source,
            std::path::Path::new("/project"),
        );
        assert!(
            result
                .referenced_dependencies
                .contains(&"@tsconfig/strictest".to_string())
        );
        assert!(
            result
                .referenced_dependencies
                .contains(&"@types/node".to_string())
        );
    }
}
