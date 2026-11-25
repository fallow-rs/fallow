//! Oxlint plugin.
//!
//! Detects Oxlint projects and marks config files as always used.

use std::path::{Path, PathBuf};

use oxc_ast::ast::{
    Argument, ArrayExpressionElement, ArrowFunctionExpression, Expression, Function,
    ImportDeclarationSpecifier, ObjectExpression, Program, Statement, VariableDeclarationKind,
};
use oxc_ast_visit::{Visit, walk};
use oxc_span::Span;
use oxc_syntax::scope::ScopeFlags;
use rustc_hash::FxHashSet;

use super::config_parser;
use super::{Plugin, PluginResult};

const ENABLERS: &[&str] = &["oxlint"];

/// Oxlint discovers `.oxlintrc.json`, `.oxlintrc.jsonc`, `oxlint.config.ts`
/// and `oxlint.config.mts` on its own. `oxlint.json` stays for older setups.
const CONFIG_PATTERNS: &[&str] = &[
    ".oxlintrc.{json,jsonc}",
    "oxlint.json",
    "oxlint.config.{ts,mts}",
];

const ALWAYS_USED: &[&str] = CONFIG_PATTERNS;

/// Module extensions that a local plugin file can have, in probe order.
const MODULE_EXTENSIONS: &[&str] = &["js", "mjs", "cjs", "ts", "mts", "cts"];

const TOOLING_DEPENDENCIES: &[&str] = &["oxlint", "oxlint-tsgolint"];

/// The number of imported preset levels that the plugin reads below a config.
const MAX_IMPORTED_PRESET_DEPTH: usize = 4;

const ULTRACITE_JS_PLUGINS_SOURCE: &str = "ultracite/oxlint/js-plugins";

const ULTRACITE_JS_PLUGIN_DEPENDENCIES: &[(&str, &str)] = &[
    ("github", "eslint-plugin-github"),
    ("sonarjs", "eslint-plugin-sonarjs"),
    ("react-doctor", "oxlint-plugin-react-doctor"),
];

define_plugin! {
    struct OxlintPlugin => "oxlint",
    enablers: ENABLERS,
    config_patterns: CONFIG_PATTERNS,
    always_used: ALWAYS_USED,
    tooling_dependencies: TOOLING_DEPENDENCIES,
    resolve_config(config_path, source, root) {
        let mut result = PluginResult::default();

        let imports = config_parser::extract_imports(source, config_path);
        for imp in &imports {
            let dep = crate::resolve::extract_package_name(imp);
            result.referenced_dependencies.push(dep);
        }

        credit_oxlint_config(&mut result, source, config_path, root, &[]);
        result.referenced_dependencies.extend(
            extract_ultracite_selected_js_plugin_dependencies(source, config_path),
        );

        result
    }
}

/// Credit the plugin specifiers of an Oxlint config object: `jsPlugins`,
/// `overrides[].jsPlugins` and `extends`. `base` is the property path of that
/// object in the exported config. It is empty for an Oxlint config file and
/// `["lint"]` for the `lint` block of a Vite+ config.
pub(super) fn credit_oxlint_config(
    result: &mut PluginResult,
    source: &str,
    config_path: &Path,
    root: &Path,
    base: &[&str],
) {
    let (specifiers, presets) =
        config_parser::extract_from_source(source, config_path, |program| {
            let mut config = config_parser::find_config_object(program)?;
            for key in base {
                config = config_parser::property_object(config, key)?;
            }
            Some((
                oxlint_config_specifiers(config),
                imported_extends_sources(program, config),
            ))
        })
        .unwrap_or_default();
    for specifier in specifiers {
        credit_config_specifier(result, config_path, root, &specifier);
    }
    let importer = absolute_config_path(config_path, root);
    let mut visited = FxHashSet::default();
    credit_imported_presets(
        result,
        &importer,
        root,
        &presets,
        MAX_IMPORTED_PRESET_DEPTH,
        &mut visited,
    );
}

