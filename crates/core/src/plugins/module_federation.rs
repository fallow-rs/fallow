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

/// The build plugin packages among the enablers. A standalone config that
/// declares a Federation key is read by one of these, and no config file
/// imports it, so the config credits them. The runtime packages are imported by
/// application code, which credits them on its own.
const BUILD_PLUGIN_ENABLERS: &[&str] = &[
    "@module-federation/enhanced",
    "@module-federation/modern-js",
    "@module-federation/nextjs-mf",
    "@module-federation/node",
    "@module-federation/rsbuild-plugin",
    "@module-federation/rspack",
    "@module-federation/vite",
    "@originjs/vite-plugin-federation",
];

/// Calls that return their options argument unchanged, so the argument is
/// read as the options with no diagnostic. Any other call can add to or change
/// what it returns.
const IDENTITY_WRAPPERS: &[&str] = &["createModuleFederationConfig", "defineConfig"];

const CONFIG_PATTERNS: &[&str] = &["module-federation.config.{ts,js,mjs,cjs,mts,cts}"];

const ALWAYS_USED: &[&str] = CONFIG_PATTERNS;

/// Callee names that receive Module Federation options inline in a bundler
/// config: `ModuleFederationPlugin` for webpack and rspack,
/// `pluginModuleFederation` for rsbuild, `federation` for vite,
/// `NextFederationPlugin` for Next.js.
/// The Federation callee that other libraries also name a function, so its
/// options pass the shape gate only when they declare a Federation key.
const AMBIGUOUS_CALLEE: &str = "federation";

const FEDERATION_CALLEES: &[&str] = &[
    "ModuleFederationPlugin",
    "NextFederationPlugin",
    "moduleFederationPlugin",
    "pluginModuleFederation",
    "federation",
];

/// Brace list appended to an extensionless `exposes` target.
const EXPOSE_EXTENSIONS: &str = super::REQUEST_EXTENSIONS;

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
    /// Project-relative package directory that replaces the config directory
    /// when the config sits in a config directory such as `config/`.
    pub package_dir: Option<&'a Path>,
}

/// The directories a bundler config anchors its Federation declarations to,
/// when they differ from the config file's own directory.
#[derive(Debug, Clone, Copy, Default)]
pub(super) struct FederationBase<'a> {
    /// The base directory option of the config, such as webpack's `context`,
    /// as a project-relative path.
    pub context: Option<&'a Path>,
    /// The project-relative package directory of a config that sits in a
    /// config directory. Webpack runs such a config from the package root.
    pub package_dir: Option<&'a Path>,
}

impl ConfigLocation<'_> {
    /// The directory a relative `exposes` target resolves against, expressed as
    /// a stand-in config path so the shared path normalization applies.
    fn target_base(&self) -> std::borrow::Cow<'_, Path> {
        match self.context.or(self.package_dir) {
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
        let directory = match self.package_dir {
            Some(package_dir) => Some(config_parser::path_to_config_string(package_dir)),
            None => self.relative_config_path().and_then(|relative| {
                Path::new(&relative)
                    .parent()
                    .map(config_parser::path_to_config_string)
            }),
        }
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
    /// The options pass through a call that is not a known identity wrapper,
    /// which can add to or change what it returns. The object literal passed
    /// to the call is read as a lower bound.
    UnrecognizedCall,
    /// The options come from a relative import or `require` whose target could
    /// not be read.
    ImportTargetUnreadable,
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
            Self::UnrecognizedCall => "unrecognized-call",
            Self::ImportTargetUnreadable => "import-target-unreadable",
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
) -> FederationRead {
    let read = read_declarations(source, location.config_path, sites);
    for declaration in &read.unread {
        result
            .config_diagnostics
            .push(super::PluginConfigDiagnostic::unreadable(
                location.config_path,
                plugin_label,
                declaration.key.name(),
                declaration.reason.token(),
            ));
    }
    read
}

/// Read and register the Federation options of one config file. Returns
/// whether the options declare a Federation key.
fn apply_from_source(
    result: &mut PluginResult,
    source: &str,
    location: &ConfigLocation<'_>,
    plugin_label: &str,
    sites: &FederationSites,
) -> bool {
    let read = extract(result, source, location, plugin_label, sites);
    apply(result, &read.config, location);
    read.declares_key
}

