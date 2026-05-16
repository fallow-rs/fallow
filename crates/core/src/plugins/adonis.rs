//! `AdonisJS` backend framework plugin.
//!
//! Detects AdonisJS v5/v6/v7 projects and marks conventional folders as entry
//! points: controllers, models, middleware, validators, services, providers,
//! preloads (`start/`), commands, configs, contracts, database migrations /
//! seeders / factories.
//!
//! The plugin lists patterns from all three supported majors as a superset.
//! v5 ships PascalCase folders (`app/Controllers/Http/`), v6 and v7 ship
//! snake_case (`app/controllers/`); since the two layouts never coexist in a
//! real project, the unmatched globs are inert and add no false positives.
//!
//! `@adonisjs/framework` (v4) is intentionally NOT covered here. v4 has a
//! distinct enabler, layout, and module type (CJS) and lives in a separate
//! plugin (`adonis4.rs`), kept off the upstream branch.
//!
//! IoC virtual imports (`@ioc:Adonis/Core/Route`, `@ioc:Adonis/Lucid/...`) are
//! a v5 convention removed in v6+. They are runtime-resolved through the
//! AdonisJS container and never reach the filesystem, so we suppress them via
//! `virtual_module_prefixes` to avoid false `unlisted-dependency` reports.
//!
//! The plugin parses `.adonisrc.json` (v5) to discover dynamic entries
//! registered via `preloads` / `providers` / `commands`, project-specific
//! path aliases (`aliases`), and runtime-referenced asset files (`metaFiles`,
//! `types`). v6 / v7 use `adonisrc.ts` instead and are handled by a separate
//! TS branch (added in a follow-up).

use std::path::Path;

use super::config_parser;
use super::{PathRule, Plugin, PluginResult};

const ENABLERS: &[&str] = &["@adonisjs/core"];

const ENTRY_PATTERNS: &[&str] = &[
    // Bootstrap files common to v5 / v6 / v7
    "server.{ts,js}",
    "ace",
    "ace.{ts,js}",
    // v6 / v7 ship the bootstrap files under bin/ (server, console, test, ...)
    "bin/**/*.{ts,js}",
    // Preload files registered via .adonisrc.json / adonisrc.ts
    "start/**/*.{ts,js}",
    // Application providers (declared by string in v5 rc, by lazy import in v6/v7 rc)
    "providers/**/*.{ts,js}",
    // Custom ace commands (referenced by directory in rc files)
    "commands/**/*.{ts,js}",
    // Auto-loaded configuration modules
    "config/**/*.{ts,js}",
    // TypeScript ambient declarations (loaded via tsconfig "types" or rc "types")
    "contracts/**/*.ts",
    // Lucid database artifacts — discovered by ace at runtime, not imported
    "database/migrations/**/*.{ts,js}",
    "database/seeders/**/*.{ts,js}",
    "database/factories/**/*.{ts,js}",
    // v5 layout (PascalCase folders). Inert for v6/v7 projects.
    "app/Controllers/Http/**/*.ts",
    "app/Controllers/Ws/**/*.ts",
    "app/Models/**/*.ts",
    "app/Middleware/**/*.ts",
    "app/Validators/**/*.ts",
    "app/Exceptions/**/*.ts",
    "app/Mailers/**/*.ts",
    "app/Listeners/**/*.ts",
    "app/Services/**/*.ts",
    "app/Repositories/**/*.ts",
    "app/Strategies/**/*.ts",
    "app/Helpers/**/*.{ts,js}",
    // v6 / v7 layout (snake_case folders). Inert for v5 projects.
    "app/controllers/**/*.ts",
    "app/models/**/*.ts",
    "app/middleware/**/*.ts",
    "app/validators/**/*.ts",
    "app/exceptions/**/*.ts",
    "app/mails/**/*.ts",
    "app/listeners/**/*.ts",
    "app/services/**/*.ts",
    "app/repositories/**/*.ts",
    "app/helpers/**/*.ts",
];

const CONFIG_PATTERNS: &[&str] = &[
    // v5 rc file (JSON)
    ".adonisrc.json",
    // v6 / v7 rc file (TS canonical, JS variant accepted by some toolchains)
    "adonisrc.ts",
    "adonisrc.js",
];

const ALWAYS_USED: &[&str] = &[
    // v5 rc file
    ".adonisrc.json",
    // v6 / v7 rc file
    "adonisrc.ts",
    "adonisrc.js",
    // v5 ace command manifest committed to source
    "ace-manifest.json",
    // Project tsconfig — always referenced indirectly
    "tsconfig.json",
    // Typed environment loader (referenced by ignitor at boot)
    "env.ts",
    // Japa test bootstrap (v5)
    "japaFile.ts",
];

// `@ioc:` is the v5 IoC container prefix. Imports like
// `@ioc:Adonis/Core/Route` resolve through the runtime container, not via the
// filesystem. Listing the prefix here prevents false `unlisted-dependency`
// reports without affecting v6 / v7 projects (they don't use this prefix).
const VIRTUAL_MODULE_PREFIXES: &[&str] = &["@ioc:"];

