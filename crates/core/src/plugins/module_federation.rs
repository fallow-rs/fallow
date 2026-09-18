//! Module Federation plugin and the shared `exposes` / `remotes` reader.
//!
//! Federation options reach a build in two shapes. A standalone
//! `module-federation.config.*` file default-exports the options object and is
//! owned by this plugin. The same options also appear inline in a bundler
//! config's `plugins` array, which the webpack, rspack, rsbuild and vite
//! plugins read through [`apply_bundler_plugin_options`].
//!
//! Reading is syntactic. `exposes` targets become entry-point globs so an
//! exposed module is not mistaken for dead code, and `remotes` aliases become
//! runtime-provided specifiers so an import of a remote container is not
//! mistaken for an unlisted npm dependency. No remote container is fetched and
//! no cross-deployment reachability is inferred.

use std::path::Path;

use oxc_ast::ast::{
    Argument, ArrayExpression, Expression, ObjectExpression, ObjectPropertyKind, PropertyKey,
};

use super::config_parser;
use super::{Plugin, PluginResult, ProvidedDependencyRule};

const ENABLERS: &[&str] = &[
    "@module-federation/enhanced",
    "@module-federation/modern-js",
    "@module-federation/nextjs-mf",
    "@module-federation/node",
    "@module-federation/rsbuild-plugin",
    "@module-federation/rspack",
    "@module-federation/runtime",
    "@module-federation/vite",
    "@module-federation/webpack-bundler-runtime",
    "@originjs/vite-plugin-federation",
];

const CONFIG_PATTERNS: &[&str] = &["module-federation.config.{ts,js,mjs,cjs,mts,cts}"];

const ALWAYS_USED: &[&str] = CONFIG_PATTERNS;

/// Callee names that receive Module Federation options inline in a bundler
/// config: `ModuleFederationPlugin` for webpack and rspack,
/// `pluginModuleFederation` for rsbuild, `federation` for vite.
const FEDERATION_CALLEES: &[&str] = &[
    "ModuleFederationPlugin",
    "moduleFederationPlugin",
    "pluginModuleFederation",
    "federation",
];

/// Extensions that make an `exposes` target name a file rather than a module
/// request or an extensionless path.
const SOURCE_EXTENSIONS: &[&str] = &[
    "ts", "tsx", "mts", "cts", "js", "jsx", "mjs", "cjs", "json", "vue", "svelte", "astro", "css",
    "scss", "sass", "less", "styl",
];

/// Brace list appended to an extensionless `exposes` target. Entry patterns are
/// plain globs with no extension expansion, so a bare `src/Button` would match
/// no file.
const EXPOSE_EXTENSIONS: &str = "{ts,tsx,mts,cts,js,jsx,mjs,cjs,vue,svelte}";

/// Glob suffix that covers every file under the directory that declared the
/// remote.
const SCOPE_SUFFIX: &str = "**/*";

/// What one Federation options object statically declares.
#[derive(Debug, Default, PartialEq, Eq)]
struct FederationConfig {
    /// Local module targets from `exposes`, as written in the config.
    pub exposed_targets: Vec<String>,
    /// Package names of bare module requests named as `exposes` targets.
    pub exposed_packages: Vec<String>,
    /// Declared `remotes` alias names, in source order.
    pub remote_aliases: Vec<String>,
}

/// Where to look for Federation options in one config file.
struct FederationSites<'a> {
    /// Config-object paths that may hold a `plugins` array.
    pub plugin_arrays: &'a [&'a [&'a str]],
    /// Read `exposes` / `remotes` off the config object itself, as the
    /// standalone `module-federation.config.*` file declares them.
    pub read_config_object: bool,
}

/// Where a config file sits, which decides how its declarations are anchored.
struct ConfigLocation<'a> {
    pub config_path: &'a Path,
    pub root: &'a Path,
    /// Project-relative base directory that replaces the config directory when
    /// resolving a relative `exposes` target, as webpack's `context` does.
    pub context: Option<&'a Path>,
}

