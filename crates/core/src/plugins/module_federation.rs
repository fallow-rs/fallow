//! Module Federation plugin and the shared `exposes` / `remotes` reader.
//!
//! Federation options reach a build in two shapes. A standalone
//! `module-federation.config.*` file default-exports the options object and is
//! owned by this plugin. The same options also reach a Federation plugin call
//! inside a bundler config, which the webpack, rspack, rsbuild, vite and
//! Next.js plugins read through [`apply_bundler_plugin_options`]. A call is read
//! wherever it sits in the config, because a plugin list is nested, held by a
//! variable, or built inside a hook as often as it is a literal array.
//!
//! Reading is syntactic. `exposes` targets become entry-point globs so an
//! exposed module is not mistaken for dead code, and `remotes` aliases become
//! runtime-provided specifiers so an import of a remote container is not
//! mistaken for an unlisted npm dependency. No remote container is fetched and
//! no cross-deployment reachability is inferred.

use std::path::Path;

use oxc_ast::ast::{
    Argument, CallExpression, Expression, NewExpression, ObjectExpression, ObjectPropertyKind,
    Program, PropertyKey,
};
use oxc_ast_visit::{Visit, walk};

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
/// `pluginModuleFederation` for rsbuild, `federation` for vite,
/// `NextFederationPlugin` for Next.js.
const FEDERATION_CALLEES: &[&str] = &[
    "ModuleFederationPlugin",
    "NextFederationPlugin",
    "moduleFederationPlugin",
    "pluginModuleFederation",
    "federation",
];

/// Brace list appended to an extensionless `exposes` target. Entry patterns are
/// plain globs with no extension expansion, so a bare `src/Button` would match
/// no file.
const EXPOSE_EXTENSIONS: &str = "{ts,tsx,mts,cts,gts,js,jsx,mjs,cjs,gjs,vue,svelte,astro,mdx}";

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
struct FederationSites {
    /// Read the options of every Federation plugin call in the file, at any
    /// position.
    pub read_plugin_calls: bool,
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

/// A Federation key this reader understands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FederationKey {
    Exposes,
    Remotes,
}

impl FederationKey {
    const fn name(self) -> &'static str {
        match self {
            Self::Exposes => "exposes",
            Self::Remotes => "remotes",
        }
    }
}

/// Why a Federation key declaration could not be read in full.
///
/// The consequence and the remedy are rendered by the shared diagnostic
/// message, keyed on the token each variant maps to, so the vocabulary lives at
/// the one place every workspace diagnostic is rendered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UnreadReason {
    /// The value is not an object literal.
    NotObjectLiteral,
    /// The value is the array form, which this reader does not read yet.
    ArrayForm,
    /// The object literal spreads a value that is not statically readable, so
    /// it may declare more than what was read.
    Spread,
    /// At least one entry's value holds no statically readable string.
    Entries,
}

/// One Federation key declaration that was present but not fully readable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct UnreadDeclaration {
    key: FederationKey,
    reason: UnreadReason,
}

impl UnreadReason {
    /// The kebab-case token that reaches the wire. The shared renderer builds
    /// the situation clause from it.
    const fn token(self) -> &'static str {
        match self {
            Self::NotObjectLiteral => "not-object-literal",
            Self::ArrayForm => "array-form",
            Self::Spread => "spread",
            Self::Entries => "unreadable-entries",
        }
    }
}

/// Read every statically available Federation options object in `source`,
/// recording one advisory per `exposes` or `remotes` declaration that is present
/// but not fully readable.
///
/// The plugin records the fact and never prints it: one renderer owns the
/// sentence, and one registry owns the deduplication, so a combined run states
/// it once and a consumer reading the envelope sees it at all (issue #2736).
fn extract(
    result: &mut PluginResult,
    source: &str,
    location: &ConfigLocation<'_>,
    plugin_label: &str,
    sites: &FederationSites,
) -> FederationConfig {
    let (config, unread) = read(source, location.config_path, sites);
    for declaration in unread {
        result
            .config_diagnostics
            .push(super::PluginConfigDiagnostic::unreadable(
                location.config_path,
                plugin_label,
                declaration.key.name(),
                declaration.reason.token(),
            ));
    }
    config
}