const TOOLING_DEPENDENCIES: &[&str] = &[
    // Core framework + companion modules autoloaded via IoC
    "@adonisjs/core",
    "@adonisjs/application",
    "@adonisjs/ace",
    "@adonisjs/assembler",
    "@adonisjs/repl",
    "@adonisjs/fold",
    "@adonisjs/config",
    "@adonisjs/env",
    "@adonisjs/encryption",
    "@adonisjs/hash",
    "@adonisjs/events",
    "@adonisjs/logger",
    "@adonisjs/http-server",
    "@adonisjs/bodyparser",
    "@adonisjs/static",
    "@adonisjs/health",
    // First-party packages registered through rc providers
    "@adonisjs/lucid",
    "@adonisjs/auth",
    "@adonisjs/session",
    "@adonisjs/shield",
    "@adonisjs/view",
    "@adonisjs/i18n",
    "@adonisjs/mail",
    "@adonisjs/redis",
    "@adonisjs/bouncer",
    "@adonisjs/limiter",
    "@adonisjs/drive",
    "@adonisjs/validator",
    "@adonisjs/http-transformers",
    // v6/v7 standard validator
    "@vinejs/vine",
    // v5 tsconfig preset and common Sentry integration
    "adonis-preset-ts",
    "adonis5-sentry",
    // v7 encryption backend
    "@boringnode/encryption",
    // peerDependencies declared by @adonisjs/core (v6/v7)
    "argon2",
    "bcrypt",
    "edge.js",
    "pino-pretty",
    "youch",
    // Optional hash driver users may swap in
    "phc-bcrypt",
    // Decorator metadata used by Lucid models and validators
    "reflect-metadata",
];

/// Keys in `.adonisrc.json` that carry an array of file paths or package names.
/// Each entry can be a plain string or an object of the form
/// `{ file: "./start/routes", environment: ["web"] }`.
const V5_PATH_ARRAY_KEYS: &[&str] = &["preloads", "providers", "commands", "aceProviders"];

/// Convert a local rc entry (path without extension) into entry-pattern globs.
///
/// Adonis rc files reference files by extension-less paths (e.g.
/// `"./start/routes"`) and folders the same way (e.g. `"./commands"`). Static
/// analysis cannot reliably tell file form from folder form without touching
/// the filesystem, so we emit both globs: the unmatched one is inert.
fn local_path_to_entry_patterns(raw: &str) -> Vec<String> {
    let stripped = raw.strip_prefix("./").unwrap_or(raw);
    let no_ext = stripped
        .strip_suffix(".ts")
        .or_else(|| stripped.strip_suffix(".js"))
        .unwrap_or(stripped)
        .trim_end_matches('/');
    if no_ext.is_empty() {
        return Vec::new();
    }
    vec![
        format!("{no_ext}.{{ts,js}}"),
        format!("{no_ext}/**/*.{{ts,js}}"),
    ]
}

/// Returns true when the rc string refers to a path inside the project
/// (relative or absolute) rather than to an npm package specifier.
fn is_local_rc_path(spec: &str) -> bool {
    spec.starts_with("./") || spec.starts_with("../") || spec.starts_with('/')
}

/// Collect string entries from an rc array field, supporting both the plain
/// string form and the `{ file: "..." }` object form used by v5.
fn rc_path_entries(source: &str, config_path: &Path, key: &str) -> Vec<String> {
    let mut entries = config_parser::extract_config_string_array(source, config_path, &[key]);
    entries.extend(config_parser::extract_config_array_object_strings(
        source,
        config_path,
        &[key],
        "file",
    ));
    entries
}

/// Apply `.adonisrc.json` entries to the plugin result.
///
/// v5 rc-driven inputs:
/// - `preloads`, `providers`, `commands`, `aceProviders`: mixed array of local
///   paths and external package specifiers. Locals become entry patterns;
///   externals become referenced dependencies (so `@adonisjs/lucid` listed as
///   a provider doesn't get flagged as unused).
/// - `aliases`: `{ App: "app", Config: "config", ... }`. Each pair becomes a
///   resolver path-alias entry (`App/` → `app/`). We read these even though
///   `tsconfig.json#paths` may already mirror them — projects sometimes
///   declare aliases in the rc file only, and the tsconfig chain can be
///   broken (which degrades oxc-resolver silently). Reading both sources
///   keeps resolution consistent.
/// - `metaFiles[].pattern`: globs of non-TS assets referenced at runtime
///   (e.g. `resources/views/**/*.edge`, `newrelic.js`). Surfaced as
///   `always_used_files` so they aren't flagged as orphans.
/// - `types`: extra `.ts` declaration files loaded at boot.
fn resolve_v5_adonisrc(config_path: &Path, source: &str) -> PluginResult {
    let mut result = PluginResult::default();

    for key in V5_PATH_ARRAY_KEYS {
        for entry in rc_path_entries(source, config_path, key) {
            if is_local_rc_path(&entry) {
                for glob in local_path_to_entry_patterns(&entry) {
                    result.entry_patterns.push(PathRule::new(glob));
                }
            } else {
                let pkg = crate::resolve::extract_package_name(&entry);
                if !pkg.is_empty() {
                    result.referenced_dependencies.push(pkg);
                }
            }
        }
    }

    for (find, replacement) in
        config_parser::extract_config_aliases(source, config_path, &["aliases"])
    {
        let prefix = if find.ends_with('/') {
            find
        } else {
            format!("{find}/")
        };
        let stripped = replacement.trim_start_matches("./").trim_end_matches('/');
        let target = if stripped.is_empty() {
            String::from("./")
        } else {
            format!("{stripped}/")
        };
        result.path_aliases.push((prefix, target));
    }

    for pattern in config_parser::extract_config_array_object_strings(
        source,
        config_path,
        &["metaFiles"],
        "pattern",
    ) {
        result.always_used_files.push(pattern);
    }

    for raw in config_parser::extract_config_string_array(source, config_path, &["types"]) {
        let normalized = raw.trim_start_matches("./").to_string();
        if !normalized.is_empty() {
            result.always_used_files.push(normalized);
        }
    }

    result
}