impl ConfigLocation<'_> {
    /// The directory a relative `exposes` target resolves against, expressed as
    /// a stand-in config path so the shared path normalization applies.
    fn target_base(&self) -> std::borrow::Cow<'_, Path> {
        match self.context {
            Some(context) => {
                std::borrow::Cow::Owned(self.root.join(context).join("module-federation"))
            }
            None => std::borrow::Cow::Borrowed(self.config_path),
        }
    }

    /// The config file's path relative to the project root.
    ///
    /// `normalize_config_path` reads a leading `/` as project-root-relative, the
    /// convention config values use, so it cannot relativize a filesystem path.
    fn relative_config_path(&self) -> Option<String> {
        if let Ok(relative) = self.config_path.strip_prefix(self.root) {
            return Some(config_parser::path_to_config_string(relative));
        }
        (!self.config_path.is_absolute())
            .then(|| config_parser::path_to_config_string(self.config_path))
    }

    /// Project-relative path of the config file, for messages.
    fn label(&self) -> String {
        self.relative_config_path()
            .unwrap_or_else(|| self.config_path.display().to_string())
    }

    /// Glob covering the directory that declared the remote.
    ///
    /// A config file governs the tree it sits in, so a config inside a
    /// workspace package cannot silence a finding in a sibling package. A root
    /// config governs the whole project.
    fn scope_pattern(&self) -> String {
        let directory = self
            .relative_config_path()
            .and_then(|relative| {
                Path::new(&relative)
                    .parent()
                    .map(config_parser::path_to_config_string)
            })
            .filter(|directory| !directory.is_empty());
        match directory {
            Some(directory) => format!("{directory}/{SCOPE_SUFFIX}"),
            None => SCOPE_SUFFIX.to_string(),
        }
    }
}

/// A Federation key that is present but not statically readable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ComputedKey {
    Exposes,
    Remotes,
}

impl ComputedKey {
    const fn name(self) -> &'static str {
        match self {
            Self::Exposes => "exposes",
            Self::Remotes => "remotes",
        }
    }

    const fn consequence(self) -> &'static str {
        match self {
            Self::Exposes => "its targets are not registered as entry points",
            Self::Remotes => "its aliases are not treated as provided by a remote container",
        }
    }

    const fn advice(self) -> &'static str {
        match self {
            Self::Exposes => {
                "Name the exposed files in `entryPoints`, or keep the object literal and compute \
                 only its values."
            }
            Self::Remotes => {
                "Name the aliases in `ignoreDependencies`, or keep the object literal and compute \
                 only its values."
            }
        }
    }
}

/// Read every statically available Federation options object in `source`,
/// warning once per config file for an `exposes` or `remotes` key that is
/// present but not a static object literal.
fn extract(
    source: &str,
    location: &ConfigLocation<'_>,
    plugin_label: &str,
    sites: &FederationSites<'_>,
) -> FederationConfig {
    let (config, computed) = read(source, location.config_path, sites);
    if !computed.is_empty() {
        let config_label = location.label();
        for key in computed {
            tracing::warn!(
                "Plugin '{plugin_label}': `{}` in '{config_label}' is not a static object \
                 literal, so {}. {}",
                key.name(),
                key.consequence(),
                key.advice(),
            );
        }
    }
    config
}

fn apply_from_source(
    result: &mut PluginResult,
    source: &str,
    location: &ConfigLocation<'_>,
    plugin_label: &str,
    sites: &FederationSites<'_>,
) {
    let config = extract(source, location, plugin_label, sites);
    apply(result, &config, location);
}

/// Read inline Federation options from a bundler config's top-level `plugins`
/// array and register what they declare.
///
/// `context` is a project-relative base directory that replaces the config
/// directory when resolving a relative `exposes` target, as webpack's `context`
/// option does.
pub(super) fn apply_bundler_plugin_options(
    result: &mut PluginResult,
    source: &str,
    config_path: &Path,
    root: &Path,
    context: Option<&Path>,
    plugin_label: &str,
) {
    apply_from_source(
        result,
        source,
        &ConfigLocation {
            config_path,
            root,
            context,
        },
        plugin_label,
        &FederationSites {
            plugin_arrays: &[&["plugins"]],
            read_config_object: false,
        },
    );
}