/// Credit the plugin specifiers of the presets that an `extends` array names
/// as imported values (issue #5101). Each preset is the default export of the
/// imported module. A preset that is not a static object gives no credit.
fn credit_imported_presets(
    result: &mut PluginResult,
    importer: &Path,
    root: &Path,
    sources: &[String],
    depth: usize,
    visited: &mut FxHashSet<PathBuf>,
) {
    if depth == 0 {
        return;
    }
    for source in sources {
        let Some(file) = resolve_preset_file(importer, root, source) else {
            continue;
        };
        if !visited.insert(file.clone()) {
            continue;
        }
        let Some(text) = crate::discover::project_read::read_project_file(root, &file) else {
            continue;
        };
        let Some((specifiers, nested)) =
            config_parser::extract_from_source(&text, &file, |program| {
                let config = config_parser::find_config_object(program)?;
                Some((
                    oxlint_config_specifiers(config),
                    imported_extends_sources(program, config),
                ))
            })
        else {
            continue;
        };
        // A path inside a dependency is not a file of the project.
        let in_dependency = super::project_glob::is_in_node_modules(&file, root);
        for specifier in specifiers {
            if !in_dependency || is_package_specifier(&specifier) {
                credit_config_specifier(result, &file, root, &specifier);
            }
        }
        credit_imported_presets(result, &file, root, &nested, depth - 1, visited);
    }
}

/// The import sources of the `extends` entries that are default-imported
/// bindings, such as `reactStrict` in `extends: [reactStrict]`.
fn imported_extends_sources(program: &Program<'_>, config: &ObjectExpression<'_>) -> Vec<String> {
    let Some(Expression::ArrayExpression(extends)) =
        config_parser::property_expr(config, "extends")
    else {
        return Vec::new();
    };
    extends
        .elements
        .iter()
        .filter_map(|element| match element.as_expression()? {
            Expression::Identifier(binding) => {
                default_import_source(program, binding.name.as_str())
            }
            _ => None,
        })
        .collect()
}

fn default_import_source(program: &Program<'_>, local: &str) -> Option<String> {
    program.body.iter().find_map(|statement| {
        let Statement::ImportDeclaration(declaration) = statement else {
            return None;
        };
        if declaration.import_kind.is_type() {
            return None;
        }
        let binds_local = declaration.specifiers.as_ref()?.iter().any(|specifier| {
            matches!(
                specifier,
                ImportDeclarationSpecifier::ImportDefaultSpecifier(default)
                    if default.local.name == local
            )
        });
        if !binds_local {
            return None;
        }
        declaration.source.value.as_str().map(str::to_owned)
    })
}

/// Resolve the module that an `import` in `importer` names. A package
/// resolves through `node_modules` and the `exports` map of its manifest.
fn resolve_preset_file(importer: &Path, root: &Path, source: &str) -> Option<PathBuf> {
    if is_local_specifier(source) {
        let file =
            resolve_local_module_file(root, resolve_config_relative_path(importer, root, source));
        return file.is_file().then_some(file);
    }
    if !is_package_specifier(source) {
        return None;
    }
    let package_name = crate::resolve::extract_package_name(source);
    let package_dir = super::eslint::find_package_dir(importer.parent()?, &package_name)?;
    let manifest: serde_json::Value = serde_json::from_str(
        &crate::discover::project_read::read_project_file(root, &package_dir.join("package.json"))?,
    )
    .ok()?;
    let subpath_key = format!(".{}", source.get(package_name.len()..).unwrap_or_default());
    let entry = super::eslint::resolve_package_entry(&manifest, &subpath_key, &package_dir)?;
    let file = resolve_local_module_file(
        root,
        config_parser::lexical_normalize(&package_dir.join(entry)),
    );
    file.is_file().then_some(file)
}

fn absolute_config_path(config_path: &Path, root: &Path) -> PathBuf {
    if config_path.is_absolute() {
        config_path.to_path_buf()
    } else {
        root.join(config_path)
    }
}

fn oxlint_config_specifiers(config: &ObjectExpression<'_>) -> Vec<String> {
    let js_plugins = |object: &ObjectExpression<'_>| {
        config_parser::property_expr(object, "jsPlugins").map_or_else(Vec::new, |value| {
            config_parser::collect_shallow_string_or_object_property_values(value, "specifier")
        })
    };
    let mut specifiers = js_plugins(config);
    if let Some(Expression::ArrayExpression(overrides)) =
        config_parser::property_expr(config, "overrides")
    {
        for element in &overrides.elements {
            if let Some(Expression::ObjectExpression(entry)) = element.as_expression() {
                specifiers.extend(js_plugins(entry));
            }
        }
    }
    if let Some(extends) = config_parser::property_expr(config, "extends") {
        specifiers.extend(config_parser::collect_shallow_string_values(extends));
    }
    specifiers
}