fn apply_from_source(
    result: &mut PluginResult,
    source: &str,
    location: &ConfigLocation<'_>,
    plugin_label: &str,
    sites: &FederationSites,
) {
    let config = extract(result, source, location, plugin_label, sites);
    apply(result, &config, location);
}

/// Read the Federation options of every Federation plugin call in a bundler
/// config and register what they declare.
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
            read_plugin_calls: true,
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
    // An entry pattern is compiled as a glob, while a target is a literal path.
    // Bracketed route filenames are the Next.js convention, so an unescaped
    // target would both miss the exposed file and credit an unrelated one.
    let escaped = globset::escape(&normalized);
    if super::has_source_extension(&normalized) {
        result.push_entry_pattern(escaped);
        return;
    }
    result.push_entry_pattern(format!("{escaped}.{EXPOSE_EXTENSIONS}"));
    result.push_entry_pattern(format!("{escaped}/index.{EXPOSE_EXTENSIONS}"));
}

fn read(
    source: &str,
    config_path: &Path,
    sites: &FederationSites,
) -> (FederationConfig, Vec<UnreadDeclaration>) {
    config_parser::extract_from_source(source, config_path, |program| {
        let mut collector = FederationCallCollector::new(program);

        if sites.read_config_object
            && let Some(config_object) = config_parser::find_config_object(program)
        {
            collector.read_options(config_object);
        }
        if sites.read_plugin_calls {
            collector.visit_program(program);
        }

        Some((collector.config, collector.unread))
    })
    .unwrap_or_default()
}

/// Every Federation plugin call in one config program, at any position.
///
/// A bundler config holds its plugin list in a literal array, in a nested array,
/// in a variable, under a tool-specific key, or inside a hook that receives the
/// config. One walk covers all of them, and the accept gate stays the callee name
/// plus an options object that declares a Federation key.
struct FederationCallCollector<'a> {
    program: &'a Program<'a>,
    config: FederationConfig,
    unread: Vec<UnreadDeclaration>,
}

impl<'a> FederationCallCollector<'a> {
    fn new(program: &'a Program<'a>) -> Self {
        Self {
            program,
            config: FederationConfig::default(),
            unread: Vec::new(),
        }
    }

    fn read_options(&mut self, options: &ObjectExpression<'_>) {
        read_exposes(options, &mut self.config, &mut self.unread);
        read_remotes(options, &mut self.config, &mut self.unread);
    }

    fn read_plugin_call(&mut self, callee: &Expression<'a>, arguments: &[Argument<'a>]) {
        if !is_federation_callee(callee) {
            return;
        }
        let Some(options) = arguments
            .first()
            .and_then(Argument::as_expression)
            .and_then(|expr| config_parser::resolve_object_expression(self.program, expr))
        else {
            return;
        };
        if !declares_federation_keys(options) {
            return;
        }
        self.read_options(options);
    }
}

impl<'a> Visit<'a> for FederationCallCollector<'a> {
    fn visit_new_expression(&mut self, new_expression: &NewExpression<'a>) {
        self.read_plugin_call(&new_expression.callee, &new_expression.arguments);
        walk::walk_new_expression(self, new_expression);
    }

    fn visit_call_expression(&mut self, call: &CallExpression<'a>) {
        self.read_plugin_call(&call.callee, &call.arguments);
        walk::walk_call_expression(self, call);
    }
}