/// Register what a Federation options object declares: exposed targets as
/// entry-point globs, exposed module requests as referenced dependencies, and
/// remote aliases as runtime-provided specifiers scoped to the declaring
/// directory.
fn apply(result: &mut PluginResult, config: &FederationConfig, location: &ConfigLocation<'_>) {
    let base = location.target_base();
    for target in &config.exposed_targets {
        push_exposed_entry_patterns(result, target, &base, location.root);
    }
    result
        .referenced_dependencies
        .extend(config.exposed_packages.iter().cloned());
    if config.remote_aliases.is_empty() {
        return;
    }
    let scope = location.scope_pattern();
    for alias in &config.remote_aliases {
        result
            .provided_dependencies
            .push(ProvidedDependencyRule::new(
                scope.clone(),
                [alias.clone()],
                [format!("{alias}/")],
            ));
    }
}

fn push_exposed_entry_patterns(result: &mut PluginResult, target: &str, base: &Path, root: &Path) {
    let trimmed = target.trim();
    if trimmed.is_empty() || trimmed.contains(':') {
        return;
    }
    let Some(normalized) = config_parser::normalize_config_path(trimmed, base, root) else {
        return;
    };
    if has_source_extension(&normalized) {
        result.push_entry_pattern(normalized);
        return;
    }
    result.push_entry_pattern(format!("{normalized}.{EXPOSE_EXTENSIONS}"));
    result.push_entry_pattern(format!("{normalized}/index.{EXPOSE_EXTENSIONS}"));
}

fn has_source_extension(target: &str) -> bool {
    Path::new(target)
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| SOURCE_EXTENSIONS.contains(&ext.to_ascii_lowercase().as_str()))
}

fn read(
    source: &str,
    config_path: &Path,
    sites: &FederationSites<'_>,
) -> (FederationConfig, Vec<ComputedKey>) {
    config_parser::extract_from_source(source, config_path, |program| {
        let config_object = config_parser::find_config_object(program)?;
        let mut config = FederationConfig::default();
        let mut computed = Vec::new();

        if sites.read_config_object {
            read_options(config_object, &mut config, &mut computed);
        }

        for path in sites.plugin_arrays {
            let Some(plugins) = nested_array(config_object, path) else {
                continue;
            };
            for element in &plugins.elements {
                if let Some(expr) = element.as_expression()
                    && let Some(options) = federation_options(expr)
                {
                    read_options(options, &mut config, &mut computed);
                }
            }
        }

        Some((config, computed))
    })
    .unwrap_or_default()
}

fn read_options(
    options: &ObjectExpression<'_>,
    config: &mut FederationConfig,
    computed: &mut Vec<ComputedKey>,
) {
    read_exposes(options, config, computed);
    read_remotes(options, config, computed);
}

fn read_exposes(
    options: &ObjectExpression<'_>,
    config: &mut FederationConfig,
    computed: &mut Vec<ComputedKey>,
) {
    let Some(mapping) = federation_key_object(options, ComputedKey::Exposes, computed) else {
        return;
    };
    for property in &mapping.properties {
        let ObjectPropertyKind::ObjectProperty(property) = property else {
            continue;
        };
        for target in exposed_target_strings(&property.value) {
            classify_exposed_target(&target, config);
        }
    }
}

/// Read the target strings of one `exposes` entry, accepting a string, a
/// template literal, an array, and the entry descriptor `{ import: ... }`.
fn exposed_target_strings(value: &Expression<'_>) -> Vec<String> {
    match config_parser::object_expression(value) {
        Some(descriptor) => config_parser::property_expr(descriptor, "import")
            .map(config_parser::expression_to_string_or_array)
            .unwrap_or_default(),
        None => config_parser::expression_to_string_or_array(value),
    }
}

/// Split an exposed target into a local path and a bare module request.
///
/// A target without a leading `./` and without a source extension is a module
/// request, which is how a bundler resolves it: an entry glob would match no
/// file, while the package still needs dependency credit.
fn classify_exposed_target(target: &str, config: &mut FederationConfig) {
    let trimmed = target.trim();
    if trimmed.is_empty() {
        return;
    }
    if config_parser::is_package_specifier(trimmed) && !has_source_extension(trimmed) {
        push_unique(
            &mut config.exposed_packages,
            crate::resolve::extract_package_name(trimmed),
        );
        return;
    }
    push_unique(&mut config.exposed_targets, trimmed.to_string());
}