fn extract_ultracite_selected_js_plugin_dependencies(
    source: &str,
    config_path: &Path,
) -> Vec<String> {
    config_parser::extract_from_source(source, config_path, |program| {
        let helper_binding = ultracite_selector_binding(program)?;
        let selected_plugins_binding = wired_js_plugins_binding(program)?;

        for statement in &program.body {
            let Statement::VariableDeclaration(declaration) = statement else {
                continue;
            };
            if declaration.kind != VariableDeclarationKind::Const {
                continue;
            }

            for declarator in &declaration.declarations {
                let Some(binding) = declarator.id.get_binding_identifier() else {
                    continue;
                };
                if binding.name.as_str() != selected_plugins_binding {
                    continue;
                }

                let Some(Expression::CallExpression(call)) = &declarator.init else {
                    return None;
                };
                let Expression::Identifier(callee) = &call.callee else {
                    return None;
                };
                if callee.name.as_str() != helper_binding || call.arguments.len() != 1 {
                    return None;
                }
                let Some(Argument::ArrayExpression(selection)) = call.arguments.first() else {
                    return None;
                };
                if !selection
                    .elements
                    .iter()
                    .all(|element| matches!(element, ArrayExpressionElement::StringLiteral(_)))
                {
                    return None;
                }

                let dependencies = selection
                    .elements
                    .iter()
                    .filter_map(|element| {
                        let ArrayExpressionElement::StringLiteral(plugin) = element else {
                            return None;
                        };
                        ULTRACITE_JS_PLUGIN_DEPENDENCIES
                            .iter()
                            .find_map(|(name, dependency)| {
                                (plugin.value == *name).then(|| (*dependency).to_string())
                            })
                    })
                    .collect();
                return Some(dependencies);
            }
        }

        None
    })
    .unwrap_or_default()
}

fn ultracite_selector_binding(program: &Program<'_>) -> Option<String> {
    let mut binding = None;

    for statement in &program.body {
        let Statement::ImportDeclaration(declaration) = statement else {
            continue;
        };
        if declaration.source.value != ULTRACITE_JS_PLUGINS_SOURCE
            || declaration.import_kind.is_type()
        {
            continue;
        }
        let Some(specifiers) = &declaration.specifiers else {
            continue;
        };

        for specifier in specifiers {
            let ImportDeclarationSpecifier::ImportSpecifier(specifier) = specifier else {
                continue;
            };
            if specifier.import_kind.is_type() || specifier.imported.name() != "selectJsPlugins" {
                continue;
            }
            if binding.is_some() {
                return None;
            }
            binding = Some(specifier.local.name.to_string());
        }
    }

    binding
}

fn wired_js_plugins_binding(program: &Program<'_>) -> Option<String> {
    let config = config_parser::find_config_object(program)?;
    if function_scoped_config_object(program, config) {
        return None;
    }
    let Expression::StaticMemberExpression(member) =
        config_parser::property_expr(config, "jsPlugins")?
    else {
        return None;
    };
    if member.property.name != "jsPlugins" {
        return None;
    }
    let Expression::Identifier(binding) = &member.object else {
        return None;
    };
    Some(binding.name.to_string())
}

fn function_scoped_config_object(program: &Program<'_>, config: &ObjectExpression<'_>) -> bool {
    let mut detector = FunctionScopeDetector {
        target: config.span,
        found: false,
    };
    detector.visit_program(program);
    detector.found
}

struct FunctionScopeDetector {
    target: Span,
    found: bool,
}

impl<'a> Visit<'a> for FunctionScopeDetector {
    fn visit_function(&mut self, function: &Function<'a>, flags: ScopeFlags) {
        if function.span.contains_inclusive(self.target) {
            self.found = true;
            return;
        }
        walk::walk_function(self, function, flags);
    }

    fn visit_arrow_function_expression(&mut self, arrow: &ArrowFunctionExpression<'a>) {
        if arrow.span.contains_inclusive(self.target) {
            self.found = true;
            return;
        }
        walk::walk_arrow_function_expression(self, arrow);
    }
}

fn credit_config_specifier(
    result: &mut PluginResult,
    config_path: &Path,
    root: &Path,
    specifier: &str,
) {
    if is_local_specifier(specifier) {
        let path = resolve_local_module_file(
            root,
            resolve_config_relative_path(config_path, root, specifier),
        );
        // Oxlint loads the default export of a local plugin file (issue #3871).
        if let Ok(relative) = path.strip_prefix(root) {
            result.push_entry_export_credit(
                config_parser::path_to_config_string(relative),
                ["default"],
            );
        }
        result.setup_files.push(path);
    } else if is_package_specifier(specifier) {
        result
            .referenced_dependencies
            .push(crate::resolve::extract_package_name(specifier));
    }
}

fn is_local_specifier(specifier: &str) -> bool {
    specifier.starts_with("./") || specifier.starts_with("../") || specifier.starts_with('/')
}

fn is_package_specifier(specifier: &str) -> bool {
    !specifier.is_empty()
        && !is_local_specifier(specifier)
        && !specifier.contains(':')
        && !specifier.contains('\\')
}