fn read_exposes(
    options: &ObjectExpression<'_>,
    config: &mut FederationConfig,
    unread: &mut Vec<UnreadDeclaration>,
) {
    let mut has_unread_entry = false;
    for declaration in federation_key_declarations(options, FederationKey::Exposes, unread) {
        let mapping = match declaration {
            KeyDeclaration::Target(target) => {
                classify_exposed_target(&target, config);
                continue;
            }
            KeyDeclaration::Mapping(mapping) => mapping,
        };
        for property in &mapping.properties {
            let ObjectPropertyKind::ObjectProperty(property) = property else {
                continue;
            };
            let targets = exposed_target_strings(&property.value);
            if targets.is_empty() {
                has_unread_entry = true;
                continue;
            }
            for target in targets {
                classify_exposed_target(&target, config);
            }
        }
    }
    if has_unread_entry {
        push_unique(
            unread,
            UnreadDeclaration {
                key: FederationKey::Exposes,
                reason: UnreadReason::Entries,
            },
        );
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
    if let Some(request) = super::module_request(trimmed) {
        push_unique(
            &mut config.exposed_packages,
            crate::resolve::extract_package_name(request),
        );
        return;
    }
    push_unique(&mut config.exposed_targets, trimmed.to_string());
}

fn read_remotes(
    options: &ObjectExpression<'_>,
    config: &mut FederationConfig,
    unread: &mut Vec<UnreadDeclaration>,
) {
    for declaration in federation_key_declarations(options, FederationKey::Remotes, unread) {
        let KeyDeclaration::Mapping(mapping) = declaration else {
            continue;
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
}

/// One readable declaration under a Federation key.
enum KeyDeclaration<'a> {
    /// An object literal that maps a public name to a target.
    Mapping(&'a ObjectExpression<'a>),
    /// A single target, from the array form. A bundler uses the element both as
    /// the public name and as the module request.
    Target(String),
}

/// Resolve one Federation key to the declarations it holds, recording why the
/// declaration is not fully readable when that is the case.
///
/// The object form gives one mapping. The array form gives one declaration per
/// element, except under `remotes`, whose array form stays unread: a bundler
/// derives the request scope of an element from the whole container location,
/// which is never a bare specifier a provider rule can cover.
fn federation_key_declarations<'a>(
    options: &'a ObjectExpression<'a>,
    key: FederationKey,
    unread: &mut Vec<UnreadDeclaration>,
) -> Vec<KeyDeclaration<'a>> {
    let Some(value) = config_parser::property_expr(options, key.name()) else {
        return Vec::new();
    };
    if let Some(mapping) = config_parser::object_expression(value) {
        record_spread(mapping, key, unread);
        return vec![KeyDeclaration::Mapping(mapping)];
    }
    let Some(array) = config_parser::array_expression(value) else {
        push_unique(
            unread,
            UnreadDeclaration {
                key,
                reason: UnreadReason::NotObjectLiteral,
            },
        );
        return Vec::new();
    };
    if key == FederationKey::Remotes {
        push_unique(
            unread,
            UnreadDeclaration {
                key,
                reason: UnreadReason::ArrayForm,
            },
        );
        return Vec::new();
    }

    let mut declarations = Vec::new();
    let mut has_unread_element = false;
    for element in &array.elements {
        let Some(expr) = element.as_expression() else {
            has_unread_element = true;
            continue;
        };
        if let Some(mapping) = config_parser::object_expression(expr) {
            record_spread(mapping, key, unread);
            declarations.push(KeyDeclaration::Mapping(mapping));
            continue;
        }
        let targets = config_parser::expression_to_string_or_array(expr);
        if targets.is_empty() {
            has_unread_element = true;
            continue;
        }
        declarations.extend(targets.into_iter().map(KeyDeclaration::Target));
    }
    if has_unread_element {
        push_unique(
            unread,
            UnreadDeclaration {
                key,
                reason: UnreadReason::Entries,
            },
        );
    }
    declarations
}

/// Record that a mapping spreads a value, which means it may declare more than
/// what was read.
fn record_spread(
    mapping: &ObjectExpression<'_>,
    key: FederationKey,
    unread: &mut Vec<UnreadDeclaration>,
) {
    if mapping
        .properties
        .iter()
        .any(|property| matches!(property, ObjectPropertyKind::SpreadProperty(_)))
    {
        push_unique(
            unread,
            UnreadDeclaration {
                key,
                reason: UnreadReason::Spread,
            },
        );
    }
}