fn read_remotes(
    options: &ObjectExpression<'_>,
    config: &mut FederationConfig,
    computed: &mut Vec<ComputedKey>,
) {
    let Some(mapping) = federation_key_object(options, ComputedKey::Remotes, computed) else {
        return;
    };
    for property in &mapping.properties {
        let ObjectPropertyKind::ObjectProperty(property) = property else {
            continue;
        };
        if let Some(alias) = property_key_name(&property.key)
            && is_remote_alias(&alias)
        {
            push_unique(&mut config.remote_aliases, alias);
        }
    }
}

/// Resolve one Federation key to its object literal, recording the key as
/// computed when it is present but not statically readable.
fn federation_key_object<'a>(
    options: &'a ObjectExpression<'a>,
    key: ComputedKey,
    computed: &mut Vec<ComputedKey>,
) -> Option<&'a ObjectExpression<'a>> {
    let value = config_parser::property_expr(options, key.name())?;
    let Some(mapping) = config_parser::object_expression(value) else {
        push_unique(computed, key);
        return None;
    };
    if mapping
        .properties
        .iter()
        .any(|property| matches!(property, ObjectPropertyKind::SpreadProperty(_)))
    {
        push_unique(computed, key);
    }
    Some(mapping)
}

/// Whether the options object declares a Federation key. The shape gate keeps a
/// same-named local symbol from activating extraction, which matters because a
/// webpack config commonly binds the plugin through `require` rather than an
/// import declaration.
fn declares_federation_keys(options: &ObjectExpression<'_>) -> bool {
    config_parser::property_expr(options, "exposes").is_some()
        || config_parser::property_expr(options, "remotes").is_some()
}

fn federation_options<'a>(expr: &'a Expression<'a>) -> Option<&'a ObjectExpression<'a>> {
    let (callee, arguments) = match unwrap_expression(expr) {
        Expression::NewExpression(new_expr) => (&new_expr.callee, &new_expr.arguments),
        Expression::CallExpression(call) => (&call.callee, &call.arguments),
        _ => return None,
    };
    if !is_federation_callee(callee) {
        return None;
    }
    let options = arguments
        .first()
        .and_then(Argument::as_expression)
        .and_then(config_parser::object_expression)?;
    declares_federation_keys(options).then_some(options)
}

fn is_federation_callee(callee: &Expression<'_>) -> bool {
    let name = match unwrap_expression(callee) {
        Expression::Identifier(identifier) => identifier.name.as_str(),
        Expression::StaticMemberExpression(member) => member.property.name.as_str(),
        _ => return false,
    };
    FEDERATION_CALLEES.contains(&name)
}

fn unwrap_expression<'a>(expr: &'a Expression<'a>) -> &'a Expression<'a> {
    match expr {
        Expression::ParenthesizedExpression(paren) => unwrap_expression(&paren.expression),
        Expression::TSAsExpression(ts_as) => unwrap_expression(&ts_as.expression),
        Expression::TSSatisfiesExpression(ts_satisfies) => {
            unwrap_expression(&ts_satisfies.expression)
        }
        _ => expr,
    }
}

fn nested_array<'a>(
    obj: &'a ObjectExpression<'a>,
    path: &[&str],
) -> Option<&'a ArrayExpression<'a>> {
    let (last, parents) = path.split_last()?;
    let mut current = obj;
    for key in parents {
        current = config_parser::property_object(current, key)?;
    }
    config_parser::property_expr(current, last).and_then(config_parser::array_expression)
}

fn property_key_name(key: &PropertyKey<'_>) -> Option<String> {
    match key {
        PropertyKey::StaticIdentifier(identifier) => Some(identifier.name.to_string()),
        PropertyKey::StringLiteral(literal) => Some(literal.value.to_string()),
        _ => None,
    }
}

/// Whether an alias can be imported as a bare specifier, which is the only form
/// a provider rule can cover.
fn is_remote_alias(alias: &str) -> bool {
    config_parser::is_package_specifier(alias) && !alias.starts_with('.')
}

fn push_unique<T: PartialEq>(values: &mut Vec<T>, value: T) {
    if !values.contains(&value) {
        values.push(value);
    }
}