fn resolve_config_relative_path(config_path: &Path, root: &Path, specifier: &str) -> PathBuf {
    let config_abs = absolute_config_path(config_path, root);
    config_parser::lexical_normalize(&config_abs.parent().unwrap_or(root).join(specifier))
}

/// Resolve a local plugin specifier to the file that Oxlint loads. A
/// specifier can name the file without its extension, or a directory. A
/// directory resolves through its `package.json` `main` field, then through
/// an `index` file (issue #3961). A `main` field that points at build output
/// maps back to its source file, the same as a package entry (issue #4034).
/// An unresolved path stays as written.
fn resolve_local_module_file(root: &Path, path: PathBuf) -> PathBuf {
    if path.is_file() {
        return path;
    }
    if let Some(file) = with_module_extension(&path) {
        return file;
    }
    if !path.is_dir() {
        return path;
    }
    let main_file = package_main(root, &path).and_then(|main| {
        if let Some(source) = crate::discover::output_entry_source_file(&path, &main) {
            return Some(source);
        }
        let main_path = config_parser::lexical_normalize(&path.join(main));
        if main_path.is_file() {
            return Some(main_path);
        }
        with_module_extension(&main_path).or_else(|| index_file(&main_path))
    });
    main_file.or_else(|| index_file(&path)).unwrap_or(path)
}

fn package_main(root: &Path, dir: &Path) -> Option<String> {
    let source = crate::discover::project_read::read_project_file(root, &dir.join("package.json"))?;
    let manifest: serde_json::Value = serde_json::from_str(&source).ok()?;
    manifest
        .get("main")
        .and_then(serde_json::Value::as_str)
        .filter(|main| !main.is_empty())
        .map(str::to_owned)
}

fn with_module_extension(path: &Path) -> Option<PathBuf> {
    let file_name = path.file_name()?.to_str()?;
    MODULE_EXTENSIONS
        .iter()
        .map(|extension| path.with_file_name(format!("{file_name}.{extension}")))
        .find(|candidate| candidate.is_file())
}