/// Whether the options object declares a Federation key. The shape gate keeps a
/// same-named local symbol from activating extraction, which matters because a
/// webpack config commonly binds the plugin through `require` rather than an
/// import declaration.
fn declares_federation_keys(options: &ObjectExpression<'_>) -> bool {
    config_parser::property_expr(options, "exposes").is_some()
        || config_parser::property_expr(options, "remotes").is_some()
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

        let location = ConfigLocation { config_path, root, context: None };
        // The declared `always_used` pattern is matched against the
        // project-relative path without a `**/` rewrite, so it covers a root
        // config only. Credit the file that was actually read, at any depth.
        if let Some(relative) = location.relative_config_path() {
            result.always_used_files.push(globset::escape(&relative));
        }

        apply_from_source(
            &mut result,
            source,
            &location,
            "module-federation",
            &FederationSites {
                read_plugin_calls: false,
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

    /// Compile an entry pattern the way `CompiledPathRule::for_entry_rule` does,
    /// so a test observes the paths a pattern really covers.
    fn covers(pattern: &str, path: &str) -> bool {
        globset::GlobBuilder::new(pattern)
            .literal_separator(true)
            .build()
            .expect("entry pattern compiles")
            .compile_matcher()
            .is_match(path)
    }

    fn standalone(source: &str) -> (FederationConfig, Vec<UnreadDeclaration>) {
        read(
            source,
            Path::new(CONFIG),
            &FederationSites {
                read_plugin_calls: false,
                read_config_object: true,
            },
        )
    }

    /// Read a bundler config the way every bundler plugin does: plugin calls
    /// only, wherever they sit.
    fn bundler(source: &str) -> (FederationConfig, Vec<UnreadDeclaration>) {
        read(
            source,
            Path::new("webpack.config.js"),
            &FederationSites {
                read_plugin_calls: true,
                read_config_object: false,
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
    fn bracketed_target_covers_the_exposed_file_only() {
        let result = resolve(r"export default { exposes: { './Page': './src/pages/[id].tsx' } };");
        let patterns = entry_patterns(&result);
        assert_eq!(patterns.len(), 1, "got {patterns:?}");
        assert!(
            covers(&patterns[0], "src/pages/[id].tsx"),
            "the exposed file is covered, got {patterns:?}"
        );
        assert!(
            !covers(&patterns[0], "src/pages/d.tsx"),
            "a bracket is not a character class, got {patterns:?}"
        );
    }

    #[test]
    fn wildcard_target_does_not_cover_files_the_config_does_not_name() {
        let result = resolve(r"export default { exposes: { './all': './src/*' } };");
        let patterns = entry_patterns(&result);
        assert!(
            !patterns
                .iter()
                .any(|pattern| covers(pattern, "src/unrelated.ts")),
            "got {patterns:?}"
        );
        assert!(
            patterns.iter().any(|pattern| covers(pattern, "src/*.ts")),
            "a file literally named `*` is still covered, got {patterns:?}"
        );
    }

    #[test]
    fn a_target_naming_a_discovered_extension_is_used_as_written() {
        let result = resolve(r"export default { exposes: { './Button': './src/Button.gts' } };");
        assert_eq!(entry_patterns(&result), vec!["src/Button.gts"]);
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

    fn unread(key: FederationKey, reason: UnreadReason) -> Vec<UnreadDeclaration> {
        vec![UnreadDeclaration { key, reason }]
    }

    #[test]
    fn computed_exposes_reports_the_key_and_keeps_literal_siblings() {
        let (config, declarations) =
            standalone(r"export default { exposes: computeExposes(), remotes: {} };");
        assert!(config.exposed_targets.is_empty());
        assert_eq!(
            declarations,
            unread(FederationKey::Exposes, UnreadReason::NotObjectLiteral)
        );

        let (config, declarations) = standalone(
            r"
            export default {
                exposes: { './a': './src/a.ts', ...extraExposes },
            };
            ",
        );
        assert_eq!(config.exposed_targets, vec!["./src/a.ts".to_string()]);
        assert_eq!(
            declarations,
            unread(FederationKey::Exposes, UnreadReason::Spread)
        );
    }

    #[test]
    fn an_entry_whose_target_is_not_readable_is_reported_once_beside_its_siblings() {
        let (config, declarations) = standalone(
            r"
            const widget = './src/Widget.tsx';
            export default {
                exposes: {
                    './Button': './src/Button.tsx',
                    './Widget': widget,
                    './Card': { name: 'card' },
                },
            };
            ",
        );
        assert_eq!(config.exposed_targets, vec!["./src/Button.tsx".to_string()]);
        assert_eq!(
            declarations,
            unread(FederationKey::Exposes, UnreadReason::Entries),
            "two unreadable entries under one key are one advisory"
        );
    }

    /// A bundler uses a string element of the `exposes` array both as the public
    /// name and as the module request, so the element is a target.
    #[test]
    fn exposes_array_form_reads_string_elements() {
        let (config, declarations) =
            standalone(r"export default { exposes: ['./src/Button.tsx', 'shared-utils'] };");
        assert_eq!(config.exposed_targets, vec!["./src/Button.tsx".to_string()]);
        assert_eq!(config.exposed_packages, vec!["shared-utils".to_string()]);
        assert!(declarations.is_empty(), "got {declarations:?}");
    }

    /// An object element of the array goes through the same mapping reader as
    /// the object form, entry descriptor included.
    #[test]
    fn exposes_array_form_reads_object_elements() {
        let (config, declarations) = standalone(
            r"
            export default {
                exposes: [
                    { './Button': './src/Button.tsx' },
                    { './Card': { import: './src/Card.tsx' } },
                ],
            };
            ",
        );
        assert_eq!(
            config.exposed_targets,
            vec!["./src/Button.tsx".to_string(), "./src/Card.tsx".to_string()]
        );
        assert!(declarations.is_empty(), "got {declarations:?}");
    }

    #[test]
    fn exposes_array_element_without_a_readable_target_is_reported() {
        let (config, declarations) = standalone(
            r"
            const widget = './src/Widget.tsx';
            export default { exposes: ['./src/Button.tsx', widget] };
            ",
        );
        assert_eq!(config.exposed_targets, vec!["./src/Button.tsx".to_string()]);
        assert_eq!(
            declarations,
            unread(FederationKey::Exposes, UnreadReason::Entries)
        );
    }

    /// A bundler derives the request scope of a `remotes` array element from the
    /// whole container location, which is never a bare specifier a provider rule
    /// can cover, so the array form of `remotes` stays unread.
    #[test]
    fn remotes_array_form_is_not_read() {
        let (config, declarations) = standalone(
            r"export default { remotes: ['checkout@https://example.test/remoteEntry.js'] };",
        );
        assert!(config.remote_aliases.is_empty());
        assert_eq!(
            declarations,
            unread(FederationKey::Remotes, UnreadReason::ArrayForm)
        );
    }

    /// The reader records a fact, and the shared renderer turns it into the
    /// sentence, so each shape has to reach the wire as its own token.
    #[test]
    fn each_unread_shape_carries_its_own_reason_token() {
        assert_eq!(UnreadReason::NotObjectLiteral.token(), "not-object-literal");
        assert_eq!(UnreadReason::ArrayForm.token(), "array-form");
        assert_eq!(UnreadReason::Spread.token(), "spread");
        assert_eq!(UnreadReason::Entries.token(), "unreadable-entries");
    }

    /// The advisory names the config file that was read and the plugin that
    /// read it, and the standalone reader names itself.
    #[test]
    fn an_unreadable_key_records_a_diagnostic_on_its_config_file() {
        let result = resolve(r"export default { exposes: computeExposes() };");
        assert_eq!(result.config_diagnostics.len(), 1);
        let diagnostic = &result.config_diagnostics[0];
        assert_eq!(diagnostic.config_path, Path::new(CONFIG));
        assert_eq!(diagnostic.plugin, "module-federation");
        assert_eq!(diagnostic.key, "exposes");
        assert_eq!(diagnostic.reason, "not-object-literal");
        assert_eq!(
            diagnostic.effect,
            super::super::PluginConfigEffect::Unreadable
        );
    }

    /// Two unreadable keys in one config file are two advisories: they share a
    /// kind and a path, and only the payload tells them apart.
    #[test]
    fn both_unreadable_keys_in_one_config_are_recorded() {
        let result = resolve(
            r"
            export default {
                exposes: makeExposes(),
                remotes: { ...envRemotes },
            };
            ",
        );
        let recorded: Vec<(&str, &str)> = result
            .config_diagnostics
            .iter()
            .map(|diagnostic| (diagnostic.key.as_str(), diagnostic.reason.as_str()))
            .collect();
        assert_eq!(
            recorded,
            vec![("exposes", "not-object-literal"), ("remotes", "spread")],
            "{:?}",
            result.config_diagnostics
        );
    }

    /// A config the reader understands in full records nothing, so a consumer
    /// warning on the kind warns about something.
    #[test]
    fn a_readable_config_records_no_diagnostic() {
        let result = resolve(
            r"
            export default {
                exposes: { './Button': './src/Button.tsx' },
                remotes: { checkout: 'checkout@https://example.test/remoteEntry.js' },
            };
            ",
        );
        assert!(
            result.config_diagnostics.is_empty(),
            "{:?}",
            result.config_diagnostics
        );
    }

    /// The same reader serves four bundler plugins through inline options, and
    /// the config file the user must edit is the bundler's, so the advisory
    /// names the bundler plugin rather than the reader.
    #[test]
    fn inline_bundler_options_record_under_the_bundler_plugin() {
        let mut result = PluginResult::default();
        let config_path = Path::new("/project/webpack.config.js");
        apply_bundler_plugin_options(
            &mut result,
            r"
            module.exports = {
                plugins: [new ModuleFederationPlugin({ remotes: envRemotes() })],
            };
            ",
            config_path,
            Path::new("/project"),
            None,
            "webpack",
        );
        assert_eq!(result.config_diagnostics.len(), 1);
        let diagnostic = &result.config_diagnostics[0];
        assert_eq!(diagnostic.plugin, "webpack");
        assert_eq!(diagnostic.key, "remotes");
        assert_eq!(diagnostic.config_path, config_path);
    }

    /// A config whose ONLY contribution is an advisory must not be discarded by
    /// the registry's empty-result gate, which is how a bundler config with a
    /// computed `remotes` map and nothing else reaches the report.
    #[test]
    fn a_result_carrying_only_a_diagnostic_is_not_empty() {
        let mut result = PluginResult::default();
        assert!(result.is_empty());
        result
            .config_diagnostics
            .push(super::super::PluginConfigDiagnostic::unreadable(
                Path::new("/project/webpack.config.js"),
                "webpack",
                "remotes",
                "not-object-literal",
            ));
        assert!(
            !result.is_empty(),
            "the advisory is the whole contribution of this config"
        );
    }

    #[test]
    fn shorthand_remotes_property_reports_the_key() {
        let (config, declarations) = standalone(
            r"
            const remotes = { checkout: 'checkout@https://example.test/remoteEntry.js' };
            export default { remotes };
            ",
        );
        assert!(config.remote_aliases.is_empty());
        assert_eq!(
            declarations,
            unread(FederationKey::Remotes, UnreadReason::NotObjectLiteral)
        );
    }

    #[test]
    fn config_without_federation_keys_contributes_only_its_own_file() {
        let result = resolve(r"export default { name: 'checkout' };");
        assert!(entry_patterns(&result).is_empty());
        assert!(result.provided_dependencies.is_empty());
        assert!(result.referenced_dependencies.is_empty());
        assert_eq!(
            result.always_used_files,
            vec!["module-federation.config.ts".to_string()]
        );
    }

    #[test]
    fn a_nested_config_file_is_credited_as_used() {
        let nested = ModuleFederationPlugin.resolve_config(
            Path::new("/project/packages/host/module-federation.config.ts"),
            r"export default { exposes: { './Button': './src/Button.tsx' } };",
            Path::new("/project"),
        );
        assert_eq!(
            nested.always_used_files,
            vec!["packages/host/module-federation.config.ts".to_string()]
        );
    }

    #[test]
    fn inline_plugin_options_are_read_from_a_plugins_array() {
        let (config, computed) = bundler(
            r"
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
            ",
        );
        assert_eq!(config.exposed_targets, vec!["./src/Button.tsx".to_string()]);
        assert_eq!(config.remote_aliases, vec!["checkout".to_string()]);
        assert!(computed.is_empty());
    }

    #[test]
    fn member_expression_callee_is_recognised() {
        let (config, _) = bundler(
            r"
            module.exports = {
                plugins: [
                    new webpack.container.ModuleFederationPlugin({
                        exposes: { './B': './src/B.tsx' },
                    }),
                ],
            };
            ",
        );
        assert_eq!(config.exposed_targets, vec!["./src/B.tsx".to_string()]);
    }

    /// Vite flattens a nested plugin array, so a Federation call one level down
    /// is part of the same build.
    #[test]
    fn federation_options_in_a_nested_plugin_array_are_read() {
        let (config, computed) = bundler(
            r"
            export default defineConfig({
                plugins: [
                    [react(), federation({ exposes: { './Button': './src/Button.tsx' } })],
                    other(),
                ],
            });
            ",
        );
        assert_eq!(config.exposed_targets, vec!["./src/Button.tsx".to_string()]);
        assert!(computed.is_empty(), "got {computed:?}");
    }

    /// A Next.js config registers the plugin inside the `webpack(config)` hook,
    /// which no config-object path reaches.
    #[test]
    fn federation_options_outside_the_plugins_array_are_read() {
        let (config, computed) = bundler(
            r"
            module.exports = {
                webpack(config, options) {
                    config.plugins.push(
                        new NextFederationPlugin({
                            name: 'shop',
                            exposes: { './pages-map': './pages-map.js' },
                        }),
                    );
                    return config;
                },
            };
            ",
        );
        assert_eq!(config.exposed_targets, vec!["./pages-map.js".to_string()]);
        assert!(computed.is_empty(), "got {computed:?}");
    }

    #[test]
    fn federation_options_from_a_plugins_identifier_are_read() {
        let (config, _) = bundler(
            r"
            const plugins = [
                new ModuleFederationPlugin({ exposes: { './Button': './src/Button.tsx' } }),
            ];
            module.exports = { plugins };
            ",
        );
        assert_eq!(config.exposed_targets, vec!["./src/Button.tsx".to_string()]);
    }

    /// An rsbuild config holds its rspack plugin list under `tools.rspack`.
    #[test]
    fn federation_options_under_a_tool_key_are_read() {
        let (config, _) = bundler(
            r"
            export default {
                tools: {
                    rspack: {
                        plugins: [
                            new ModuleFederationPlugin({
                                exposes: { './Button': './src/Button.tsx' },
                            }),
                        ],
                    },
                },
            };
            ",
        );
        assert_eq!(config.exposed_targets, vec!["./src/Button.tsx".to_string()]);
    }

    /// Options held by a `const` above the plugin list are the most common real
    /// shape, so the reader resolves a same-file binding.
    #[test]
    fn federation_options_bound_to_a_local_const_are_read() {
        let (config, computed) = bundler(
            r"
            const mfConfig = {
                name: 'host',
                exposes: { './Button': './src/Button.tsx' },
                remotes: { checkout: 'checkout@https://example.test/remoteEntry.js' },
            };
            module.exports = {
                plugins: [new ModuleFederationPlugin(mfConfig)],
            };
            ",
        );
        assert_eq!(config.exposed_targets, vec!["./src/Button.tsx".to_string()]);
        assert_eq!(config.remote_aliases, vec!["checkout".to_string()]);
        assert!(computed.is_empty(), "got {computed:?}");
    }

    /// One options object read through two positions stays one declaration.
    #[test]
    fn the_same_call_read_twice_registers_one_target() {
        let (config, _) = bundler(
            r"
            const federationPlugin = new ModuleFederationPlugin({
                exposes: { './Button': './src/Button.tsx' },
            });
            module.exports = { plugins: [federationPlugin, federationPlugin] };
            ",
        );
        assert_eq!(config.exposed_targets, vec!["./src/Button.tsx".to_string()]);
    }

    /// The callee name alone never activates the reader. A widened search must
    /// keep the shape gate, or a library that happens to export `federation`
    /// registers entry points for an unrelated project.
    #[test]
    fn plugin_call_without_federation_keys_is_inert() {
        for source in [
            r"module.exports = { plugins: [new ModuleFederationPlugin({ name: 'x' })] };",
            r"module.exports = { plugins: [somethingElse({ exposes: { './a': './src/a.ts' } })] };",
            r"module.exports = { plugins: [federation(mfConfig)] };",
            r"module.exports = { plugins: [federation('graphql-schema', { batch: true })] };",
            r"
            const options = { registry: './src/registry.ts' };
            module.exports = { plugins: [federation(options)] };
            ",
            r"
            export default defineConfig({
                plugins: [[federation(), other()]],
            });
            ",
        ] {
            let (config, computed) = bundler(source);
            assert_eq!(config, FederationConfig::default(), "source: {source}");
            assert!(computed.is_empty(), "source: {source}");
        }
    }
}