/// Top-level keys in `adonisrc.ts` whose array elements register modules via
/// lazy imports. v6 / v7 use `() => import('SPEC')` for `commands`,
/// `providers`, and `preloads` (and accept
/// `{ file: () => import('SPEC'), environment: [...] }` for
/// environment-scoped entries).
const V6_LAZY_IMPORT_ARRAY_KEYS: &[&str] = &["commands", "providers", "preloads"];

/// Nested paths in `adonisrc.ts` whose array elements register modules via
/// lazy imports. Adonis assembler hooks (`hooks.onBuildStarting`,
/// `hooks.onBuildCompleted`, `hooks.onDevServerStarted`,
/// `hooks.onSourceFileChanged`) accept the same `() => import('SPEC')` shape
/// as the top-level arrays. Specs typically reference external packages
/// (`@adonisjs/vite/build_hook`) or project-local hook modules
/// (`./hooks/on_build_starting`).
const V6_LAZY_IMPORT_NESTED_ARRAY_PATHS: &[&[&str]] = &[
    &["hooks", "onBuildStarting"],
    &["hooks", "onBuildCompleted"],
    &["hooks", "onDevServerStarted"],
    &["hooks", "onSourceFileChanged"],
];

/// Apply `adonisrc.ts` entries to the plugin result.
///
/// v6 / v7 rc-driven inputs:
/// - `commands`, `providers`, `preloads`: arrays of arrow functions of the
///   form `() => import('SPEC')` (or `{ file: () => import('SPEC') }` for
///   the environment-scoped object variant). External specs (e.g.
///   `@adonisjs/core/commands`) become referenced dependencies; Node subpath
///   specs (`#start/routes`) and project-relative specs map to entry
///   patterns. Node subpath resolution itself is set up below via
///   `package.json#imports`.
/// - `hooks.onBuildStarting`, `hooks.onBuildCompleted`,
///   `hooks.onDevServerStarted`, `hooks.onSourceFileChanged`: Adonis
///   assembler hooks declared as arrays of the same thunk shape. Routed
///   through the same `classify_v6_specifier` so external hook packages
///   (`@adonisjs/vite/build_hook`) stay as referenced deps and local hook
///   modules (`./hooks/on_build_starting`) get entry patterns.
/// - `metaFiles[].pattern`: runtime-referenced asset globs (e.g. Edge views,
///   instrumentation files). Surfaced as `always_used_files`.
/// - `directories`: project-level overrides for where the framework looks for
///   things like resolvers, controllers, services. Whatever directory is
///   listed must stay alive as an entry pattern. Default directories already
///   live in the static `ENTRY_PATTERNS`; this picks up project-specific
///   additions (e.g. the FriendsOfAdonis graphql playground sets
///   `resolvers: 'app/graphql/resolvers'`).
/// - `package.json#imports`: Node subpath imports declared at the project
///   root. v6 / v7 projects use these as their primary alias mechanism
///   (replacing v5's rc-level `aliases`). The default scaffold ships a
///   conventional set (`#controllers/*` → `./app/controllers/*.js`, etc.)
///   but projects routinely add custom entries. Reading the project's
///   `package.json` is therefore the authoritative source: it adapts to
///   whatever mapping the project actually uses without us hardcoding
///   conventional layouts that may diverge.
fn resolve_v6_adonisrc(config_path: &Path, source: &str, root: &Path) -> PluginResult {
    let mut result = PluginResult::default();

    for key in V6_LAZY_IMPORT_ARRAY_KEYS {
        for spec in config_parser::extract_lazy_imports_in_array(source, config_path, &[key]) {
            classify_v6_specifier(&spec, &mut result);
        }
    }

    for path in V6_LAZY_IMPORT_NESTED_ARRAY_PATHS {
        for spec in config_parser::extract_lazy_imports_in_array(source, config_path, path) {
            classify_v6_specifier(&spec, &mut result);
        }
    }

    let directory_keys =
        config_parser::extract_config_object_keys(source, config_path, &["directories"]);
    for key in &directory_keys {
        let Some(dir_path) =
            config_parser::extract_config_string(source, config_path, &["directories", key])
        else {
            continue;
        };
        let trimmed = dir_path.trim_start_matches("./").trim_end_matches('/');
        if !trimmed.is_empty() {
            result
                .entry_patterns
                .push(PathRule::new(format!("{trimmed}/**/*.{{ts,js}}")));
        }
    }

    for pattern in config_parser::extract_config_array_object_strings(
        source,
        config_path,
        &["metaFiles"],
        "pattern",
    ) {
        result.always_used_files.push(pattern);
    }

    if let Some(pairs) = read_subpath_imports(root) {
        for (find, replacement) in pairs {
            if let Some(alias) = subpath_import_to_path_alias(&find, &replacement) {
                result.path_aliases.push(alias);
            }
        }
    }

    result
}