/// Read the Federation options of every Federation plugin call in a bundler
/// config and register what they declare.
///
/// `base` names the directories that replace the config directory when
/// resolving a relative `exposes` target and scoping a remote alias.
pub(super) fn apply_bundler_plugin_options(
    result: &mut PluginResult,
    source: &str,
    config_path: &Path,
    root: &Path,
    base: FederationBase<'_>,
    plugin_label: &str,
) {
    apply_from_source(
        result,
        source,
        &ConfigLocation {
            config_path,
            root,
            context: base.context,
            package_dir: base.package_dir,
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
    let Some(normalized) = config_parser::normalize_config_path(trimmed, base, root)
        .or_else(|| parent_relative_target(trimmed, base, root))
    else {
        return;
    };
    // An entry pattern is compiled as a glob, while a target is a literal path.
    // Bracketed route filenames are the Next.js convention, so an unescaped
    // target would both miss the exposed file and credit an unrelated one.
    let escaped = globset::escape(&normalized);
    let patterns = if super::has_source_extension(&normalized) {
        vec![escaped]
    } else {
        vec![
            format!("{escaped}.{EXPOSE_EXTENSIONS}"),
            format!("{escaped}/index.{EXPOSE_EXTENSIONS}"),
        ]
    };
    // Only a target that climbs out of the plugin root is parent-relative, so
    // only it asks the workspace prefix to resolve its `../` segments.
    let parent_relative = normalized.starts_with("../");
    for pattern in patterns {
        if parent_relative {
            result.push_parent_relative_entry_pattern(pattern);
        } else {
            result.push_entry_pattern(pattern);
        }
    }
}

/// A relative target that climbs out of the plugin root, as a path relative to
/// that root with its leading `../` segments.
///
/// A workspace package is read with its own directory as the root, so a target
/// in a sibling workspace climbs out of it while it stays inside the project.
/// The entry rule is marked parent-relative, so the workspace prefix resolves
/// the `../` segments later. A target that climbs out of the project keeps them
/// and matches no project file.
fn parent_relative_target(target: &str, base: &Path, root: &Path) -> Option<String> {
    if !target.starts_with("../") {
        return None;
    }
    let directory = base.parent().unwrap_or(root);
    let candidate = config_parser::lexical_normalize(&directory.join(target));
    let root = config_parser::lexical_normalize(root);
    let mut ancestor = root.as_path();
    let mut climbs = 0;
    while !candidate.starts_with(ancestor) {
        ancestor = ancestor.parent()?;
        climbs += 1;
    }
    let rest = candidate.strip_prefix(ancestor).ok()?;
    (climbs > 0 && !rest.as_os_str().is_empty()).then(|| {
        format!(
            "{}{}",
            "../".repeat(climbs),
            config_parser::path_to_config_string(rest)
        )
    })
}

/// What one config file declares for Module Federation.
#[derive(Debug, Default)]
struct FederationRead {
    config: FederationConfig,
    unread: Vec<UnreadDeclaration>,
    /// Whether an accepted options value declares `exposes` or `remotes`.
    declares_key: bool,
}

#[cfg(test)]
fn read(
    source: &str,
    config_path: &Path,
    sites: &FederationSites,
) -> (FederationConfig, Vec<UnreadDeclaration>) {
    let read = read_declarations(source, config_path, sites);
    (read.config, read.unread)
}

fn read_declarations(source: &str, config_path: &Path, sites: &FederationSites) -> FederationRead {
    config_parser::extract_from_source(source, config_path, |program| {
        let mut collector = FederationCallCollector::new(program, config_path);

        if sites.read_config_object {
            let mut options = ResolvedOptions::default();
            if read_config_object_options(program, config_path, &mut options) {
                collector.merge(options, true);
            }
        }
        if sites.read_plugin_calls {
            collector.visit_program(program);
        }

        Some(FederationRead {
            config: collector.config,
            unread: collector.unread,
            declares_key: collector.declares_key,
        })
    })
    .unwrap_or_default()
}

/// Read the options a standalone config exports.
///
/// The exported value goes through the options resolver first, so a wrapper
/// call is read the same way as in a plugin call. A shape the resolver does not
/// accept, such as a function that returns the options, falls back to the
/// shared config object lookup.
fn read_config_object_options(
    program: &Program<'_>,
    config_path: &Path,
    options: &mut ResolvedOptions,
) -> bool {
    if config_parser::find_module_export_expression(program)
        .is_some_and(|export| resolve_options(program, config_path, export, 0, options))
    {
        return true;
    }
    let Some(config_object) = config_parser::find_config_object(program) else {
        return false;
    };
    read_options_object(program, config_path, config_object, 0, options);
    true
}

/// Every Federation plugin call in one config program, at any position.
///
/// A bundler config holds its plugin list in a literal array, in a nested array,
/// in a variable, under a tool-specific key, or inside a hook that receives the
/// config. One walk covers all of them, and the accept gate stays the callee name
/// plus an options object that declares a Federation key.
struct FederationCallCollector<'a, 'p> {
    program: &'a Program<'a>,
    config_path: &'p Path,
    config: FederationConfig,
    unread: Vec<UnreadDeclaration>,
    declares_key: bool,
}

impl<'a, 'p> FederationCallCollector<'a, 'p> {
    fn new(program: &'a Program<'a>, config_path: &'p Path) -> Self {
        Self {
            program,
            config_path,
            config: FederationConfig::default(),
            unread: Vec::new(),
            declares_key: false,
        }
    }

    fn read_plugin_call(&mut self, callee: &Expression<'a>, arguments: &[Argument<'a>]) {
        let Some(callee_name) = federation_callee_name(callee) else {
            return;
        };
        let Some(argument) = arguments.first().and_then(Argument::as_expression) else {
            return;
        };
        let mut options = ResolvedOptions::default();
        if !resolve_options(self.program, self.config_path, argument, 0, &mut options) {
            return;
        }
        self.merge(options, callee_name != AMBIGUOUS_CALLEE);
    }

    /// Take what one options value declares.
    ///
    /// The shape gate applies to the whole value: a call whose options declare
    /// no Federation key registers nothing, so a same-named local symbol does
    /// not activate extraction. The one exception is an unread part (a spread,
    /// an import target or an unrecognized call) in the options of a
    /// `federation_specific` source, which names Module Federation beyond
    /// doubt. A spread or an import that is not readable can hold each key the
    /// readable part does not declare, so it is recorded against each of those
    /// keys. An unrecognized call can change each key it receives, so it is
    /// recorded against each key its argument declares, or against both keys
    /// when the options declare none.
    fn merge(&mut self, options: ResolvedOptions, federation_specific: bool) {
        let has_unread_part =
            options.unreadable_spread || options.unreadable_import || options.unrecognized_call;
        if options.declared.is_empty() && !(has_unread_part && federation_specific) {
            return;
        }
        self.declares_key |= !options.declared.is_empty();
        for target in options.config.exposed_targets {
            push_unique(&mut self.config.exposed_targets, target);
        }
        for package in options.config.exposed_packages {
            push_unique(&mut self.config.exposed_packages, package);
        }
        for alias in options.config.remote_aliases {
            push_unique(&mut self.config.remote_aliases, alias);
        }
        for declaration in options.unread {
            push_unique(&mut self.unread, declaration);
        }
        for key in [FederationKey::Exposes, FederationKey::Remotes] {
            let declared = options.declared.contains(&key);
            let reasons = [
                (
                    options.unrecognized_keys.contains(&key)
                        || (options.unrecognized_call && options.declared.is_empty()),
                    UnreadReason::UnrecognizedCall,
                ),
                (
                    options.unreadable_import && !declared,
                    UnreadReason::ImportTargetUnreadable,
                ),
                (options.unreadable_spread && !declared, UnreadReason::Spread),
            ];
            for (applies, reason) in reasons {
                if applies {
                    push_unique(&mut self.unread, UnreadDeclaration { key, reason });
                }
            }
        }
    }
}

/// How many steps of indirection the options resolver follows. A binding, a
/// spread, an `Object.assign` argument and an import are one step each.
const MAX_OPTIONS_DEPTH: usize = 4;

/// What one Federation options value declares, read from every object literal
/// the value resolves to.
#[derive(Debug, Default)]
struct ResolvedOptions {
    config: FederationConfig,
    unread: Vec<UnreadDeclaration>,
    /// The Federation keys the readable part declares.
    declared: Vec<FederationKey>,
    /// Whether a spread or an `Object.assign` argument did not resolve, so the
    /// value can declare more than what was read.
    unreadable_spread: bool,
    /// Whether a followed relative import or `require` target could not be
    /// read.
    unreadable_import: bool,
    /// Whether the options pass through a call that is not a known identity
    /// wrapper, so what was read is a lower bound.
    unrecognized_call: bool,
    /// The Federation keys that the argument of an unrecognized call declares.
    unrecognized_keys: Vec<FederationKey>,
}

impl ResolvedOptions {
    /// Take what a nested resolution read.
    fn absorb(&mut self, other: Self) {
        for target in other.config.exposed_targets {
            push_unique(&mut self.config.exposed_targets, target);
        }
        for package in other.config.exposed_packages {
            push_unique(&mut self.config.exposed_packages, package);
        }
        for alias in other.config.remote_aliases {
            push_unique(&mut self.config.remote_aliases, alias);
        }
        for declaration in other.unread {
            push_unique(&mut self.unread, declaration);
        }
        for key in other.declared {
            push_unique(&mut self.declared, key);
        }
        for key in other.unrecognized_keys {
            push_unique(&mut self.unrecognized_keys, key);
        }
        self.unreadable_spread |= other.unreadable_spread;
        self.unreadable_import |= other.unreadable_import;
        self.unrecognized_call |= other.unrecognized_call;
    }
}

/// Read every object literal that an options expression resolves to: an object
/// literal, a top-level binding of the same file, a relative ESM import or
/// `require`, a spread of one of these, `Object.assign(...)` over them, and the
/// argument of a wrapper call.
///
/// Returns `false` when the expression itself does not resolve. A package
/// `require` does not resolve. A relative import or `require` whose target
/// cannot be read resolves, and is recorded as unreadable.
fn resolve_options(
    program: &Program<'_>,
    path: &Path,
    expr: &Expression<'_>,
    depth: usize,
    options: &mut ResolvedOptions,
) -> bool {
    if depth > MAX_OPTIONS_DEPTH {
        return false;
    }
    match unwrap_expression(expr) {
        Expression::ObjectExpression(object) => {
            read_options_object(program, path, object, depth, options);
            true
        }
        Expression::CallExpression(call) if config_parser::is_require_call(call) => {
            resolve_required_options(path, call, depth, options)
        }
        Expression::CallExpression(call) if is_object_assign(&call.callee) => {
            for argument in &call.arguments {
                let resolved = argument.as_expression().is_some_and(|argument| {
                    resolve_options(program, path, argument, depth + 1, options)
                });
                options.unreadable_spread |= !resolved;
            }
            true
        }
        Expression::CallExpression(call) => {
            resolve_wrapped_options(program, path, unwrap_expression(expr), call, depth, options)
        }
        Expression::Identifier(identifier) => {
            resolve_options_name(program, path, &identifier.name, depth, options)
        }
        _ => false,
    }
}

/// Read the options a wrapper call receives.
///
/// A known identity wrapper passes its argument through unchanged. Any other
/// call can add to or change what it returns, so the object literal passed to
/// it is read as a lower bound and the call is recorded. A call with no
/// readable argument does not resolve.
fn resolve_wrapped_options(
    program: &Program<'_>,
    path: &Path,
    expr: &Expression<'_>,
    call: &CallExpression<'_>,
    depth: usize,
    options: &mut ResolvedOptions,
) -> bool {
    // Resolve the argument on its own, so the keys it declares are known.
    let mut received = ResolvedOptions::default();
    let resolved = call
        .arguments
        .first()
        .and_then(Argument::as_expression)
        .is_some_and(|argument| resolve_options(program, path, argument, depth + 1, &mut received))
        || config_parser::extract_object_from_expression(expr).is_some_and(|object| {
            read_options_object(program, path, object, depth + 1, &mut received);
            true
        });
    if !resolved {
        return false;
    }
    if !is_identity_wrapper(&call.callee) {
        received.unrecognized_call = true;
        received.unrecognized_keys.clone_from(&received.declared);
    }
    options.absorb(received);
    true
}

/// Whether a callee is a known identity wrapper, called by name or as a
/// member.
fn is_identity_wrapper(callee: &Expression<'_>) -> bool {
    let name = match unwrap_expression(callee) {
        Expression::Identifier(identifier) => identifier.name.as_str(),
        Expression::StaticMemberExpression(member) => member.property.name.as_str(),
        _ => return false,
    };
    IDENTITY_WRAPPERS.contains(&name)
}

/// Read the declarations of one options object literal, and follow each spread
/// in it.
fn read_options_object(
    program: &Program<'_>,
    path: &Path,
    object: &ObjectExpression<'_>,
    depth: usize,
    options: &mut ResolvedOptions,
) {
    for key in [FederationKey::Exposes, FederationKey::Remotes] {
        if config_parser::property_expr(object, key.name()).is_some() {
            push_unique(&mut options.declared, key);
        }
    }
    read_exposes(object, &mut options.config, &mut options.unread);
    read_remotes(object, &mut options.config, &mut options.unread);
    for property in &object.properties {
        if let ObjectPropertyKind::SpreadProperty(spread) = property {
            let resolved = resolve_options(program, path, &spread.argument, depth + 1, options);
            options.unreadable_spread |= !resolved;
        }
    }
}

/// Resolve a name to its options: a stable top-level binding of the same file
/// first, then a relative ESM import.
fn resolve_options_name(
    program: &Program<'_>,
    path: &Path,
    name: &str,
    depth: usize,
    options: &mut ResolvedOptions,
) -> bool {
    if let Some(init) = config_parser::find_stable_binding_init(program, name) {
        return resolve_options_init(program, path, init, depth + 1, options);
    }
    let Some((specifier, imported_name)) =
        config_parser::find_relative_import_binding(program, name)
    else {
        return false;
    };
    resolve_module_options(path, &specifier, imported_name.as_deref(), depth, options)
}

/// Record a followed import target that could not be read. The import is
/// still a resolved value: what it holds is unknown, not absent.
fn record_unreadable_import(resolved: bool, options: &mut ResolvedOptions) -> bool {
    options.unreadable_import |= !resolved;
    true
}

/// Resolve the options a relative `require('./x')` names: the value that module
/// exports as a whole. A package `require` does not resolve, and a relative
/// target that cannot be read is recorded.
fn resolve_required_options(
    path: &Path,
    call: &CallExpression<'_>,
    depth: usize,
    options: &mut ResolvedOptions,
) -> bool {
    let Some(specifier) = config_parser::get_require_source(call)
        .filter(|specifier| config_parser::is_relative_specifier(specifier))
    else {
        return false;
    };
    resolve_module_options(path, &specifier, None, depth, options)
}

/// Read the options a relative sibling module exports: under `export_name`,
/// or as the whole module when `export_name` is `None`. A target that cannot
/// be read is recorded, and the value still counts as resolved.
fn resolve_module_options(
    path: &Path,
    specifier: &str,
    export_name: Option<&str>,
    depth: usize,
    options: &mut ResolvedOptions,
) -> bool {
    let Some((module_path, source)) = config_parser::resolve_sibling_module(path, specifier) else {
        return record_unreadable_import(false, options);
    };
    let resolved = config_parser::extract_from_source(&source, &module_path, |module| {
        let init = match export_name {
            Some(name) => config_parser::find_exported_init(module, Some(name))?,
            None => config_parser::find_module_export_expression(module)?,
        };
        resolve_options_init(module, &module_path, init, depth + 1, options).then_some(())
    })
    .is_some();
    record_unreadable_import(resolved, options)
}

/// Resolve the value a binding or an export holds. Beyond the shapes of
/// [`resolve_options`], this accepts a function that returns the options.
fn resolve_options_init(
    program: &Program<'_>,
    path: &Path,
    init: &Expression<'_>,
    depth: usize,
    options: &mut ResolvedOptions,
) -> bool {
    if resolve_options(program, path, init, depth, options) {
        return true;
    }
    let Some(object) = config_parser::extract_object_from_expression(init) else {
        return false;
    };
    read_options_object(program, path, object, depth, options);
    true
}

/// Whether a callee is `Object.assign`.
fn is_object_assign(callee: &Expression<'_>) -> bool {
    matches!(
        unwrap_expression(callee),
        Expression::StaticMemberExpression(member)
            if member.property.name == "assign"
                && matches!(&member.object, Expression::Identifier(object) if object.name == "Object")
    )
}

impl<'a> Visit<'a> for FederationCallCollector<'a, '_> {
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
        // A bundler reads an element as one module request. A glob and a nested
        // array are neither a request nor a mapping, so each one is unread
        // rather than a literal path.
        let target = config_parser::expression_to_string(expr)
            .filter(|target| !super::has_glob_syntax(target));
        let Some(target) = target else {
            has_unread_element = true;
            continue;
        };
        declarations.push(KeyDeclaration::Target(target));
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

/// The Federation callee name of a call, or `None` for any other callee.
fn federation_callee_name<'a>(callee: &'a Expression<'a>) -> Option<&'a str> {
    let name = match unwrap_expression(callee) {
        Expression::Identifier(identifier) => identifier.name.as_str(),
        Expression::StaticMemberExpression(member) => member.property.name.as_str(),
        Expression::ComputedMemberExpression(member) => match unwrap_expression(&member.expression)
        {
            Expression::StringLiteral(literal) => literal.value.as_str(),
            _ => return None,
        },
        _ => return None,
    };
    FEDERATION_CALLEES.contains(&name).then_some(name)
}

fn unwrap_expression<'a>(expr: &'a Expression<'a>) -> &'a Expression<'a> {
    match expr {
        Expression::ParenthesizedExpression(paren) => unwrap_expression(&paren.expression),
        Expression::TSAsExpression(ts_as) => unwrap_expression(&ts_as.expression),
        Expression::TSSatisfiesExpression(ts_satisfies) => {
            unwrap_expression(&ts_satisfies.expression)
        }
        Expression::TSNonNullExpression(non_null) => unwrap_expression(&non_null.expression),
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

        let location = ConfigLocation {
            config_path,
            root,
            context: None,
            package_dir: None,
        };
        // The declared `always_used` pattern is matched against the
        // project-relative path without a `**/` rewrite, so it covers a root
        // config only. Credit the file that was actually read, at any depth.
        if let Some(relative) = location.relative_config_path() {
            result.always_used_files.push(globset::escape(&relative));
        }

        let declares_key = apply_from_source(
            &mut result,
            source,
            &location,
            "module-federation",
            &FederationSites {
                read_plugin_calls: false,
                read_config_object: true,
            },
        );
        if declares_key {
            result
                .referenced_dependencies
                .extend(BUILD_PLUGIN_ENABLERS.iter().map(|name| (*name).to_string()));
        }

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
    fn target_outside_the_project_root_matches_no_project_file() {
        let result = resolve(r"export default { exposes: { './Button': '../other/Button.tsx' } };");
        let patterns = entry_patterns(&result);
        assert_eq!(patterns, vec!["../other/Button.tsx".to_string()]);
        assert!(!covers(&patterns[0], "other/Button.tsx"));
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

    /// A bundler reads an `exposes` element as one module request. An element
    /// that holds glob syntax, a nested array or a non-string value is not a
    /// request, so the advisory names the key instead of a literal path.
    #[test]
    fn exposes_array_elements_that_are_not_a_request_are_reported() {
        for source in [
            r"export default { exposes: ['./src/*.tsx'] };",
            r"export default { exposes: [['./src/Button.tsx']] };",
            r"export default { exposes: [42] };",
        ] {
            let (config, declarations) = standalone(source);
            assert_eq!(config, FederationConfig::default(), "source: {source}");
            assert_eq!(
                declarations,
                unread(FederationKey::Exposes, UnreadReason::Entries),
                "source: {source}"
            );
        }
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
            FederationBase::default(),
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

    /// Two calls at two positions that share one options `const` register one
    /// target.
    #[test]
    fn two_calls_that_share_one_options_const_register_one_target() {
        let (config, _) = bundler(
            r"
            const mfConfig = { exposes: { './Button': './src/Button.tsx' } };
            module.exports = {
                plugins: [new ModuleFederationPlugin(mfConfig)],
                webpack(config) {
                    config.plugins.push(new ModuleFederationPlugin(mfConfig));
                    return config;
                },
            };
            ",
        );
        assert_eq!(config.exposed_targets, vec!["./src/Button.tsx".to_string()]);
    }

    /// A `const` inside the `webpack(config)` hook shadows the top-level `const`
    /// of the same name, so the top-level object is not the object at the call.
    #[test]
    fn a_shadowed_options_name_is_not_read() {
        let (config, computed) = bundler(
            r"
            const mfConfig = { exposes: { './Top': './src/Top.tsx' } };
            module.exports = {
                webpack(config) {
                    const mfConfig = { exposes: { './Hook': './src/Hook.tsx' } };
                    config.plugins.push(new ModuleFederationPlugin(mfConfig));
                    return config;
                },
            };
            ",
        );
        assert_eq!(config, FederationConfig::default());
        assert!(computed.is_empty(), "got {computed:?}");
    }

    /// A parameter that carries the options is a different binding than the
    /// top-level `const` of the same name.
    #[test]
    fn an_options_parameter_is_not_read() {
        let (config, computed) = bundler(
            r"
            const options = { exposes: { './Top': './src/Top.tsx' } };
            function make(options) {
                return new ModuleFederationPlugin(options);
            }
            module.exports = { plugins: [make(buildOptions())] };
            ",
        );
        assert_eq!(config, FederationConfig::default());
        assert!(computed.is_empty(), "got {computed:?}");
    }

    /// A binding that the config writes to does not hold its initializer at the
    /// call, so a reassignment and a member write both stop the read.
    #[test]
    fn options_that_the_config_writes_to_are_not_read() {
        for source in [
            r"
            let mfConfig = { exposes: { './A': './src/A.tsx' } };
            mfConfig = buildConfig();
            module.exports = { plugins: [new ModuleFederationPlugin(mfConfig)] };
            ",
            r"
            const mfConfig = { exposes: { './Old': './src/Old.tsx' } };
            delete mfConfig.exposes['./Old'];
            module.exports = { plugins: [new ModuleFederationPlugin(mfConfig)] };
            ",
            r"
            const mfConfig = { exposes: { './A': './src/A.tsx' } };
            mfConfig.exposes['./B'] = './src/B.tsx';
            module.exports = { plugins: [new ModuleFederationPlugin(mfConfig)] };
            ",
        ] {
            let (config, computed) = bundler(source);
            assert_eq!(config, FederationConfig::default(), "source: {source}");
            assert!(computed.is_empty(), "source: {source}");
        }
    }

    /// A `var` is function scoped and hoisted, so its initializer is not the
    /// value at the call.
    #[test]
    fn options_bound_by_var_are_not_read() {
        let (config, computed) = bundler(
            r"
            var mfConfig = { exposes: { './Button': './src/Button.tsx' } };
            module.exports = { plugins: [new ModuleFederationPlugin(mfConfig)] };
            ",
        );
        assert_eq!(config, FederationConfig::default());
        assert!(computed.is_empty(), "got {computed:?}");
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
            r"module.exports = { plugins: [federation({ ...opts })] };",
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

    /// Read a bundler config from an empty temp directory, so a relative
    /// `require` or import resolves against no file of the test process.
    fn bundler_in_temp_dir(source: &str) -> (FederationConfig, Vec<UnreadDeclaration>) {
        let dir = tempfile::tempdir().expect("temp dir");
        read(
            source,
            &dir.path().join("webpack.config.js"),
            &FederationSites {
                read_plugin_calls: true,
                read_config_object: false,
            },
        )
    }

    fn exposed(target: &str) -> FederationConfig {
        FederationConfig {
            exposed_targets: vec![target.to_string()],
            ..FederationConfig::default()
        }
    }

    #[test]
    fn exported_options_const_is_read() {
        let (config, computed) = bundler(
            r"
            export const mfConfig = { exposes: { './Button': './src/Button.tsx' } };
            export default { plugins: [new ModuleFederationPlugin(mfConfig)] };
            ",
        );
        assert_eq!(config, exposed("./src/Button.tsx"));
        assert!(computed.is_empty(), "got {computed:?}");
    }

    #[test]
    fn non_null_options_and_computed_member_callee_are_read() {
        let (config, computed) = read(
            r"
            const mfConfig = { exposes: { './Button': './src/Button.tsx' } };
            export default { plugins: [new ModuleFederationPlugin(mfConfig!)] };
            ",
            Path::new("webpack.config.ts"),
            &FederationSites {
                read_plugin_calls: true,
                read_config_object: false,
            },
        );
        assert_eq!(config, exposed("./src/Button.tsx"));
        assert!(computed.is_empty(), "got {computed:?}");

        let (config, computed) = bundler(
            r"
            const container = require('@module-federation/enhanced');
            module.exports = {
                plugins: [new container['ModuleFederationPlugin']({
                    exposes: { './Button': './src/Button.tsx' },
                })],
            };
            ",
        );
        assert_eq!(config, exposed("./src/Button.tsx"));
        assert!(computed.is_empty(), "got {computed:?}");
    }

    #[test]
    fn spread_and_object_assign_over_local_bindings_are_read() {
        for source in [
            r"
            const base = { exposes: { './Button': './src/Button.tsx' } };
            const mfConfig = { ...base };
            module.exports = { plugins: [new ModuleFederationPlugin(mfConfig)] };
            ",
            r"
            const base = { exposes: { './Button': './src/Button.tsx' } };
            module.exports = { plugins: [new ModuleFederationPlugin(Object.assign({}, base))] };
            ",
            r"
            const base = createModuleFederationConfig({ exposes: { './Button': './src/Button.tsx' } });
            module.exports = { plugins: [new ModuleFederationPlugin({ name: 'app', ...base })] };
            ",
        ] {
            let (config, computed) = bundler(source);
            assert_eq!(config, exposed("./src/Button.tsx"), "source: {source}");
            assert!(computed.is_empty(), "source: {source}");
        }
    }

    #[test]
    fn an_unreadable_spread_is_recorded_against_each_undeclared_key() {
        let (config, computed) = bundler_in_temp_dir(
            r"
            module.exports = {
                plugins: [new ModuleFederationPlugin({
                    ...getShared(),
                    exposes: { './Button': './src/Button.tsx' },
                })],
            };
            ",
        );
        assert_eq!(config, exposed("./src/Button.tsx"));
        assert_eq!(
            computed,
            unread(FederationKey::Remotes, UnreadReason::Spread)
        );

        for source in [
            r"module.exports = { plugins: [new ModuleFederationPlugin({ ...shared })] };",
            r"module.exports = { plugins: [new ModuleFederationPlugin(Object.assign({}, shared))] };",
        ] {
            let (config, computed) = bundler_in_temp_dir(source);
            assert_eq!(config, FederationConfig::default(), "source: {source}");
            assert_eq!(
                computed,
                vec![
                    UnreadDeclaration {
                        key: FederationKey::Exposes,
                        reason: UnreadReason::Spread,
                    },
                    UnreadDeclaration {
                        key: FederationKey::Remotes,
                        reason: UnreadReason::Spread,
                    },
                ],
                "source: {source}"
            );
        }
    }

    #[test]
    fn a_spread_cycle_ends_as_an_unreadable_spread() {
        let (config, computed) = bundler(
            r"
            const a = { ...b, exposes: { './Button': './src/Button.tsx' } };
            const b = { ...a };
            module.exports = { plugins: [new ModuleFederationPlugin(a)] };
            ",
        );
        assert_eq!(config, exposed("./src/Button.tsx"));
        assert_eq!(
            computed,
            unread(FederationKey::Remotes, UnreadReason::Spread)
        );
    }

    #[test]
    fn a_package_require_is_silent() {
        let (config, computed) = bundler(
            r"
            const mfConfig = require('shared-federation-config');
            module.exports = { plugins: [new ModuleFederationPlugin(mfConfig)] };
            ",
        );
        assert_eq!(config, FederationConfig::default());
        assert!(computed.is_empty(), "got {computed:?}");
    }

    #[test]
    fn options_from_a_relative_require_are_read() {
        let dir = tempfile::tempdir().expect("temp dir");
        std::fs::write(
            dir.path().join("mf.config.js"),
            r"
            const options = { exposes: { './Button': './src/Button.tsx' } };
            module.exports = options;
            ",
        )
        .expect("write sibling config");
        let (config, computed) = read(
            r"
            const mfConfig = require('./mf.config');
            module.exports = {
                plugins: [
                    new ModuleFederationPlugin(mfConfig),
                    new ModuleFederationPlugin({ ...require('./mf.config.js') }),
                ],
            };
            ",
            &dir.path().join("webpack.config.js"),
            &FederationSites {
                read_plugin_calls: true,
                read_config_object: false,
            },
        );
        assert_eq!(config, exposed("./src/Button.tsx"));
        assert!(computed.is_empty(), "got {computed:?}");
    }

    #[test]
    fn options_from_a_relative_import_are_read() {
        let dir = tempfile::tempdir().expect("temp dir");
        let config_path = dir.path().join("webpack.config.ts");
        std::fs::write(
            dir.path().join("mf.config.ts"),
            r"
            const base = { exposes: { './Button': './src/Button.tsx' } };
            export const mfConfig = { ...base };
            export default createModuleFederationConfig({ remotes: { checkout: 'checkout@x' } });
            ",
        )
        .expect("write sibling config");
        let (config, computed) = read(
            r"
            import remote, { mfConfig } from './mf.config';
            export default {
                plugins: [
                    new ModuleFederationPlugin(mfConfig),
                    new ModuleFederationPlugin(remote),
                ],
            };
            ",
            &config_path,
            &FederationSites {
                read_plugin_calls: true,
                read_config_object: false,
            },
        );
        assert_eq!(
            config,
            FederationConfig {
                exposed_targets: vec!["./src/Button.tsx".to_string()],
                remote_aliases: vec!["checkout".to_string()],
                ..FederationConfig::default()
            }
        );
        assert!(computed.is_empty(), "got {computed:?}");
    }

    #[test]
    fn a_standalone_config_reads_a_spread_of_a_local_binding() {
        let (config, computed) = standalone(
            r"
            const base = { exposes: { './Button': './src/Button.tsx' } };
            export default { name: 'app', ...base };
            ",
        );
        assert_eq!(config, exposed("./src/Button.tsx"));
        assert!(computed.is_empty(), "got {computed:?}");
    }

    fn unread_both(reason: UnreadReason) -> Vec<UnreadDeclaration> {
        vec![
            UnreadDeclaration {
                key: FederationKey::Exposes,
                reason,
            },
            UnreadDeclaration {
                key: FederationKey::Remotes,
                reason,
            },
        ]
    }

    /// A known identity wrapper passes its argument through, so it is read
    /// with no diagnostic, inline and bound to a name alike.
    #[test]
    fn identity_wrappers_are_read_without_a_diagnostic() {
        for source in [
            r"module.exports = { plugins: [new ModuleFederationPlugin(createModuleFederationConfig({ exposes: { './Button': './src/Button.tsx' } }))] };",
            r"module.exports = { plugins: [new ModuleFederationPlugin(defineConfig({ exposes: { './Button': './src/Button.tsx' } }))] };",
            r"
            const mf = createModuleFederationConfig({ exposes: { './Button': './src/Button.tsx' } });
            module.exports = { plugins: [new ModuleFederationPlugin(mf)] };
            ",
            r"
            const base = { exposes: { './Button': './src/Button.tsx' } };
            module.exports = { plugins: [new ModuleFederationPlugin(mf.createModuleFederationConfig(base))] };
            ",
        ] {
            let (config, computed) = bundler(source);
            assert_eq!(config, exposed("./src/Button.tsx"), "source: {source}");
            assert!(computed.is_empty(), "source: {source}, got {computed:?}");
        }
        for source in [
            r"export default createModuleFederationConfig({ exposes: { './Button': './src/Button.tsx' } });",
            r"export default defineConfig({ exposes: { './Button': './src/Button.tsx' } });",
        ] {
            let (config, computed) = standalone(source);
            assert_eq!(config, exposed("./src/Button.tsx"), "source: {source}");
            assert!(computed.is_empty(), "source: {source}, got {computed:?}");
        }
    }

    /// A call that is not a known wrapper can add or change what it returns.
    /// The object literal passed to it stays credited as a lower bound, and the
    /// call is recorded against each key that literal declares. The inline
    /// form and the bound form give the same result.
    #[test]
    fn an_unrecognized_call_keeps_the_inner_literal_and_is_recorded() {
        let expected = unread(FederationKey::Exposes, UnreadReason::UnrecognizedCall);
        for source in [
            r"module.exports = { plugins: [new ModuleFederationPlugin(federationConfig({ name: 'app', exposes: { './Button': './src/Button.tsx' } }))] };",
            r"
            const mf = federationConfig({ name: 'app', exposes: { './Button': './src/Button.tsx' } });
            module.exports = { plugins: [new ModuleFederationPlugin(mf)] };
            ",
            r"
            const base = { name: 'app', exposes: { './Button': './src/Button.tsx' } };
            module.exports = { plugins: [new ModuleFederationPlugin(withShared(base))] };
            ",
        ] {
            let (config, computed) = bundler(source);
            assert_eq!(config, exposed("./src/Button.tsx"), "source: {source}");
            assert_eq!(computed, expected, "source: {source}");
        }
        let (config, computed) = standalone(
            r"export default federationConfig({ exposes: { './Button': './src/Button.tsx' } });",
        );
        assert_eq!(config, exposed("./src/Button.tsx"));
        assert_eq!(computed, expected);

        // A literal that declares no key still names Module Federation beyond
        // doubt under a specific callee, so the call is recorded against both
        // keys. The generic `federation` callee stays inert.
        let (config, computed) = bundler(
            r"module.exports = { plugins: [new ModuleFederationPlugin(withShared({ name: 'app' }))] };",
        );
        assert_eq!(config, FederationConfig::default());
        assert_eq!(computed, unread_both(UnreadReason::UnrecognizedCall));
        let (config, computed) =
            bundler(r"export default { plugins: [federation(withShared({ name: 'app' }))] };");
        assert_eq!(config, FederationConfig::default());
        assert!(computed.is_empty(), "got {computed:?}");
    }

    /// Only the keys that come from the unrecognized call are recorded. A key
    /// declared by a literal outside the call is read in full.
    #[test]
    fn an_unrecognized_call_records_only_the_keys_it_receives() {
        let (config, computed) = bundler(
            r"
            module.exports = { plugins: [new ModuleFederationPlugin({
                ...withShared({ exposes: { './Button': './src/Button.tsx' } }),
                remotes: { checkout: 'checkout@x' },
            })] };
            ",
        );
        assert_eq!(
            config,
            FederationConfig {
                exposed_targets: vec!["./src/Button.tsx".to_string()],
                remote_aliases: vec!["checkout".to_string()],
                ..FederationConfig::default()
            }
        );
        assert_eq!(
            computed,
            unread(FederationKey::Exposes, UnreadReason::UnrecognizedCall)
        );
    }

    /// A followed relative import or `require` whose target cannot be read is
    /// recorded. An argument with no relative binding records nothing.
    #[test]
    fn an_unreadable_import_target_is_recorded() {
        let dir = tempfile::tempdir().expect("temp dir");
        std::fs::write(
            dir.path().join("mf.options.js"),
            "const build = require('./build');\nmodule.exports = build();\n",
        )
        .expect("write sibling config");
        let read_in_dir = |source: &str| {
            read(
                source,
                &dir.path().join("webpack.config.js"),
                &FederationSites {
                    read_plugin_calls: true,
                    read_config_object: false,
                },
            )
        };
        for source in [
            r"module.exports = { plugins: [new ModuleFederationPlugin(require('./missing.options'))] };",
            r"module.exports = { plugins: [new ModuleFederationPlugin(require('./mf.options'))] };",
            r"
            const mf = require('./mf.options');
            module.exports = { plugins: [new ModuleFederationPlugin(mf)] };
            ",
            r"
            import mf from './missing.options';
            export default { plugins: [new ModuleFederationPlugin(mf)] };
            ",
        ] {
            let (config, computed) = read_in_dir(source);
            assert_eq!(config, FederationConfig::default(), "source: {source}");
            assert_eq!(
                computed,
                unread_both(UnreadReason::ImportTargetUnreadable),
                "source: {source}"
            );
        }

        // A readable key beside an unreadable spread of an import records the
        // import against the key it does not declare.
        let (config, computed) = read_in_dir(
            r"
            module.exports = { plugins: [new ModuleFederationPlugin({
                ...require('./missing.options'),
                exposes: { './Button': './src/Button.tsx' },
            })] };
            ",
        );
        assert_eq!(config, exposed("./src/Button.tsx"));
        assert_eq!(
            computed,
            unread(FederationKey::Remotes, UnreadReason::ImportTargetUnreadable)
        );

        for source in [
            r"module.exports = { plugins: [new ModuleFederationPlugin(require('shared-federation-options'))] };",
            r"module.exports = { plugins: [new ModuleFederationPlugin(options)] };",
            r"module.exports = { plugins: [federation(require('./missing.options'))] };",
        ] {
            let (config, computed) = read_in_dir(source);
            assert_eq!(config, FederationConfig::default(), "source: {source}");
            assert!(computed.is_empty(), "source: {source}, got {computed:?}");
        }
    }

    #[test]
    fn the_new_reasons_carry_their_own_tokens() {
        assert_eq!(UnreadReason::UnrecognizedCall.token(), "unrecognized-call");
        assert_eq!(
            UnreadReason::ImportTargetUnreadable.token(),
            "import-target-unreadable"
        );
    }

    /// A standalone config that declares a Federation key credits the build
    /// plugin packages, because no bundler config imports them. The runtime
    /// package is imported by application code and is never credited here.
    #[test]
    fn a_standalone_config_that_declares_a_key_credits_the_build_plugins() {
        let result = resolve(
            r"module.exports = { name: 'app', exposes: { './Button': './src/Button.tsx' } };",
        );
        for package in [
            "@module-federation/enhanced",
            "@module-federation/rsbuild-plugin",
            "@module-federation/vite",
        ] {
            assert!(
                result
                    .referenced_dependencies
                    .iter()
                    .any(|dep| dep == package),
                "{package} is credited, got {:?}",
                result.referenced_dependencies
            );
        }
        assert!(
            !result
                .referenced_dependencies
                .iter()
                .any(|dep| dep == "@module-federation/runtime"),
            "the runtime is not credited, got {:?}",
            result.referenced_dependencies
        );

        let result = resolve(r"module.exports = { remotes: { checkout: 'checkout@x' } };");
        assert!(
            result
                .referenced_dependencies
                .iter()
                .any(|dep| dep == "@module-federation/enhanced"),
            "a remotes key credits too, got {:?}",
            result.referenced_dependencies
        );

        let result = resolve(r"module.exports = { name: 'app', shared: ['react'] };");
        assert!(
            result.referenced_dependencies.is_empty(),
            "no Federation key, no credit, got {:?}",
            result.referenced_dependencies
        );
    }

    /// A target in a sibling directory of the plugin root keeps its parent
    /// segments. The workspace prefix resolves them later, so a target in a
    /// sibling workspace is credited and a target outside the project matches
    /// no file.
    #[test]
    fn a_target_outside_the_plugin_root_keeps_its_parent_segments() {
        let result = ModuleFederationPlugin.resolve_config(
            Path::new("/project/packages/app/webpack.config.js"),
            r"export default { exposes: { './Thing': '../shared/src/Thing.tsx', './Lib': '../shared/src/lib' } };",
            Path::new("/project/packages/app"),
        );
        let patterns = entry_patterns(&result);
        assert!(
            patterns.contains(&"../shared/src/Thing.tsx".to_string()),
            "got {patterns:?}"
        );
        assert!(
            patterns
                .iter()
                .any(|pattern| covers(pattern, "../shared/src/lib/index.ts")),
            "got {patterns:?}"
        );
    }
}