fn index_file(dir: &Path) -> Option<PathBuf> {
    MODULE_EXTENSIONS
        .iter()
        .map(|extension| dir.join(format!("index.{extension}")))
        .find(|candidate| candidate.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A local preset and a local plugin manifest that link out of the
    /// project are not read.
    #[test]
    #[cfg(unix)]
    fn presets_that_link_out_of_the_project_are_not_read() {
        let dir = tempfile::tempdir().expect("temp dir");
        let root = dir.path().join("project");
        let outside = dir.path().join("elsewhere");
        std::fs::create_dir_all(&root).expect("create root");
        std::fs::create_dir_all(&outside).expect("create outside");
        std::fs::write(
            outside.join("preset.js"),
            "export default { jsPlugins: [\"plugin-outside\"] };\n",
        )
        .expect("write");
        std::fs::write(
            outside.join("package.json"),
            r#"{ "name": "plugin", "main": "lib/plugin.js" }"#,
        )
        .expect("write");
        std::os::unix::fs::symlink(outside.join("preset.js"), root.join("preset.js"))
            .expect("link");
        std::fs::create_dir_all(root.join("plugin/lib")).expect("create plugin");
        std::fs::write(root.join("plugin/lib/plugin.js"), "export default {};\n").expect("write");
        std::fs::write(root.join("plugin/index.js"), "export default {};\n").expect("write");
        std::os::unix::fs::symlink(
            outside.join("package.json"),
            root.join("plugin/package.json"),
        )
        .expect("link");

        let result = resolve_project_config(
            &root,
            r#"
                import preset from "./preset.js";
                export default { extends: [preset] };
            "#,
        );
        assert!(
            !result
                .referenced_dependencies
                .contains(&"plugin-outside".to_string())
        );
        assert_eq!(
            resolve_local_module_file(&root, root.join("plugin")),
            root.join("plugin/index.js")
        );
    }

    fn write_file(path: &Path, contents: &str) {
        std::fs::create_dir_all(path.parent().expect("file has a parent")).expect("create dir");
        std::fs::write(path, contents).expect("write file");
    }

    #[test]
    fn directory_main_in_unbuilt_output_dir_resolves_to_source() {
        // Issue #4034: in a fresh clone the build output does not exist.
        let dir = tempfile::tempdir().expect("tempdir");
        let plugin = dir.path().join("plugin");
        write_file(
            &plugin.join("package.json"),
            r#"{ "name": "plugin", "main": "./dist/index.js" }"#,
        );
        write_file(&plugin.join("src/index.ts"), "export default {};\n");

        let resolved = resolve_local_module_file(dir.path(), plugin.clone());
        assert_eq!(resolved, plugin.join("src/index.ts"));
    }

    #[test]
    fn directory_main_outside_output_dir_stays_the_main_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let plugin = dir.path().join("plugin");
        write_file(
            &plugin.join("package.json"),
            r#"{ "name": "plugin", "main": "lib/plugin.js" }"#,
        );
        write_file(&plugin.join("lib/plugin.js"), "export default {};\n");
        write_file(&plugin.join("src/index.ts"), "export default {};\n");

        let resolved = resolve_local_module_file(dir.path(), plugin.clone());
        assert_eq!(resolved, plugin.join("lib/plugin.js"));
    }

    #[test]
    fn resolve_config_js_plugins() {
        let source = r#"
            {
            "plugins": ["typescript", "vitest", "unicorn", "import", "promise", "node"],
            "jsPlugins": [
                "eslint-plugin-testing-library",
                "eslint-plugin-playwright",
                "eslint-plugin-sonarjs"
            ]
            }
        "#;
        let plugin = OxlintPlugin;
        let result =
            plugin.resolve_config(Path::new(".oxlintrc.json"), source, Path::new("/project"));

        let deps = &result.referenced_dependencies;
        assert!(deps.contains(&"eslint-plugin-testing-library".to_string()));
        assert!(deps.contains(&"eslint-plugin-playwright".to_string()));
        assert!(deps.contains(&"eslint-plugin-sonarjs".to_string()));
        assert!(!deps.contains(&"typescript".to_string()));
        assert!(!deps.contains(&"vitest".to_string()));
        assert!(!deps.contains(&"unicorn".to_string()));
    }

    #[test]
    fn resolve_config_js_plugins_object_aliases() {
        let source = r#"
            {
                "jsPlugins": [
                    { "name": "testing", "specifier": "eslint-plugin-testing-library" },
                    { "name": "playwright", "specifier": "eslint-plugin-playwright" }
                ]
            }
        "#;
        let plugin = OxlintPlugin;
        let result =
            plugin.resolve_config(Path::new(".oxlintrc.json"), source, Path::new("/project"));

        let deps = &result.referenced_dependencies;
        assert!(deps.contains(&"eslint-plugin-testing-library".to_string()));
        assert!(deps.contains(&"eslint-plugin-playwright".to_string()));
        assert!(!deps.contains(&"testing".to_string()));
        assert!(!deps.contains(&"playwright".to_string()));
    }

    #[test]
    fn resolve_config_js_plugins_oxlint_json() {
        let source = r#"
            {
                "jsPlugins": ["eslint-plugin-testing-library", "eslint-plugin-playwright"]
            }
        "#;
        let plugin = OxlintPlugin;

        let result = plugin.resolve_config(Path::new("oxlint.json"), source, Path::new("/project"));

        let deps = &result.referenced_dependencies;
        assert!(deps.contains(&"eslint-plugin-testing-library".to_string()));
        assert!(deps.contains(&"eslint-plugin-playwright".to_string()));
    }

    #[test]
    fn resolve_config_js_plugins_oxlint_ts_config() {
        let source = r#"
            import { defineConfig } from "oxlint";

            export default defineConfig({
                jsPlugins: ["eslint-plugin-testing-library", "eslint-plugin-playwright"]
            });
        "#;
        let plugin = OxlintPlugin;
        let result =
            plugin.resolve_config(Path::new("oxlint.config.ts"), source, Path::new("/project"));

        let deps = &result.referenced_dependencies;
        assert!(deps.contains(&"eslint-plugin-testing-library".to_string()));
        assert!(deps.contains(&"eslint-plugin-playwright".to_string()));
    }

    #[test]
    fn resolve_config_override_js_plugins_json() {
        let source = r#"
            {
                "overrides": [
                    { "files": ["*.ts"], "jsPlugins": ["eslint-plugin-foo"] },
                    {
                        "files": ["*.test.ts"],
                        "jsPlugins": [
                            { "name": "bar", "specifier": "eslint-plugin-bar" },
                            "./plugins/local.js"
                        ]
                    },
                    { "files": ["*.js"], "rules": {} }
                ]
            }
        "#;
        let plugin = OxlintPlugin;
        let result = plugin.resolve_config(
            Path::new("/project/.oxlintrc.json"),
            source,
            Path::new("/project"),
        );

        let deps = &result.referenced_dependencies;
        assert!(deps.contains(&"eslint-plugin-foo".to_string()));
        assert!(deps.contains(&"eslint-plugin-bar".to_string()));
        assert!(!deps.contains(&"bar".to_string()));
        assert!(
            result
                .setup_files
                .contains(&PathBuf::from("/project/plugins/local.js"))
        );
    }

    #[test]
    fn resolve_config_override_js_plugins_ts_config() {
        let source = r#"
            import { defineConfig } from "oxlint";

            export default defineConfig({
                overrides: [
                    { files: ["*.ts"], jsPlugins: ["eslint-plugin-foo"] },
                ],
            });
        "#;
        let plugin = OxlintPlugin;
        let result =
            plugin.resolve_config(Path::new("oxlint.config.ts"), source, Path::new("/project"));

        assert!(
            result
                .referenced_dependencies
                .contains(&"eslint-plugin-foo".to_string())
        );
    }

    #[test]
    fn resolve_config_ultracite_selected_js_plugins() {
        let source = r#"
            import { defineConfig } from "oxlint";
            import core from "ultracite/oxlint/core";
            import { jsPluginSettings, selectJsPlugins } from "ultracite/oxlint/js-plugins";

            const jsPlugins = selectJsPlugins(["github", "sonarjs", "react-doctor"]);

            export default defineConfig({
                extends: [core, jsPlugins],
                ignorePatterns: core.ignorePatterns,
                jsPlugins: jsPlugins.jsPlugins,
                settings: jsPluginSettings,
            });
        "#;
        let plugin = OxlintPlugin;
        let result =
            plugin.resolve_config(Path::new("oxlint.config.ts"), source, Path::new("/project"));

        let deps = &result.referenced_dependencies;
        assert!(deps.contains(&"eslint-plugin-github".to_string()));
        assert!(deps.contains(&"eslint-plugin-sonarjs".to_string()));
        assert!(deps.contains(&"oxlint-plugin-react-doctor".to_string()));
    }

    #[test]
    fn resolve_config_ignores_unwired_and_unrelated_plugin_selectors() {
        let source = r#"
            import { defineConfig } from "oxlint";
            import { selectJsPlugins } from "ultracite/oxlint/js-plugins";

            const unusedSelection = selectJsPlugins(["github"]);
            const jsPlugins = chooseJsPlugins(["sonarjs"]);

            export default defineConfig({
                jsPlugins: jsPlugins.jsPlugins,
            });
        "#;
        let plugin = OxlintPlugin;
        let result =
            plugin.resolve_config(Path::new("oxlint.config.ts"), source, Path::new("/project"));

        let deps = &result.referenced_dependencies;
        assert!(!deps.contains(&"eslint-plugin-github".to_string()));
        assert!(!deps.contains(&"eslint-plugin-sonarjs".to_string()));
    }

    #[test]
    fn resolve_config_requires_ultracite_selector_provenance() {
        let source = r#"
            import { defineConfig } from "oxlint";
            import { selectJsPlugins } from "another-package";

            const jsPlugins = selectJsPlugins(["github"]);

            export default defineConfig({
                jsPlugins: jsPlugins.jsPlugins,
            });
        "#;
        let plugin = OxlintPlugin;
        let result =
            plugin.resolve_config(Path::new("oxlint.config.ts"), source, Path::new("/project"));

        assert!(
            !result
                .referenced_dependencies
                .contains(&"eslint-plugin-github".to_string())
        );
    }

    #[test]
    fn resolve_config_ignores_dynamic_ultracite_plugin_selections() {
        let source = r#"
            import { defineConfig } from "oxlint";
            import { selectJsPlugins } from "ultracite/oxlint/js-plugins";

            const extraPlugins = ["sonarjs"];
            const jsPlugins = selectJsPlugins([
                "github",
                "anti-slop",
                "unknown-plugin",
                configuredPlugin,
                ...extraPlugins,
            ]);

            export default defineConfig({
                jsPlugins: jsPlugins.jsPlugins,
            });
        "#;
        let plugin = OxlintPlugin;
        let result =
            plugin.resolve_config(Path::new("oxlint.config.ts"), source, Path::new("/project"));

        let deps = &result.referenced_dependencies;
        assert!(!deps.contains(&"eslint-plugin-github".to_string()));
        assert!(!deps.contains(&"eslint-plugin-sonarjs".to_string()));
        assert!(!deps.contains(&"anti-slop".to_string()));
        assert!(!deps.contains(&"unknown-plugin".to_string()));
    }

    #[test]
    fn resolve_config_supports_an_aliased_ultracite_selector_import() {
        let source = r#"
            import { defineConfig } from "oxlint";
            import { selectJsPlugins as select } from "ultracite/oxlint/js-plugins";

            const selected = select(["github", "anti-slop", "unknown-plugin"]);

            export default defineConfig({
                jsPlugins: selected.jsPlugins,
            });
        "#;
        let plugin = OxlintPlugin;
        let result =
            plugin.resolve_config(Path::new("oxlint.config.ts"), source, Path::new("/project"));

        let deps = &result.referenced_dependencies;
        assert!(deps.contains(&"eslint-plugin-github".to_string()));
        assert!(!deps.contains(&"anti-slop".to_string()));
        assert!(!deps.contains(&"unknown-plugin".to_string()));
    }

    #[test]
    fn resolve_config_requires_const_and_exact_member_wiring() {
        for source in [
            r#"
                import { defineConfig } from "oxlint";
                import { selectJsPlugins } from "ultracite/oxlint/js-plugins";
                let selected = selectJsPlugins(["github"]);
                export default defineConfig({ jsPlugins: selected.jsPlugins });
            "#,
            r#"
                import { defineConfig } from "oxlint";
                import { selectJsPlugins } from "ultracite/oxlint/js-plugins";
                const selected = selectJsPlugins(["github"]);
                export default defineConfig({ jsPlugins: selected.plugins });
            "#,
        ] {
            let plugin = OxlintPlugin;
            let result =
                plugin.resolve_config(Path::new("oxlint.config.ts"), source, Path::new("/project"));

            assert!(
                !result
                    .referenced_dependencies
                    .contains(&"eslint-plugin-github".to_string())
            );
        }
    }

    #[test]
    fn resolve_config_rejects_function_scoped_shadowing() {
        for source in [
            r#"
                import { defineConfig } from "oxlint";
                import { selectJsPlugins } from "ultracite/oxlint/js-plugins";
                const selected = selectJsPlugins(["github"]);
                export default defineConfig((selected) => ({
                    jsPlugins: selected.jsPlugins,
                }));
            "#,
            r#"
                import { defineConfig } from "oxlint";
                import { selectJsPlugins } from "ultracite/oxlint/js-plugins";
                const selected = selectJsPlugins(["github"]);
                export default defineConfig(function (selected) {
                    return { jsPlugins: selected.jsPlugins };
                });
            "#,
        ] {
            let plugin = OxlintPlugin;
            let result =
                plugin.resolve_config(Path::new("oxlint.config.ts"), source, Path::new("/project"));

            assert!(
                !result
                    .referenced_dependencies
                    .contains(&"eslint-plugin-github".to_string())
            );
        }
    }

    #[test]
    fn resolve_config_js_plugins_tuple_with_options() {
        let source = r#"
            {
                "jsPlugins": [
                    "eslint-plugin-testing-library",
                    ["eslint-plugin-playwright", { "rules": {} }]
                ]
            }
        "#;
        let plugin = OxlintPlugin;
        let result =
            plugin.resolve_config(Path::new(".oxlintrc.json"), source, Path::new("/project"));

        let deps = &result.referenced_dependencies;
        assert!(deps.contains(&"eslint-plugin-testing-library".to_string()));
        assert!(deps.contains(&"eslint-plugin-playwright".to_string()));
    }

    #[test]
    fn resolve_config_js_plugins_local_paths_are_setup_files() {
        let source = r#"
            {
                "jsPlugins": [
                    "./plugins/local.js",
                    "../shared/other-plugin.js",
                    "eslint-plugin-playwright"
                ]
            }
        "#;
        let plugin = OxlintPlugin;
        let result = plugin.resolve_config(
            Path::new("/project/config/.oxlintrc.json"),
            source,
            Path::new("/project"),
        );

        assert!(
            result
                .setup_files
                .contains(&PathBuf::from("/project/config/plugins/local.js"))
        );
        assert!(
            result
                .setup_files
                .contains(&PathBuf::from("/project/shared/other-plugin.js"))
        );
        assert!(
            result
                .referenced_dependencies
                .contains(&"eslint-plugin-playwright".to_string())
        );
        assert!(!result.referenced_dependencies.contains(&".".to_string()));
    }

    #[test]
    fn resolve_config_empty() {
        let source = r#"{ "options": { "typeAware": true } }"#;
        let plugin = OxlintPlugin;
        let result =
            plugin.resolve_config(Path::new(".oxlintrc.json"), source, Path::new("/project"));

        assert!(result.referenced_dependencies.is_empty());
    }

    #[test]
    fn tooling_dependencies_include_cli_tooling_packages() {
        let plugin = OxlintPlugin;
        let tooling = plugin.tooling_dependencies();
        assert!(tooling.contains(&"oxlint"));
        assert!(tooling.contains(&"oxlint-tsgolint"));
    }

    #[test]
    fn resolve_config_no_js_plugins() {
        let source = r#"
            {
                "plugins": ["typescript", "import"],
                "rules": { "no-console": "warn" }
            }
        "#;
        let plugin = OxlintPlugin;
        let result =
            plugin.resolve_config(Path::new(".oxlintrc.json"), source, Path::new("/project"));

        assert!(result.referenced_dependencies.is_empty());
    }

    fn resolve_project_config(root: &Path, source: &str) -> PluginResult {
        let config_path = root.join("oxlint.config.ts");
        write_file(&config_path, source);
        OxlintPlugin.resolve_config(&config_path, source, root)
    }

    #[test]
    fn resolve_config_credits_js_plugins_of_an_imported_package_preset() {
        // Issue #5101: the preset is an imported value under node_modules.
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        let preset = root.join("node_modules/lint-preset");
        write_file(
            &preset.join("package.json"),
            r#"{"name":"lint-preset","type":"module","exports":{"./react-strict":"./react-strict.js"}}"#,
        );
        write_file(
            &preset.join("react-strict.js"),
            r#"export default { jsPlugins: ["oxlint-plugin-react-doctor"] }"#,
        );

        let result = resolve_project_config(
            root,
            r#"
                import { defineConfig } from "oxlint";
                import reactStrict from "lint-preset/react-strict";
                export default defineConfig({ extends: [reactStrict] });
            "#,
        );

        assert!(
            result
                .referenced_dependencies
                .contains(&"oxlint-plugin-react-doctor".to_string())
        );
    }

    #[test]
    fn resolve_config_follows_nested_imported_presets_up_to_the_depth_limit() {
        // Each preset `level-N` extends `level-(N+1)` and names `plugin-N`.
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        let levels = MAX_IMPORTED_PRESET_DEPTH + 2;
        for level in 1..=levels {
            let preset = root.join(format!("node_modules/level-{level}"));
            write_file(
                &preset.join("package.json"),
                &format!(r#"{{"name":"level-{level}","main":"index.js"}}"#),
            );
            let next = level + 1;
            let body = if level < levels {
                format!(
                    "import next from \"level-{next}\";\n\
                     export default {{ extends: [next], jsPlugins: [\"plugin-{level}\"] }};\n"
                )
            } else {
                format!("export default {{ jsPlugins: [\"plugin-{level}\"] }};\n")
            };
            write_file(&preset.join("index.js"), &body);
        }

        let result = resolve_project_config(
            root,
            r#"
                import level from "level-1";
                export default { extends: [level] };
            "#,
        );

        let deps = &result.referenced_dependencies;
        for level in 1..=MAX_IMPORTED_PRESET_DEPTH {
            assert!(deps.contains(&format!("plugin-{level}")), "plugin-{level}");
        }
        for level in MAX_IMPORTED_PRESET_DEPTH + 1..=levels {
            assert!(!deps.contains(&format!("plugin-{level}")), "plugin-{level}");
        }
    }

    #[test]
    fn resolve_config_ignores_imported_presets_that_are_not_static() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        write_file(
            &root.join("presets/dynamic.js"),
            "export default makePreset([\"plugin-dynamic\"]);\n",
        );
        write_file(
            &root.join("presets/cycle.js"),
            "import self from \"./cycle.js\";\n\
             export default { extends: [self], jsPlugins: [\"plugin-cycle\"] };\n",
        );

        let result = resolve_project_config(
            root,
            r#"
                import dynamic from "./presets/dynamic.js";
                import cycle from "./presets/cycle.js";
                import missing from "missing-preset";
                import type typed from "./presets/cycle.js";
                export default { extends: [dynamic, cycle, missing, typed, unknown] };
            "#,
        );

        let deps = &result.referenced_dependencies;
        assert!(!deps.contains(&"plugin-dynamic".to_string()));
        assert!(deps.contains(&"plugin-cycle".to_string()));
    }

    #[test]
    fn resolve_config_extends_credits_package_and_records_local_path() {
        let source = r#"
            {
                "extends": ["@nkzw/oxlint-config", "./local.json"]
            }
        "#;
        let plugin = OxlintPlugin;
        let result = plugin.resolve_config(
            Path::new("/project/.oxlintrc.json"),
            source,
            Path::new("/project"),
        );

        assert!(
            result
                .referenced_dependencies
                .contains(&"@nkzw/oxlint-config".to_string()),
            "expected @nkzw/oxlint-config in referenced_dependencies"
        );
        assert!(
            result
                .setup_files
                .contains(&PathBuf::from("/project/local.json")),
            "expected /project/local.json in setup_files"
        );
        assert!(
            !result
                .referenced_dependencies
                .contains(&"./local.json".to_string()),
            "local path must not appear in referenced_dependencies"
        );
    }
}