/// Route a single v6 lazy-import specifier to the correct result bucket.
///
/// Three shapes are expected:
/// - `#alias/path` (Node subpath import): the actual file is reached via
///   `package.json#imports` substitution, but the static patterns above and
///   the path_aliases set up below already cover it. Skipped here to avoid
///   emitting bogus entry patterns derived from the alias key itself.
/// - `./path` or `../path` (relative): project file → entry patterns.
/// - bare specifier (e.g. `@adonisjs/core/commands`): external package →
///   referenced dependency so the package is not flagged unused.
fn classify_v6_specifier(spec: &str, result: &mut PluginResult) {
    if spec.starts_with('#') {
        return;
    }
    if is_local_rc_path(spec) {
        for glob in local_path_to_entry_patterns(spec) {
            result.entry_patterns.push(PathRule::new(glob));
        }
        return;
    }
    let pkg = crate::resolve::extract_package_name(spec);
    if !pkg.is_empty() {
        result.referenced_dependencies.push(pkg);
    }
}

/// Convert a Node subpath-imports entry into a fallow path-alias pair.
///
/// Examples:
/// - `"#controllers/*"` → `"./app/controllers/*.js"`
///   becomes `("#controllers/", "app/controllers/")`.
/// - `"#start/routes"` (no `*`) → ignored: bare-key imports do not map cleanly
///   to a prefix-substitution alias, so they fall back to static patterns.
fn subpath_import_to_path_alias(key: &str, value: &str) -> Option<(String, String)> {
    let prefix = key.strip_suffix("/*")?;
    if prefix.is_empty() {
        return None;
    }
    let target = value
        .trim_start_matches("./")
        .strip_suffix("/*.js")
        .or_else(|| value.trim_start_matches("./").strip_suffix("/*.ts"))
        .or_else(|| value.trim_start_matches("./").strip_suffix("/*"))?;
    if target.is_empty() {
        return None;
    }
    Some((format!("{prefix}/"), format!("{target}/")))
}

/// Read `package.json#imports` from the project root.
///
/// Returns the `(key, replacement)` pairs as authored. Only string-valued
/// replacements are returned; conditional-imports objects
/// (`{ "node": "...", "default": "..." }`) are skipped since AdonisJS
/// scaffolds use the plain string form. Returns `None` when the file is
/// missing, unreadable, or the `imports` field is absent.
fn read_subpath_imports(root: &Path) -> Option<Vec<(String, String)>> {
    let pkg_path = root.join("package.json");
    let content = std::fs::read_to_string(&pkg_path).ok()?;
    let value: serde_json::Value = serde_json::from_str(&content).ok()?;
    let imports = value.get("imports")?.as_object()?;
    let mut pairs = Vec::with_capacity(imports.len());
    for (k, v) in imports {
        if let Some(s) = v.as_str() {
            pairs.push((k.clone(), s.to_string()));
        }
    }
    Some(pairs)
}