define_plugin! {
    struct ModuleFederationPlugin => "module-federation",
    enablers: ENABLERS,
    config_patterns: CONFIG_PATTERNS,
    always_used: ALWAYS_USED,
    resolve_config(config_path, source, root) {
        let mut result = PluginResult::default();
        super::add_import_referenced_dependencies(&mut result, source, config_path);

        apply_from_source(
            &mut result,
            source,
            &ConfigLocation { config_path, root, context: None },
            "module-federation",
            &FederationSites {
                plugin_arrays: &[],
                read_config_object: true,
            },
        );

        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CONFIG: &str = "/project/module-federation.config.ts";
    const ROOT: &str = "/project";

    fn resolve(source: &str) -> PluginResult {
        ModuleFederationPlugin.resolve_config(Path::new(CONFIG), source, Path::new(ROOT))
    }

    fn entry_patterns(result: &PluginResult) -> Vec<String> {
        result
            .entry_patterns
            .iter()
            .map(|rule| rule.pattern.clone())
            .collect()
    }

    fn standalone(source: &str) -> (FederationConfig, Vec<ComputedKey>) {
        read(
            source,
            Path::new(CONFIG),
            &FederationSites {
                plugin_arrays: &[],
                read_config_object: true,
            },
        )
    }

    #[test]
    fn exposes_string_target_becomes_entry_pattern() {
        let result = resolve(
            r"
            import { createModuleFederationConfig } from '@module-federation/enhanced';
            export default createModuleFederationConfig({
                name: 'checkout',
                exposes: { './Button': './src/components/Button.tsx' },
            });
            ",
        );
        assert_eq!(entry_patterns(&result), vec!["src/components/Button.tsx"]);
        assert!(
            result
                .referenced_dependencies
                .contains(&"@module-federation/enhanced".to_string())
        );
    }

    #[test]
    fn exposes_import_descriptor_becomes_entry_pattern() {
        let result = resolve(
            r"
            export default {
                exposes: { './Button': { import: './src/components/Button.tsx' } },
            };
            ",
        );
        assert_eq!(entry_patterns(&result), vec!["src/components/Button.tsx"]);
    }

    #[test]
    fn extensionless_exposes_target_expands_file_and_directory_index() {
        let result = resolve(r"export default { exposes: { './Button': './src/Button' } };");
        assert_eq!(
            entry_patterns(&result),
            vec![
                format!("src/Button.{EXPOSE_EXTENSIONS}"),
                format!("src/Button/index.{EXPOSE_EXTENSIONS}"),
            ]
        );
    }

    #[test]
    fn bare_module_request_target_is_credited_as_dependency() {
        let result = resolve(r"export default { exposes: { './utils': 'shared-utils' } };");
        assert!(entry_patterns(&result).is_empty());
        assert!(
            result
                .referenced_dependencies
                .contains(&"shared-utils".to_string())
        );
    }

    #[test]
    fn target_outside_the_project_root_is_dropped() {
        let result = resolve(r"export default { exposes: { './Button': '../other/Button.tsx' } };");
        assert!(entry_patterns(&result).is_empty());
    }

    #[test]
    fn remote_alias_covers_the_alias_and_its_subpaths_only() {
        let result = resolve(
            r"
            export default {
                remotes: { checkout: 'checkout@https://example.test/remoteEntry.js' },
            };
            ",
        );
        let rules = &result.provided_dependencies;
        assert_eq!(rules.len(), 1);
        let rule = &rules[0];
        assert!(rule.covers_specifier("checkout"));
        assert!(rule.covers_specifier("checkout/Button"));
        assert!(!rule.covers_specifier("checkout-ui"));
        assert!(rule.may_cover_package("checkout"));
    }

    #[test]
    fn remote_rule_is_scoped_to_the_directory_that_declared_it() {
        let source = r"
            export default {
                remotes: { checkout: 'checkout@https://example.test/remoteEntry.js' },
            };
        ";
        let nested = ModuleFederationPlugin.resolve_config(
            Path::new("/project/packages/host/module-federation.config.ts"),
            source,
            Path::new("/project"),
        );
        assert_eq!(
            nested.provided_dependencies[0].path.pattern,
            format!("packages/host/{SCOPE_SUFFIX}")
        );

        let at_root = resolve(source);
        assert_eq!(at_root.provided_dependencies[0].path.pattern, SCOPE_SUFFIX);
    }

    #[test]
    fn remote_external_descriptor_still_yields_the_alias() {
        let result = resolve(
            r"
            export default {
                remotes: {
                    remote: { external: 'app@http://example.test/remoteEntry.js', shareScope: 'default' },
                },
            };
            ",
        );
        assert_eq!(result.provided_dependencies.len(), 1);
        assert!(result.provided_dependencies[0].covers_specifier("remote/Thing"));
    }

    #[test]
    fn common_js_config_reads_both_keys() {
        let result = resolve(
            r"
            module.exports = {
                exposes: { './Button': './src/Button.tsx' },
                remotes: { checkout: 'checkout@https://example.test/remoteEntry.js' },
            };
            ",
        );
        assert_eq!(entry_patterns(&result), vec!["src/Button.tsx"]);
        assert_eq!(result.provided_dependencies.len(), 1);
    }

    #[test]
    fn computed_exposes_reports_the_key_and_keeps_literal_siblings() {
        let (config, computed) =
            standalone(r"export default { exposes: computeExposes(), remotes: {} };");
        assert!(config.exposed_targets.is_empty());
        assert_eq!(computed, vec![ComputedKey::Exposes]);

        let (config, computed) = standalone(
            r"
            export default {
                exposes: { './a': './src/a.ts', ...extraExposes },
            };
            ",
        );
        assert_eq!(config.exposed_targets, vec!["./src/a.ts".to_string()]);
        assert_eq!(computed, vec![ComputedKey::Exposes]);
    }

    #[test]
    fn shorthand_remotes_property_reports_the_key() {
        let (config, computed) = standalone(
            r"
            const remotes = { checkout: 'checkout@https://example.test/remoteEntry.js' };
            export default { remotes };
            ",
        );
        assert!(config.remote_aliases.is_empty());
        assert_eq!(computed, vec![ComputedKey::Remotes]);
    }

    #[test]
    fn config_without_federation_keys_contributes_nothing() {
        let result = resolve(r"export default { name: 'checkout' };");
        assert!(result.is_empty());
    }

    #[test]
    fn inline_plugin_options_are_read_from_a_plugins_array() {
        let source = r"
            const { ModuleFederationPlugin } = require('webpack').container;
            module.exports = {
                plugins: [
                    new ModuleFederationPlugin({
                        name: 'host',
                        exposes: { './Button': './src/Button.tsx' },
                        remotes: { checkout: 'checkout@https://example.test/remoteEntry.js' },
                    }),
                ],
            };
        ";
        let (config, computed) = read(
            source,
            Path::new("webpack.config.js"),
            &FederationSites {
                plugin_arrays: &[&["plugins"]],
                read_config_object: false,
            },
        );
        assert_eq!(config.exposed_targets, vec!["./src/Button.tsx".to_string()]);
        assert_eq!(config.remote_aliases, vec!["checkout".to_string()]);
        assert!(computed.is_empty());
    }

    #[test]
    fn member_expression_callee_is_recognised() {
        let (config, _) = read(
            r"
            module.exports = {
                plugins: [
                    new webpack.container.ModuleFederationPlugin({
                        exposes: { './B': './src/B.tsx' },
                    }),
                ],
            };
            ",
            Path::new("webpack.config.js"),
            &FederationSites {
                plugin_arrays: &[&["plugins"]],
                read_config_object: false,
            },
        );
        assert_eq!(config.exposed_targets, vec!["./src/B.tsx".to_string()]);
    }

    #[test]
    fn plugin_call_without_federation_keys_is_inert() {
        for source in [
            r"module.exports = { plugins: [new ModuleFederationPlugin({ name: 'x' })] };",
            r"module.exports = { plugins: [somethingElse({ exposes: { './a': './src/a.ts' } })] };",
            r"module.exports = { plugins: [federation(mfConfig)] };",
        ] {
            let (config, computed) = read(
                source,
                Path::new("webpack.config.js"),
                &FederationSites {
                    plugin_arrays: &[&["plugins"]],
                    read_config_object: false,
                },
            );
            assert_eq!(config, FederationConfig::default(), "source: {source}");
            assert!(computed.is_empty(), "source: {source}");
        }
    }
}