define_plugin! {
    struct AdonisPlugin => "adonis",
    enablers: ENABLERS,
    entry_patterns: ENTRY_PATTERNS,
    config_patterns: CONFIG_PATTERNS,
    always_used: ALWAYS_USED,
    tooling_dependencies: TOOLING_DEPENDENCIES,
    virtual_module_prefixes: VIRTUAL_MODULE_PREFIXES,
    resolve_config(config_path, source, root) {
        let filename = config_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("");

        match filename {
            ".adonisrc.json" => resolve_v5_adonisrc(config_path, source),
            "adonisrc.ts" | "adonisrc.js" => resolve_v6_adonisrc(config_path, source, root),
            _ => PluginResult::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn has_entry_pattern(result: &PluginResult, pattern: &str) -> bool {
        result
            .entry_patterns
            .iter()
            .any(|rule| rule.pattern == pattern)
    }

    fn rc_path() -> &'static Path {
        Path::new(".adonisrc.json")
    }

    #[test]
    fn local_path_emits_file_and_dir_globs() {
        assert_eq!(
            local_path_to_entry_patterns("./start/routes"),
            vec![
                "start/routes.{ts,js}".to_string(),
                "start/routes/**/*.{ts,js}".to_string(),
            ]
        );
    }

    #[test]
    fn local_path_strips_existing_extension() {
        assert_eq!(
            local_path_to_entry_patterns("./providers/AppProvider.ts"),
            vec![
                "providers/AppProvider.{ts,js}".to_string(),
                "providers/AppProvider/**/*.{ts,js}".to_string(),
            ]
        );
    }

    #[test]
    fn local_path_handles_directory_form() {
        assert_eq!(
            local_path_to_entry_patterns("./commands"),
            vec![
                "commands.{ts,js}".to_string(),
                "commands/**/*.{ts,js}".to_string(),
            ]
        );
    }

    #[test]
    fn local_path_rejects_empty_after_stripping() {
        assert!(local_path_to_entry_patterns("./").is_empty());
        assert!(local_path_to_entry_patterns("").is_empty());
    }

    #[test]
    fn is_local_rc_path_detects_relative_and_absolute() {
        assert!(is_local_rc_path("./start/routes"));
        assert!(is_local_rc_path("../shared/config"));
        assert!(is_local_rc_path("/abs/path"));
        assert!(!is_local_rc_path("@adonisjs/core"));
        assert!(!is_local_rc_path("adonis5-sentry"));
    }

    #[test]
    fn resolve_v5_extracts_preloads_providers_commands() {
        let source = r#"{
            "typescript": true,
            "preloads": ["./start/routes", "./start/kernel"],
            "providers": [
                "./providers/AppProvider",
                "@adonisjs/core",
                "@adonisjs/lucid"
            ],
            "commands": [
                "./commands",
                "@adonisjs/core/build/commands/index.js"
            ]
        }"#;
        let result = resolve_v5_adonisrc(rc_path(), source);

        // Locals become entry patterns (both file and directory forms).
        assert!(has_entry_pattern(&result, "start/routes.{ts,js}"));
        assert!(has_entry_pattern(&result, "start/kernel.{ts,js}"));
        assert!(has_entry_pattern(&result, "providers/AppProvider.{ts,js}"));
        assert!(has_entry_pattern(&result, "commands.{ts,js}"));
        assert!(has_entry_pattern(&result, "commands/**/*.{ts,js}"));

        // External package specs become referenced dependencies.
        assert!(
            result
                .referenced_dependencies
                .contains(&"@adonisjs/core".to_string())
        );
        assert!(
            result
                .referenced_dependencies
                .contains(&"@adonisjs/lucid".to_string())
        );
    }

    #[test]
    fn resolve_v5_extracts_object_form_entries() {
        // v5 allows `{ file: "...", environment: ["web"] }` next to plain strings.
        let source = r#"{
            "preloads": [
                { "file": "./start/socket", "environment": ["web"] },
                "./start/events"
            ]
        }"#;
        let result = resolve_v5_adonisrc(rc_path(), source);
        assert!(has_entry_pattern(&result, "start/socket.{ts,js}"));
        assert!(has_entry_pattern(&result, "start/events.{ts,js}"));
    }

    #[test]
    fn resolve_v5_extracts_ace_providers() {
        let source = r#"{
            "aceProviders": [
                "@adonisjs/repl",
                "./providers/AceProvider"
            ]
        }"#;
        let result = resolve_v5_adonisrc(rc_path(), source);
        assert!(has_entry_pattern(&result, "providers/AceProvider.{ts,js}"));
        assert!(
            result
                .referenced_dependencies
                .contains(&"@adonisjs/repl".to_string())
        );
    }

    #[test]
    fn resolve_v5_extracts_path_aliases() {
        let source = r#"{
            "aliases": {
                "App": "app",
                "Config": "config",
                "Database": "database",
                "Contracts": "contracts"
            }
        }"#;
        let result = resolve_v5_adonisrc(rc_path(), source);
        assert!(
            result
                .path_aliases
                .contains(&("App/".to_string(), "app/".to_string()))
        );
        assert!(
            result
                .path_aliases
                .contains(&("Config/".to_string(), "config/".to_string()))
        );
        assert!(
            result
                .path_aliases
                .contains(&("Database/".to_string(), "database/".to_string()))
        );
        assert!(
            result
                .path_aliases
                .contains(&("Contracts/".to_string(), "contracts/".to_string()))
        );
    }

    #[test]
    fn resolve_v5_path_aliases_strip_dot_slash() {
        // Some projects write replacement values as "./app" instead of "app".
        let source = r#"{
            "aliases": {
                "App": "./app"
            }
        }"#;
        let result = resolve_v5_adonisrc(rc_path(), source);
        assert!(
            result
                .path_aliases
                .contains(&("App/".to_string(), "app/".to_string()))
        );
    }

    #[test]
    fn resolve_v5_extracts_meta_files_patterns() {
        let source = r#"{
            "metaFiles": [
                { "pattern": "resources/views/**/*.edge", "reloadServer": false },
                { "pattern": "newrelic.js", "reloadServer": false }
            ]
        }"#;
        let result = resolve_v5_adonisrc(rc_path(), source);
        assert!(
            result
                .always_used_files
                .contains(&"resources/views/**/*.edge".to_string())
        );
        assert!(
            result
                .always_used_files
                .contains(&"newrelic.js".to_string())
        );
    }

    #[test]
    fn resolve_v5_extracts_types_declarations() {
        let source = r#"{
            "types": ["./contracts/logger.ts", "./contracts/env.ts"]
        }"#;
        let result = resolve_v5_adonisrc(rc_path(), source);
        assert!(
            result
                .always_used_files
                .contains(&"contracts/logger.ts".to_string())
        );
        assert!(
            result
                .always_used_files
                .contains(&"contracts/env.ts".to_string())
        );
    }

    #[test]
    fn resolve_v5_ignores_non_adonisrc_filenames() {
        // The resolve_config dispatcher returns default when the filename
        // doesn't match a known rc shape. We assert it here so refactors
        // don't silently misroute adonisrc.ts (handled separately).
        let plugin = AdonisPlugin;
        let result = plugin.resolve_config(
            Path::new("some-other.json"),
            r#"{ "preloads": ["./should-not-be-extracted"] }"#,
            Path::new("/project"),
        );
        assert!(result.entry_patterns.is_empty());
        assert!(result.referenced_dependencies.is_empty());
        assert!(result.path_aliases.is_empty());
    }

    #[test]
    fn resolve_v5_full_customer_api_shape() {
        // Mirror the actual .adonisrc.json from project-target/customer-api-rest-v2
        // to assert the parser handles real-world rc shape end to end.
        let source = r#"{
            "typescript": true,
            "commands": [
                "./commands",
                "@adonisjs/core/build/commands/index.js",
                "@adonisjs/repl/build/commands",
                "@adonisjs/lucid/build/commands",
                "@adonisjs/mail/build/commands"
            ],
            "exceptionHandlerNamespace": "App/Exceptions/Handler",
            "aliases": {
                "App": "app",
                "Config": "config",
                "Database": "database",
                "Contracts": "contracts"
            },
            "preloads": [
                "./start/routes",
                "./start/events",
                "./start/kernel",
                "./start/validator"
            ],
            "providers": [
                "./providers/AppProvider",
                "./providers/LoggerProvider",
                "@adonisjs/core",
                "@adonisjs/lucid",
                "@adonisjs/auth",
                "@adonisjs/mail",
                "adonis5-sentry",
                "@adonisjs/view"
            ],
            "aceProviders": ["@adonisjs/repl"],
            "metaFiles": [
                { "pattern": "resources/views/**/*.edge", "reloadServer": false },
                { "pattern": "newrelic.js", "reloadServer": false }
            ],
            "types": ["./contracts/logger.ts"]
        }"#;

        let result = resolve_v5_adonisrc(rc_path(), source);

        // Every preload yields an entry pattern.
        for preload in [
            "start/routes.{ts,js}",
            "start/events.{ts,js}",
            "start/kernel.{ts,js}",
            "start/validator.{ts,js}",
        ] {
            assert!(
                has_entry_pattern(&result, preload),
                "missing preload entry: {preload}"
            );
        }

        // Project providers become entries.
        assert!(has_entry_pattern(&result, "providers/AppProvider.{ts,js}"));
        assert!(has_entry_pattern(
            &result,
            "providers/LoggerProvider.{ts,js}"
        ));

        // External providers / ace providers become referenced deps.
        for ext in [
            "@adonisjs/core",
            "@adonisjs/lucid",
            "@adonisjs/auth",
            "@adonisjs/mail",
            "@adonisjs/view",
            "@adonisjs/repl",
            "adonis5-sentry",
        ] {
            assert!(
                result.referenced_dependencies.contains(&ext.to_string()),
                "missing referenced dependency: {ext}"
            );
        }

        // Aliases mapped with trailing slash convention.
        assert_eq!(result.path_aliases.len(), 4);

        // Meta files and types end up in always_used_files.
        assert!(
            result
                .always_used_files
                .contains(&"resources/views/**/*.edge".to_string())
        );
        assert!(
            result
                .always_used_files
                .contains(&"newrelic.js".to_string())
        );
        assert!(
            result
                .always_used_files
                .contains(&"contracts/logger.ts".to_string())
        );
    }

    #[test]
    fn is_enabled_detects_adonis_core_dependency() {
        // The plugin must activate whenever @adonisjs/core is in dependencies.
        // Other unrelated packages must not enable it on their own.
        let plugin = AdonisPlugin;
        assert!(plugin.is_enabled_with_deps(
            &["@adonisjs/core".to_string(), "luxon".to_string()],
            Path::new("/project"),
        ));
        assert!(!plugin.is_enabled_with_deps(
            &["express".to_string(), "luxon".to_string()],
            Path::new("/project"),
        ));
    }

    #[test]
    fn subpath_import_to_path_alias_typical_form() {
        assert_eq!(
            subpath_import_to_path_alias("#controllers/*", "./app/controllers/*.js"),
            Some(("#controllers/".to_string(), "app/controllers/".to_string()))
        );
    }

    #[test]
    fn subpath_import_to_path_alias_ts_extension_form() {
        assert_eq!(
            subpath_import_to_path_alias("#models/*", "./app/models/*.ts"),
            Some(("#models/".to_string(), "app/models/".to_string()))
        );
    }

    #[test]
    fn subpath_import_to_path_alias_no_extension() {
        assert_eq!(
            subpath_import_to_path_alias("#start/*", "./start/*"),
            Some(("#start/".to_string(), "start/".to_string()))
        );
    }

    #[test]
    fn subpath_import_to_path_alias_rejects_bare_keys() {
        // Subpath imports without "/*" don't map cleanly to prefix aliases;
        // we drop them rather than emit a broken alias.
        assert_eq!(
            subpath_import_to_path_alias("#start/routes", "./start/routes.js"),
            None
        );
    }

    fn rc_ts_path() -> &'static Path {
        Path::new("adonisrc.ts")
    }

    #[test]
    fn resolve_v6_classifies_lazy_imports_into_correct_buckets() {
        let source = r"
            import { defineConfig } from '@adonisjs/core/app'

            export default defineConfig({
                commands: [
                    () => import('@adonisjs/core/commands'),
                    () => import('@adonisjs/lucid/commands'),
                ],
                providers: [
                    () => import('@adonisjs/core/providers/app_provider'),
                    {
                        file: () => import('@adonisjs/core/providers/repl_provider'),
                        environment: ['repl', 'test'],
                    },
                    () => import('./providers/app_provider'),
                ],
                preloads: [
                    () => import('#start/routes'),
                    () => import('#start/kernel'),
                ],
            })
        ";

        // Use a non-existent root so package.json#imports reading silently fails.
        let result = resolve_v6_adonisrc(rc_ts_path(), source, Path::new("/non-existent-root"));

        // Bare external packages → referenced dependencies.
        for ext in ["@adonisjs/core", "@adonisjs/lucid"] {
            assert!(
                result.referenced_dependencies.contains(&ext.to_string()),
                "missing referenced dep: {ext}"
            );
        }

        // Local relative path → entry pattern.
        assert!(has_entry_pattern(&result, "providers/app_provider.{ts,js}"));

        // Subpath imports (#start/...) are deliberately routed via path aliases,
        // not as direct entry patterns derived from the spec.
        assert!(
            !has_entry_pattern(&result, "start/routes.{ts,js}"),
            "subpath spec should not become an entry pattern directly"
        );
    }

    #[test]
    fn resolve_v6_extracts_directory_overrides() {
        let source = r"
            export default defineConfig({
                directories: {
                    resolvers: 'app/graphql/resolvers',
                    schemas: './app/graphql/schemas',
                },
            })
        ";
        let result = resolve_v6_adonisrc(rc_ts_path(), source, Path::new("/non-existent-root"));
        assert!(has_entry_pattern(
            &result,
            "app/graphql/resolvers/**/*.{ts,js}"
        ));
        assert!(has_entry_pattern(
            &result,
            "app/graphql/schemas/**/*.{ts,js}"
        ));
    }

    #[test]
    fn resolve_v6_classifies_hook_lazy_imports() {
        // Adonis assembler hooks accept the same `() => import('SPEC')`
        // thunk shape as commands / providers / preloads, just nested under
        // `hooks.<name>`. External hook packages should land as referenced
        // deps; project-local hook modules should yield entry patterns.
        let source = r"
            import { defineConfig } from '@adonisjs/core/app'

            export default defineConfig({
                hooks: {
                    onBuildStarting: [
                        () => import('@adonisjs/vite/build_hook'),
                        () => import('./hooks/on_build_starting'),
                    ],
                    onBuildCompleted: [
                        () => import('my-package/hooks/on_build_completed'),
                    ],
                    onDevServerStarted: [
                        () => import('./hooks/on_dev_started'),
                    ],
                    onSourceFileChanged: [
                        () => import('@scope/dev-tools/hooks/on_change'),
                    ],
                },
            })
        ";
        let result = resolve_v6_adonisrc(rc_ts_path(), source, Path::new("/non-existent-root"));

        // External hook packages routed as referenced dependencies.
        for pkg in ["@adonisjs/vite", "my-package", "@scope/dev-tools"] {
            assert!(
                result.referenced_dependencies.contains(&pkg.to_string()),
                "missing referenced hook dep: {pkg}"
            );
        }

        // Local hook modules become entry patterns (file + directory form).
        assert!(has_entry_pattern(
            &result,
            "hooks/on_build_starting.{ts,js}"
        ));
        assert!(has_entry_pattern(&result, "hooks/on_dev_started.{ts,js}"));
    }

    #[test]
    fn resolve_v6_extracts_meta_files() {
        let source = r"
            export default defineConfig({
                metaFiles: [
                    { pattern: 'public/**', reloadServer: false },
                    { pattern: 'resources/views/**/*.edge', reloadServer: false },
                ],
            })
        ";
        let result = resolve_v6_adonisrc(rc_ts_path(), source, Path::new("/non-existent-root"));
        assert!(result.always_used_files.contains(&"public/**".to_string()));
        assert!(
            result
                .always_used_files
                .contains(&"resources/views/**/*.edge".to_string())
        );
    }

    #[test]
    fn resolve_v6_reads_subpath_imports_from_package_json() {
        // Create a temp project with a v6-style package.json + adonisrc.ts.
        // We exercise the real disk read inside resolve_v6_adonisrc.
        let temp = tempfile::tempdir().expect("tempdir");
        let root = temp.path();
        // Use `r##"..."##` (two hashes) because the JSON content contains
        // `"#` substrings (the subpath-import key opener) which would
        // terminate a single-hash raw string prematurely.
        std::fs::write(
            root.join("package.json"),
            r##"{
                "name": "test-v6-app",
                "type": "module",
                "imports": {
                    "#controllers/*": "./app/controllers/*.js",
                    "#models/*": "./app/models/*.js",
                    "#start/*": "./start/*.js",
                    "#config/*": "./config/*.js",
                    "#graphql/*": "./app/graphql/*.js"
                }
            }"##,
        )
        .expect("write package.json");

        let rc_source = r"
            export default defineConfig({
                preloads: [() => import('#start/routes')],
            })
        ";
        let rc_path = root.join("adonisrc.ts");
        std::fs::write(&rc_path, rc_source).expect("write adonisrc.ts");

        let result = resolve_v6_adonisrc(&rc_path, rc_source, root);

        // Every imports entry should yield a path alias pair.
        for (find, repl) in [
            ("#controllers/", "app/controllers/"),
            ("#models/", "app/models/"),
            ("#start/", "start/"),
            ("#config/", "config/"),
            ("#graphql/", "app/graphql/"),
        ] {
            assert!(
                result
                    .path_aliases
                    .contains(&(find.to_string(), repl.to_string())),
                "missing alias: {find} -> {repl}"
            );
        }
    }

    #[test]
    fn resolve_v6_handles_missing_package_json_gracefully() {
        // No package.json at root → no aliases emitted, but the rest of the
        // parse still completes and other buckets populate normally.
        let source = r"
            export default defineConfig({
                providers: [() => import('@adonisjs/core/providers/app_provider')],
            })
        ";
        let result = resolve_v6_adonisrc(rc_ts_path(), source, Path::new("/non-existent-root"));
        assert!(result.path_aliases.is_empty());
        assert!(
            result
                .referenced_dependencies
                .contains(&"@adonisjs/core".to_string())
        );
    }

    #[test]
    fn resolve_v6_full_friends_of_adonis_playground_shape() {
        // End-to-end shape mirroring project-target/FriendsOfAdonis/playgrounds/graphql.
        let source = r"
            import { indexEntities } from '@adonisjs/core'
            import { defineConfig } from '@adonisjs/core/app'
            import { indexResolvers } from '@foadonis/graphql'

            export default defineConfig({
                commands: [
                    () => import('@adonisjs/core/commands'),
                    () => import('@adonisjs/lucid/commands'),
                    () => import('@adonisjs/bouncer/commands'),
                    () => import('@foadonis/graphql/commands'),
                ],
                providers: [
                    () => import('@adonisjs/core/providers/app_provider'),
                    () => import('@adonisjs/core/providers/hash_provider'),
                    {
                        file: () => import('@adonisjs/core/providers/repl_provider'),
                        environment: ['repl', 'test'],
                    },
                    () => import('@foadonis/graphql/graphql_provider'),
                    () => import('@adonisjs/lucid/database_provider'),
                ],
                preloads: [
                    () => import('#start/routes'),
                    () => import('#start/kernel'),
                    () => import('#start/graphql'),
                ],
                directories: {
                    resolvers: 'app/graphql/resolvers',
                },
            })
        ";

        let temp = tempfile::tempdir().expect("tempdir");
        let root = temp.path();
        // Two-hash raw string: JSON keys contain `"#` which would close the
        // single-hash form prematurely.
        std::fs::write(
            root.join("package.json"),
            r##"{
                "name": "playground-graphql",
                "type": "module",
                "imports": {
                    "#controllers/*": "./app/controllers/*.js",
                    "#models/*": "./app/models/*.js",
                    "#middleware/*": "./app/middleware/*.js",
                    "#start/*": "./start/*.js",
                    "#config/*": "./config/*.js",
                    "#graphql/*": "./app/graphql/*.js"
                }
            }"##,
        )
        .expect("write package.json");

        let result = resolve_v6_adonisrc(&root.join("adonisrc.ts"), source, root);

        // Every external command/provider package is registered as referenced.
        for pkg in [
            "@adonisjs/core",
            "@adonisjs/lucid",
            "@adonisjs/bouncer",
            "@foadonis/graphql",
        ] {
            assert!(
                result.referenced_dependencies.contains(&pkg.to_string()),
                "missing referenced dep: {pkg}"
            );
        }

        // directories.resolvers must contribute an entry pattern so resolvers
        // outside the conventional app/* layout stay alive.
        assert!(has_entry_pattern(
            &result,
            "app/graphql/resolvers/**/*.{ts,js}"
        ));

        // package.json#imports map to path aliases — project-specific
        // (#graphql/*) included.
        assert!(
            result
                .path_aliases
                .contains(&("#graphql/".to_string(), "app/graphql/".to_string()))
        );
        assert!(
            result
                .path_aliases
                .contains(&("#start/".to_string(), "start/".to_string()))
        );
    }
}
