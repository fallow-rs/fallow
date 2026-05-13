//! Main resolution engine: creates the oxc_resolver instance and resolves individual specifiers.

use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

use globset::{Glob, GlobSetBuilder};
use oxc_resolver::{Resolution, ResolveError, ResolveOptions, Resolver};
use serde_json::Value;

use super::fallbacks::{
    extract_package_name_from_node_modules_path, try_css_extension_fallback,
    try_path_alias_fallback, try_pnpm_workspace_fallback, try_scss_include_path_fallback,
    try_scss_node_modules_fallback, try_scss_partial_fallback, try_source_fallback,
    try_workspace_package_fallback,
};
use super::path_info::{
    extract_package_name, is_bare_specifier, is_path_alias, is_valid_package_name,
};
use super::react_native::{build_condition_names, build_extensions};
use super::types::{ResolveContext, ResolveResult};

/// Create an `oxc_resolver` instance with standard configuration.
///
/// When React Native or Expo plugins are active, platform-specific extensions
/// (e.g., `.web.tsx`, `.ios.ts`) are prepended to the extension list so that
/// Metro-style platform resolution works correctly. User-supplied
/// `extra_conditions` are prepended to the resolver's `condition_names`
/// list, giving them priority over baseline conditions during package.json
/// `exports` / `imports` matching.
pub(super) fn create_resolver(active_plugins: &[String], extra_conditions: &[String]) -> Resolver {
    let mut options = ResolveOptions {
        extensions: build_extensions(active_plugins),
        // Support TypeScript's node16/nodenext module resolution where .ts files
        // are imported with .js extensions (e.g., `import './api.js'` for `api.ts`).
        extension_alias: vec![
            (
                ".js".into(),
                vec![".ts".into(), ".tsx".into(), ".js".into()],
            ),
            (".jsx".into(), vec![".tsx".into(), ".jsx".into()]),
            (".mjs".into(), vec![".mts".into(), ".mjs".into()]),
            (".cjs".into(), vec![".cts".into(), ".cjs".into()]),
        ],
        condition_names: build_condition_names(active_plugins, extra_conditions),
        main_fields: vec!["module".into(), "main".into()],
        ..Default::default()
    };

    // Always use auto-discovery mode so oxc_resolver finds the nearest tsconfig.json
    // for each file. This is critical for monorepos where workspace packages have
    // their own tsconfig with path aliases (e.g., `~/*` → `./src/*`). Manual mode
    // with a root tsconfig only uses that single tsconfig's paths for ALL files,
    // missing workspace-specific aliases. Auto mode walks up from each file to find
    // the nearest tsconfig.json and follows `extends` chains, so workspace tsconfigs
    // that extend a root tsconfig still inherit root-level paths.
    options.tsconfig = Some(oxc_resolver::TsconfigDiscovery::Auto);

    Resolver::new(options)
}

/// Return `true` for errors raised while loading a tsconfig file (as opposed to
/// errors about the specifier itself). When `resolve_file` fails with one of these,
/// a broken sibling tsconfig is poisoning resolution for the current file — retrying
/// via `resolve(dir, specifier)` bypasses `TsconfigDiscovery::Auto` and restores
/// resolution for everything that does not need path aliases (relative, absolute,
/// bare package specifiers).
///
/// `IOError` and `Json` are included because a malformed or unreadable tsconfig
/// surfaces as one of these — the variants are shared with package.json parsing,
/// but a retry is still safe: if the error really came from the specifier's own
/// resolution, `resolve()` will fail the same way and we fall through to the
/// existing error handling.
const fn is_tsconfig_error(err: &ResolveError) -> bool {
    matches!(
        err,
        ResolveError::TsconfigNotFound(_)
            | ResolveError::TsconfigCircularExtend(_)
            | ResolveError::TsconfigSelfReference(_)
            | ResolveError::Json(_)
            | ResolveError::IOError(_)
    )
}

enum ResolveFileAttempt {
    Resolved {
        resolution: Resolution,
        used_tsconfig_fallback: bool,
    },
    Failed {
        used_tsconfig_fallback: bool,
    },
}

/// Try `resolve_file` first (honors per-file tsconfig discovery); on a
/// tsconfig-loading failure, retry with `resolve(dir, specifier)` which skips
/// tsconfig entirely. Emits a single `tracing::warn!` per unique error message
/// so users get one actionable hint per broken tsconfig without log spam.
fn resolve_file_with_tsconfig_fallback(
    ctx: &ResolveContext<'_>,
    from_file: &Path,
    specifier: &str,
) -> ResolveFileAttempt {
    resolve_file_with_resolver_and_tsconfig_fallback(ctx, ctx.resolver, from_file, specifier)
}

fn resolve_file_with_resolver_and_tsconfig_fallback(
    ctx: &ResolveContext<'_>,
    resolver: &Resolver,
    from_file: &Path,
    specifier: &str,
) -> ResolveFileAttempt {
    match resolver.resolve_file(from_file, specifier) {
        Ok(resolution) => ResolveFileAttempt::Resolved {
            resolution,
            used_tsconfig_fallback: false,
        },
        Err(err) if is_tsconfig_error(&err) => {
            warn_once_tsconfig(ctx, &err);
            let dir = from_file.parent().unwrap_or(from_file);
            match resolver.resolve(dir, specifier) {
                Ok(resolution) => ResolveFileAttempt::Resolved {
                    resolution,
                    used_tsconfig_fallback: true,
                },
                Err(_) => ResolveFileAttempt::Failed {
                    used_tsconfig_fallback: true,
                },
            }
        }
        Err(_) => ResolveFileAttempt::Failed {
            used_tsconfig_fallback: false,
        },
    }
}

/// Emit a `tracing::warn!` the first time a given tsconfig error message is
/// observed. The shared `Mutex<FxHashSet<String>>` in the resolver context
/// dedupes across all parallel threads for the lifetime of one analysis run.
fn warn_once_tsconfig(ctx: &ResolveContext<'_>, err: &ResolveError) {
    let message = err.to_string();
    let should_warn = {
        let Ok(mut seen) = ctx.tsconfig_warned.lock() else {
            // Mutex poisoned by a panic on another thread — stay silent rather
            // than poisoning this thread's resolution with another panic.
            return;
        };
        seen.insert(message.clone())
    };
    if should_warn {
        tracing::warn!(
            "Broken tsconfig chain: {message}. Falling back to resolver-less resolution for \
             affected files. Relative and bare imports still work, but tsconfig path aliases \
             from missing inherited configs will not. Fix the extends/references chain to restore \
             full alias support."
        );
    }
}

fn nearest_tsconfig_path(root: &Path, from_file: &Path) -> Option<PathBuf> {
    let mut current = from_file.parent()?;
    loop {
        let candidate = current.join("tsconfig.json");
        if candidate.is_file() {
            return Some(candidate);
        }
        if current == root {
            return None;
        }
        current = current.parent()?;
        if !current.starts_with(root) {
            return None;
        }
    }
}

fn local_tsconfig_chain(root: &Path, from_file: &Path) -> Vec<PathBuf> {
    let Some(first) = nearest_tsconfig_path(root, from_file) else {
        return Vec::new();
    };
    let mut chain = Vec::new();
    let mut seen = rustc_hash::FxHashSet::default();
    collect_local_tsconfig_chain(root, from_file, &first, &mut chain, &mut seen);
    chain
}

fn collect_local_tsconfig_chain(
    root: &Path,
    from_file: &Path,
    tsconfig_path: &Path,
    chain: &mut Vec<PathBuf>,
    seen: &mut rustc_hash::FxHashSet<PathBuf>,
) {
    if !seen.insert(tsconfig_path.to_path_buf()) {
        return;
    }
    let Some(json) = read_tsconfig_json(tsconfig_path) else {
        return;
    };
    chain.push(tsconfig_path.to_path_buf());

    let tsconfig_dir = tsconfig_path.parent().unwrap_or(root);
    for reference in referenced_tsconfig_paths(tsconfig_dir, &json) {
        if reference.is_file() && tsconfig_applies_to_file(&reference, from_file, root) {
            collect_local_tsconfig_chain(root, from_file, &reference, chain, seen);
        }
    }

    for extends in tsconfig_extends_values(&json) {
        let next = resolve_tsconfig_extends_path(tsconfig_dir, extends);
        if next.is_file() {
            collect_local_tsconfig_chain(root, from_file, &next, chain, seen);
        }
    }
}

fn referenced_tsconfig_paths(base_dir: &Path, json: &Value) -> Vec<PathBuf> {
    json.get("references")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|reference| reference.get("path").and_then(Value::as_str))
        .map(|path| resolve_tsconfig_reference_path(base_dir, path))
        .collect()
}

fn resolve_tsconfig_reference_path(base_dir: &Path, reference: &str) -> PathBuf {
    let path = base_dir.join(reference);
    if path.is_dir() {
        return path.join("tsconfig.json");
    }
    if path
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext == "json" || ext == "jsonc")
    {
        return path;
    }
    let mut with_json = OsString::from(path.as_os_str());
    with_json.push(".json");
    let with_json = PathBuf::from(with_json);
    if with_json.is_file() {
        with_json
    } else {
        path.join("tsconfig.json")
    }
}

fn tsconfig_extends_values(json: &Value) -> Vec<&str> {
    match json.get("extends") {
        Some(Value::String(extends)) => vec![extends.as_str()],
        Some(Value::Array(values)) => values.iter().filter_map(Value::as_str).collect(),
        _ => Vec::new(),
    }
}

fn tsconfig_applies_to_file(tsconfig_path: &Path, from_file: &Path, root: &Path) -> bool {
    let Some(json) = read_tsconfig_json(tsconfig_path) else {
        return false;
    };
    let tsconfig_dir = tsconfig_path.parent().unwrap_or(root);
    if let Some(files) = json.get("files").and_then(Value::as_array) {
        return files
            .iter()
            .filter_map(Value::as_str)
            .map(|file| resolve_tsconfig_relative_path(tsconfig_dir, file))
            .any(|file| same_path(&file, from_file));
    }

    let include_matches = json
        .get("include")
        .and_then(Value::as_array)
        .is_none_or(|include| glob_values_match(tsconfig_dir, include, from_file));
    if !include_matches {
        return false;
    }

    !json
        .get("exclude")
        .and_then(Value::as_array)
        .is_some_and(|exclude| glob_values_match(tsconfig_dir, exclude, from_file))
}

fn glob_values_match(base_dir: &Path, values: &[Value], path: &Path) -> bool {
    let mut builder = GlobSetBuilder::new();
    let mut has_patterns = false;
    for value in values.iter().filter_map(Value::as_str) {
        let mut pattern = resolve_tsconfig_relative_path(base_dir, value);
        if !has_glob_meta(value) && pattern.is_dir() {
            pattern = pattern.join("**/*");
        }
        let Some(pattern) = pattern.to_str() else {
            continue;
        };
        let Ok(glob) = Glob::new(pattern) else {
            continue;
        };
        builder.add(glob);
        has_patterns = true;
    }
    has_patterns && builder.build().is_ok_and(|set| set.is_match(path))
}

fn has_glob_meta(value: &str) -> bool {
    value
        .bytes()
        .any(|byte| matches!(byte, b'*' | b'?' | b'[' | b']' | b'{'))
}

fn resolve_tsconfig_relative_path(base_dir: &Path, path: &str) -> PathBuf {
    let path = Path::new(path);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        base_dir.join(path)
    }
}

fn same_path(left: &Path, right: &Path) -> bool {
    left == right
        || dunce::canonicalize(left)
            .ok()
            .zip(dunce::canonicalize(right).ok())
            .is_some_and(|(left, right)| left == right)
}

fn resolve_tsconfig_extends_path(base_dir: &Path, extends: &str) -> PathBuf {
    let path = if is_relative_tsconfig_extends(extends) || Path::new(extends).is_absolute() {
        base_dir.join(extends)
    } else if let Some(package_path) = resolve_package_tsconfig_extends(base_dir, extends) {
        package_path
    } else {
        base_dir.join(extends)
    };
    if path
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext == "json" || ext == "jsonc")
    {
        path
    } else {
        let mut with_json = OsString::from(path.as_os_str());
        with_json.push(".json");
        PathBuf::from(with_json)
    }
}

fn is_relative_tsconfig_extends(extends: &str) -> bool {
    extends.starts_with("./") || extends.starts_with("../")
}

fn resolve_package_tsconfig_extends(base_dir: &Path, extends: &str) -> Option<PathBuf> {
    for ancestor in base_dir.ancestors() {
        let candidate = ancestor.join("node_modules").join(extends);
        let candidate = resolve_tsconfig_extends_candidate(candidate);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

fn resolve_tsconfig_extends_candidate(path: PathBuf) -> PathBuf {
    if path.is_dir() {
        return path.join("tsconfig.json");
    }
    if path
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext == "json" || ext == "jsonc")
    {
        return path;
    }
    let mut with_json = OsString::from(path.as_os_str());
    with_json.push(".json");
    let with_json = PathBuf::from(with_json);
    if with_json.is_file() { with_json } else { path }
}

fn read_tsconfig_json(path: &Path) -> Option<Value> {
    read_json_file(path)
}

fn read_json_file(path: &Path) -> Option<Value> {
    let content = fs::read_to_string(path).ok()?;
    if let Ok(json) = serde_json::from_str::<Value>(&content) {
        return Some(json);
    }
    jsonc_parser::parse_to_serde_value::<Value>(&content, &jsonc_parse_options()).ok()
}

fn jsonc_parse_options() -> jsonc_parser::ParseOptions {
    jsonc_parser::ParseOptions {
        allow_comments: true,
        allow_loose_object_property_names: false,
        allow_trailing_commas: true,
        allow_missing_commas: false,
        allow_single_quoted_strings: false,
        allow_hexadecimal_numbers: false,
        allow_unary_plus_numbers: false,
    }
}

fn path_alias_pattern_matches(pattern: &str, specifier: &str) -> bool {
    if pattern == "*" {
        return false;
    }
    path_alias_capture(pattern, specifier).is_some()
}

fn path_alias_capture<'a>(pattern: &str, specifier: &'a str) -> Option<&'a str> {
    match pattern.split_once('*') {
        Some((prefix, suffix)) if !prefix.is_empty() || !suffix.is_empty() => {
            if specifier.starts_with(prefix)
                && specifier.ends_with(suffix)
                && specifier.len() >= prefix.len() + suffix.len()
            {
                Some(&specifier[prefix.len()..specifier.len() - suffix.len()])
            } else {
                None
            }
        }
        Some(_) => Some(specifier),
        None => (specifier == pattern).then_some(""),
    }
}

fn matches_nearest_tsconfig_path_alias(root: &Path, from_file: &Path, specifier: &str) -> bool {
    for tsconfig_path in local_tsconfig_chain(root, from_file) {
        let Some(paths) = read_tsconfig_json(&tsconfig_path).and_then(|json| {
            json.get("compilerOptions")
                .and_then(|compiler_options| compiler_options.get("paths"))
                .and_then(Value::as_object)
                .cloned()
        }) else {
            continue;
        };
        return paths
            .keys()
            .any(|pattern| path_alias_pattern_matches(pattern, specifier));
    }
    false
}

fn try_nearest_tsconfig_path_alias(
    ctx: &ResolveContext<'_>,
    from_file: &Path,
    specifier: &str,
) -> Option<ResolveResult> {
    let chain = local_tsconfig_chain(ctx.root, from_file);
    for tsconfig_path in &chain {
        let Some(json) = read_tsconfig_json(tsconfig_path) else {
            continue;
        };
        let has_paths = json
            .get("compilerOptions")
            .and_then(|compiler_options| compiler_options.get("paths"))
            .is_some();
        if let Some(result) = try_tsconfig_paths_from_config(ctx, tsconfig_path, &json, specifier) {
            return Some(result);
        }
        if has_paths {
            return None;
        }
    }
    for tsconfig_path in &chain {
        let Some(json) = read_tsconfig_json(tsconfig_path) else {
            continue;
        };
        let has_base_url = json
            .get("compilerOptions")
            .and_then(|compiler_options| compiler_options.get("baseUrl"))
            .is_some();
        if let Some(result) =
            try_tsconfig_base_url_from_config(ctx, tsconfig_path, &json, specifier)
        {
            return Some(result);
        }
        if has_base_url {
            return None;
        }
    }
    None
}

fn try_tsconfig_paths_from_config(
    ctx: &ResolveContext<'_>,
    tsconfig_path: &Path,
    json: &Value,
    specifier: &str,
) -> Option<ResolveResult> {
    let compiler_options = json.get("compilerOptions")?;
    let paths = compiler_options.get("paths")?.as_object()?;
    let tsconfig_dir = tsconfig_path.parent().unwrap_or(ctx.root);
    let base_dir = compiler_options
        .get("baseUrl")
        .and_then(Value::as_str)
        .map_or_else(
            || tsconfig_dir.to_path_buf(),
            |base_url| {
                let base_url = Path::new(base_url);
                if base_url.is_absolute() {
                    base_url.to_path_buf()
                } else {
                    tsconfig_dir.join(base_url)
                }
            },
        );

    let mut matches: Vec<_> = paths
        .iter()
        .filter_map(|(pattern, targets)| {
            path_alias_capture(pattern, specifier)
                .map(|capture| (pattern.as_str(), capture, targets))
        })
        .collect();
    matches.sort_by_key(|(pattern, _, _)| std::cmp::Reverse(path_alias_specificity(pattern)));

    for (_, capture, targets) in matches {
        let Some(targets) = targets.as_array() else {
            continue;
        };
        for target in targets.iter().filter_map(Value::as_str) {
            let target = if target.contains('*') {
                target.replacen('*', capture, 1)
            } else {
                target.to_string()
            };
            let target_path = Path::new(&target);
            let absolute = if target_path.is_absolute() {
                target_path.to_path_buf()
            } else {
                base_dir.join(target_path)
            };
            if let Some(result) = try_tsconfig_alias_target(ctx, &absolute) {
                return Some(result);
            }
        }
    }
    None
}

fn path_alias_specificity(pattern: &str) -> usize {
    match pattern.split_once('*') {
        Some((prefix, suffix)) => prefix.len() + suffix.len(),
        None => usize::MAX,
    }
}

fn try_tsconfig_base_url_from_config(
    ctx: &ResolveContext<'_>,
    tsconfig_path: &Path,
    json: &Value,
    specifier: &str,
) -> Option<ResolveResult> {
    if specifier.starts_with('.') || Path::new(specifier).is_absolute() {
        return None;
    }
    let compiler_options = json.get("compilerOptions")?;
    let base_url = compiler_options.get("baseUrl")?.as_str()?;
    let tsconfig_dir = tsconfig_path.parent().unwrap_or(ctx.root);
    let base_url = Path::new(base_url);
    let base_dir = if base_url.is_absolute() {
        base_url.to_path_buf()
    } else {
        tsconfig_dir.join(base_url)
    };
    try_tsconfig_alias_target(ctx, &base_dir.join(specifier))
}

fn try_tsconfig_alias_target(ctx: &ResolveContext<'_>, target: &Path) -> Option<ResolveResult> {
    if let Some(result) = resolve_tsconfig_alias_candidate(ctx, target) {
        return Some(result);
    }

    if let Some(result) = try_tsconfig_alias_extension_alias(ctx, target) {
        return Some(result);
    }

    if let Some(result) = try_tsconfig_alias_directory(ctx, target) {
        return Some(result);
    }

    if should_probe_extensions(ctx, target) {
        for ext in ctx.extensions {
            if let Some(result) =
                resolve_tsconfig_alias_candidate(ctx, &with_appended_extension(target, ext))
            {
                return Some(result);
            }
        }
    }

    if target.extension().is_none() {
        for ext in ctx.extensions {
            let index = target.join(format!("index{ext}"));
            if let Some(result) = resolve_tsconfig_alias_candidate(ctx, &index) {
                return Some(result);
            }
        }
    }

    None
}

fn should_probe_extensions(ctx: &ResolveContext<'_>, target: &Path) -> bool {
    target
        .extension()
        .and_then(|ext| ext.to_str())
        .is_none_or(|ext| {
            !ctx.extensions
                .iter()
                .any(|known| known.trim_start_matches('.') == ext)
        })
}

fn with_appended_extension(path: &Path, extension: &str) -> PathBuf {
    let mut value = OsString::from(path.as_os_str());
    value.push(extension);
    PathBuf::from(value)
}

fn try_tsconfig_alias_directory(ctx: &ResolveContext<'_>, target: &Path) -> Option<ResolveResult> {
    if !target.is_dir() {
        return None;
    }
    if let Some(package_json) = read_json_file(&target.join("package.json")) {
        for field in ["module", "main"] {
            if let Some(entry) = package_json.get(field).and_then(Value::as_str)
                && let Some(result) = try_tsconfig_alias_target(ctx, &target.join(entry))
            {
                return Some(result);
            }
        }
    }
    let parent = target.parent()?;
    let name = target.file_name()?.to_str()?;
    let resolved = ctx.resolver.resolve(parent, name).ok()?;
    resolve_tsconfig_alias_candidate(ctx, resolved.path())
}

fn try_tsconfig_alias_extension_alias(
    ctx: &ResolveContext<'_>,
    target: &Path,
) -> Option<ResolveResult> {
    let import_ext = target.extension().and_then(|ext| ext.to_str())?;
    if !matches!(import_ext, "js" | "jsx" | "mjs" | "cjs") {
        return None;
    }
    for ext in ctx
        .extensions
        .iter()
        .filter(|ext| extension_alias_matches(import_ext, ext))
    {
        if let Some(result) =
            resolve_tsconfig_alias_candidate(ctx, &with_exact_extension(target, ext))
        {
            return Some(result);
        }
    }
    None
}

fn extension_alias_matches(import_ext: &str, candidate_ext: &str) -> bool {
    let candidate_ext = candidate_ext.trim_start_matches('.');
    let aliases: &[&str] = match import_ext {
        "js" => &["ts", "tsx", "js"],
        "jsx" => &["tsx", "jsx"],
        "mjs" => &["mts", "mjs"],
        "cjs" => &["cts", "cjs"],
        _ => return false,
    };
    aliases
        .iter()
        .any(|alias| candidate_ext == *alias || candidate_ext.ends_with(&format!(".{alias}")))
}

fn with_exact_extension(path: &Path, extension: &str) -> PathBuf {
    let Some(file_stem) = path.file_stem() else {
        return path.to_path_buf();
    };
    let mut file_name = OsString::from(file_stem);
    file_name.push(extension);
    path.with_file_name(file_name)
}

fn resolve_tsconfig_alias_candidate(
    ctx: &ResolveContext<'_>,
    candidate: &Path,
) -> Option<ResolveResult> {
    if let Some(&file_id) = ctx.raw_path_to_id.get(candidate) {
        return Some(ResolveResult::InternalModule(file_id));
    }
    if let Ok(canonical) = dunce::canonicalize(candidate) {
        if let Some(&file_id) = ctx.path_to_id.get(canonical.as_path()) {
            return Some(ResolveResult::InternalModule(file_id));
        }
        if let Some(fallback) = ctx.canonical_fallback
            && let Some(file_id) = fallback.get(&canonical)
        {
            return Some(ResolveResult::InternalModule(file_id));
        }
        if let Some(file_id) = try_source_fallback(&canonical, ctx.path_to_id) {
            return Some(ResolveResult::InternalModule(file_id));
        }
        if let Some(file_id) =
            try_pnpm_workspace_fallback(&canonical, ctx.path_to_id, ctx.workspace_roots)
        {
            return Some(ResolveResult::InternalModule(file_id));
        }
    }
    None
}

/// Try the SCSS-specific resolution fallbacks in order: local partial,
/// framework-supplied include paths, and `node_modules/`.
///
/// Applies when the importer is a `.scss` / `.sass` file OR the import
/// originated from an SFC `<style lang="scss">` block (`from_style = true`).
/// SFC importers carry the `.vue` / `.svelte` extension at the file system
/// level but still emit SCSS-shape specifiers from style blocks; the
/// `from_style` flag is the authoritative signal that the import is a
/// CSS-context reference rather than a JS-context import from the same file.
/// Returns `None` when none of the fallbacks produce a hit, so the outer error
/// path continues to the generic alias / bare / workspace fallbacks.
fn try_scss_fallbacks(
    ctx: &ResolveContext<'_>,
    from_file: &Path,
    specifier: &str,
    from_style: bool,
) -> Option<ResolveResult> {
    let is_scss_importer = from_file
        .extension()
        .is_some_and(|e| e == "scss" || e == "sass");
    if !is_scss_importer && !from_style {
        return None;
    }
    // 0. CSS-extension probe: `./Foo` -> `./Foo.scss` / `.sass` / `.css`. The
    //    standard resolver's extension list contains both `.vue` / `.svelte` /
    //    `.astro` AND CSS extensions; for SFC importers (`from_style = true`)
    //    `./Foo` would otherwise resolve to the SFC itself instead of the
    //    sibling `Foo.scss`. SCSS importers also benefit (defensive against
    //    future extension list changes).
    if let Some(result) = try_css_extension_fallback(ctx, from_file, specifier) {
        return Some(result);
    }
    // 1. Local partial convention: `@use 'variables'` → `_variables.scss`.
    if let Some(result) = try_scss_partial_fallback(ctx, from_file, specifier) {
        return Some(result);
    }
    // 2. Framework-supplied SCSS include paths (Angular's
    //    `stylePreprocessorOptions.includePaths`, Nx equivalent). See #103.
    if let Some(result) = try_scss_include_path_fallback(ctx, from_file, specifier, from_style) {
        return Some(result);
    }
    // 3. `node_modules/` search (Sass's own resolution algorithm):
    //    `@import 'bootstrap/scss/functions'` →
    //    `node_modules/bootstrap/scss/_functions.scss`. Returns
    //    `ResolveResult::NpmPackage` so unused-/unlisted-dependency detection
    //    stays accurate. See #125.
    try_scss_node_modules_fallback(ctx, from_file, specifier, from_style)
}

fn is_style_file(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| matches!(ext, "css" | "scss" | "sass"))
}

/// Return `true` when the path's extension is a JS/TS-family runtime extension.
///
/// Used to reject standard-resolver hits when the importer is a stylesheet:
/// Sass's resolution algorithm only ever considers `.css` / `.scss` / `.sass`
/// files, so a sibling `.tsx` / `.ts` / `.js` cannot legally satisfy a Sass
/// `@use` / `@import`. The resolver's extension list mixes JS/TS and CSS,
/// so without this guard `@use 'Widget'` from a `.scss` importer would
/// resolve to a sibling `Widget.tsx` whenever both files exist. See #245.
fn is_js_ts_extension(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| {
            matches!(
                ext,
                "ts" | "tsx" | "mts" | "cts" | "gts" | "js" | "jsx" | "mjs" | "cjs" | "gjs"
            )
        })
}

fn is_plain_css_file(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext == "css")
}

fn is_bare_style_subpath(specifier: &str) -> bool {
    is_bare_specifier(specifier)
        && specifier.contains('/')
        && Path::new(specifier)
            .extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| {
                ext.eq_ignore_ascii_case("css")
                    || ext.eq_ignore_ascii_case("scss")
                    || ext.eq_ignore_ascii_case("sass")
                    || ext.eq_ignore_ascii_case("less")
            })
}

fn try_css_relative_subpath_fallback(
    ctx: &ResolveContext<'_>,
    from_file: &Path,
    specifier: &str,
    from_style: bool,
) -> Option<ResolveResult> {
    if !is_plain_css_file(from_file) || !is_bare_style_subpath(specifier) {
        return None;
    }

    let relative = format!("./{specifier}");
    match resolve_specifier(ctx, from_file, &relative, from_style) {
        ResolveResult::Unresolvable(_) => None,
        result => Some(result),
    }
}

fn is_node_modules_path(path: &Path) -> bool {
    path.components().any(|component| match component {
        std::path::Component::Normal(segment) => segment == "node_modules",
        _ => false,
    })
}

fn should_preserve_node_modules_style_file(
    specifier: &str,
    from_file: &Path,
    resolved_path: &Path,
) -> bool {
    if !is_style_file(resolved_path) || !is_node_modules_path(resolved_path) {
        return false;
    }

    let is_bare_subpath =
        is_bare_specifier(specifier) && extract_package_name(specifier).as_str() != specifier;
    if is_bare_subpath {
        return true;
    }

    is_node_modules_path(from_file) && (specifier.starts_with('.') || specifier.starts_with('/'))
}

fn try_style_condition_package_resolution(
    ctx: &ResolveContext<'_>,
    from_file: &Path,
    specifier: &str,
    from_style: bool,
) -> Option<ResolveResult> {
    if !is_bare_style_subpath(specifier) || (!from_style && !is_style_file(from_file)) {
        return None;
    }

    let ResolveFileAttempt::Resolved {
        resolution: resolved,
        ..
    } = resolve_file_with_resolver_and_tsconfig_fallback(
        ctx,
        ctx.style_resolver,
        from_file,
        specifier,
    )
    else {
        return None;
    };
    let resolved_path = resolved.path();

    if let Some(&file_id) = ctx.raw_path_to_id.get(resolved_path) {
        return Some(ResolveResult::InternalModule(file_id));
    }

    if let Some(pkg_name) = extract_package_name_from_node_modules_path(resolved_path)
        && !ctx.workspace_roots.contains_key(pkg_name.as_str())
    {
        return Some(ResolveResult::NpmPackage(pkg_name));
    }

    if let Ok(canonical) = dunce::canonicalize(resolved_path) {
        if let Some(&file_id) = ctx.path_to_id.get(canonical.as_path()) {
            return Some(ResolveResult::InternalModule(file_id));
        }
        if let Some(fallback) = ctx.canonical_fallback
            && let Some(file_id) = fallback.get(&canonical)
        {
            return Some(ResolveResult::InternalModule(file_id));
        }
        if let Some(file_id) = try_source_fallback(&canonical, ctx.path_to_id) {
            return Some(ResolveResult::InternalModule(file_id));
        }
        if let Some(file_id) =
            try_pnpm_workspace_fallback(&canonical, ctx.path_to_id, ctx.workspace_roots)
        {
            return Some(ResolveResult::InternalModule(file_id));
        }
        if let Some(pkg_name) = extract_package_name_from_node_modules_path(&canonical)
            && !ctx.workspace_roots.contains_key(pkg_name.as_str())
        {
            return Some(ResolveResult::NpmPackage(pkg_name));
        }
        return Some(ResolveResult::ExternalFile(canonical));
    }

    extract_package_name_from_node_modules_path(resolved_path)
        .map(ResolveResult::NpmPackage)
        .or_else(|| Some(ResolveResult::ExternalFile(resolved_path.to_path_buf())))
}

/// Resolve a single import specifier to a target.
///
/// `from_style` is `true` for imports extracted from CSS contexts (currently
/// SFC `<style lang="scss">` blocks and `<style src>` references). It enables
/// SCSS partial / include-path / node_modules fallbacks for SFC importers
/// without applying them to JS-context imports from the same file.
#[expect(
    clippy::too_many_lines,
    reason = "central import resolver keeps fallback order visible; style-preservation logic is \
              intentionally local to the resolution decision tree"
)]
pub(super) fn resolve_specifier(
    ctx: &ResolveContext<'_>,
    from_file: &Path,
    specifier: &str,
    from_style: bool,
) -> ResolveResult {
    // URL imports (https://, http://, data:) are valid but can't be resolved locally
    if specifier.contains("://") || specifier.starts_with("data:") {
        return ResolveResult::ExternalFile(PathBuf::from(specifier));
    }

    // Root-relative paths (`/src/main.tsx`, `/static/style.css`) are a web
    // convention meaning "relative to the project/workspace root". Vite,
    // Parcel, and other dev servers resolve them this way. In monorepos, each
    // workspace member has its own Vite root, so `site/index.html` referencing
    // `/src/main.tsx` should resolve to `site/src/main.tsx`, not
    // `<monorepo-root>/src/main.tsx`. Use the source file's parent directory as
    // the base, which is correct for both workspace members and single projects.
    //
    // Applied to web-facing source files: HTML, JSX/TSX, plain JS/TS, and
    // Glimmer files. The JSX/TSX case covers SSR frameworks like Hono where JSX
    // templates emit `<link href="/static/style.css" />`: these paths cannot be
    // AST-resolved and have the same web-root semantics as HTML. See issue #105
    // (till's comment). Applied unconditionally to JS/TS too because the JSX
    // visitor emits `ImportInfo` with the raw attribute value, and the file
    // extension after JSX retry may not reflect the original source (`.js`
    // files with JSX still parse as JSX and get their asset refs recorded here).
    if specifier.starts_with('/')
        && from_file.extension().is_some_and(|e| {
            matches!(
                e.to_str(),
                Some(
                    "html"
                        | "jsx"
                        | "tsx"
                        | "js"
                        | "ts"
                        | "mjs"
                        | "cjs"
                        | "mts"
                        | "cts"
                        | "gts"
                        | "gjs"
                )
            )
        })
    {
        let relative = format!(".{specifier}");
        let source_dir = from_file.parent().unwrap_or(ctx.root);
        if let Ok(resolved) = ctx.resolver.resolve(source_dir, &relative) {
            let resolved_path = resolved.path();
            if let Some(&file_id) = ctx.raw_path_to_id.get(resolved_path) {
                return ResolveResult::InternalModule(file_id);
            }
            if let Ok(canonical) = dunce::canonicalize(resolved_path) {
                if let Some(&file_id) = ctx.path_to_id.get(canonical.as_path()) {
                    return ResolveResult::InternalModule(file_id);
                }
                if let Some(fallback) = ctx.canonical_fallback
                    && let Some(file_id) = fallback.get(&canonical)
                {
                    return ResolveResult::InternalModule(file_id);
                }
            }
        }
        // Fall back to project root for non-workspace setups where the source
        // file may be in a subdirectory (e.g., `public/index.html` referencing
        // `/src/main.tsx`, or a Hono JSX layout in `src/` referencing `/static/style.css`).
        if source_dir != ctx.root
            && let Ok(resolved) = ctx.resolver.resolve(ctx.root, &relative)
        {
            let resolved_path = resolved.path();
            if let Some(&file_id) = ctx.raw_path_to_id.get(resolved_path) {
                return ResolveResult::InternalModule(file_id);
            }
            if let Ok(canonical) = dunce::canonicalize(resolved_path) {
                if let Some(&file_id) = ctx.path_to_id.get(canonical.as_path()) {
                    return ResolveResult::InternalModule(file_id);
                }
                if let Some(fallback) = ctx.canonical_fallback
                    && let Some(file_id) = fallback.get(&canonical)
                {
                    return ResolveResult::InternalModule(file_id);
                }
            }
        }
        return ResolveResult::Unresolvable(specifier.to_string());
    }

    // CSS-context imports (SFC `<style>` blocks) bypass the standard resolver
    // entirely and route through SCSS-aware fallbacks first. The standard
    // resolver's extension list mixes JS / SFC / CSS extensions, so a bare
    // `./Foo` from a `Foo.vue` `<style lang="scss">` block would resolve to
    // `Foo.vue` itself instead of the sibling `Foo.scss`. The SCSS fallback
    // chain restricts probing to `.css` / `.scss` / `.sass` (plus partial /
    // include-path / node_modules conventions), which matches Sass's actual
    // resolution algorithm. See issue #195 (Case B).
    if from_style && let Some(result) = try_scss_fallbacks(ctx, from_file, specifier, true) {
        return result;
    }

    // Bare specifier classification (used for fallback logic below).
    let is_bare = is_bare_specifier(specifier);
    let is_alias = is_path_alias(specifier);
    let matches_plugin_alias = ctx
        .path_aliases
        .iter()
        .any(|(prefix, _)| specifier.starts_with(prefix));

    if let Some(result) =
        try_style_condition_package_resolution(ctx, from_file, specifier, from_style)
    {
        return result;
    }

    // Use resolve_file instead of resolve so that TsconfigDiscovery::Auto works.
    // oxc_resolver's resolve() ignores Auto tsconfig discovery — only resolve_file()
    // walks up from the importing file to find the nearest tsconfig.json and apply
    // its path aliases (e.g., @/ → src/).
    //
    // If resolve_file returns a tsconfig-related error (e.g., a solution-style
    // tsconfig.json references a sibling with a broken `extends` chain), retry with
    // the directory-only `resolve()` form so a broken sibling config does not poison
    // resolution for files covered by a healthy sibling. See issue #97.
    match resolve_file_with_tsconfig_fallback(ctx, from_file, specifier) {
        ResolveFileAttempt::Resolved {
            resolution: resolved,
            used_tsconfig_fallback,
        } => {
            let resolved_path = resolved.path();
            // Reject JS/TS hits for stylesheet importers. The standard resolver's
            // extension list mixes JS/TS with CSS-family extensions and tries
            // `.tsx` / `.ts` before `.scss` / `.sass` / `.css`, so a `@use 'Widget'`
            // from a `.scss` file would otherwise resolve to a sibling
            // `Widget.tsx` even when `Widget.scss` exists next to it. Sass's
            // actual resolution algorithm only considers stylesheets; redirect
            // to the SCSS-aware fallback chain (CSS-extension probe, partial
            // convention, include paths, node_modules) and short-circuit with
            // `Unresolvable` if those also fail. See issue #245.
            let is_scss_importer = from_file
                .extension()
                .is_some_and(|e| e == "scss" || e == "sass");
            if is_scss_importer && is_js_ts_extension(resolved_path) {
                if let Some(result) = try_scss_fallbacks(ctx, from_file, specifier, from_style) {
                    return result;
                }
                return ResolveResult::Unresolvable(specifier.to_string());
            }
            if used_tsconfig_fallback {
                if let Some(result) = try_tsconfig_root_dirs(ctx, from_file, specifier) {
                    return result;
                }
                if (is_bare || is_alias || matches_plugin_alias)
                    && let Some(result) = try_nearest_tsconfig_path_alias(ctx, from_file, specifier)
                {
                    return result;
                }
                if matches_nearest_tsconfig_path_alias(ctx.root, from_file, specifier) {
                    return ResolveResult::Unresolvable(specifier.to_string());
                }
            }
            // Try raw path lookup first (avoids canonicalize syscall in most cases)
            if let Some(&file_id) = ctx.raw_path_to_id.get(resolved_path) {
                return ResolveResult::InternalModule(file_id);
            }

            // Fast path for bare specifiers resolving to node_modules: if the resolved
            // path is in node_modules (but not pnpm's .pnpm virtual store) and the
            // package is not a workspace package, skip the expensive canonicalize()
            // syscall and go directly to NpmPackage. Workspace packages need the full
            // fallback chain (source fallback, pnpm fallback) to map dist→src.
            // Note: the byte pattern check handles Unix and Windows separators separately.
            // Paths with mixed separators fall through to canonicalize() (perf-only cost).
            if is_bare
                && !resolved_path
                    .as_os_str()
                    .as_encoded_bytes()
                    .windows(7)
                    .any(|w| w == b"/.pnpm/" || w == b"\\.pnpm\\")
                && let Some(pkg_name) = extract_package_name_from_node_modules_path(resolved_path)
                && !ctx.workspace_roots.contains_key(pkg_name.as_str())
            {
                return if should_preserve_node_modules_style_file(
                    specifier,
                    from_file,
                    resolved_path,
                ) {
                    ResolveResult::ExternalFile(resolved_path.to_path_buf())
                } else {
                    ResolveResult::NpmPackage(pkg_name)
                };
            }

            // Fall back to canonical path lookup
            match dunce::canonicalize(resolved_path) {
                Ok(canonical) => {
                    if let Some(&file_id) = ctx.path_to_id.get(canonical.as_path()) {
                        ResolveResult::InternalModule(file_id)
                    } else if let Some(fallback) = ctx.canonical_fallback
                        && let Some(file_id) = fallback.get(&canonical)
                    {
                        // Intra-project symlink: raw path differs from canonical path.
                        // The lazy fallback resolves this without upfront bulk canonicalize.
                        ResolveResult::InternalModule(file_id)
                    } else if let Some(file_id) = try_source_fallback(&canonical, ctx.path_to_id) {
                        // Exports map resolved to a built output (e.g., dist/utils.js)
                        // but the source file (e.g., src/utils.ts) is what we track.
                        ResolveResult::InternalModule(file_id)
                    } else if let Some(file_id) =
                        try_pnpm_workspace_fallback(&canonical, ctx.path_to_id, ctx.workspace_roots)
                    {
                        ResolveResult::InternalModule(file_id)
                    } else if let Some(pkg_name) =
                        extract_package_name_from_node_modules_path(&canonical)
                    {
                        // Workspace package resolved through a node_modules symlink to
                        // a built output (e.g. dist/esm/button/index.js) that has no
                        // src/ mirror. Retry against the workspace root's source tree.
                        // See issue #106.
                        if ctx.workspace_roots.contains_key(pkg_name.as_str())
                            && let Some(result) = try_workspace_package_fallback(ctx, specifier)
                        {
                            return result;
                        }
                        if should_preserve_node_modules_style_file(specifier, from_file, &canonical)
                        {
                            ResolveResult::ExternalFile(canonical)
                        } else {
                            ResolveResult::NpmPackage(pkg_name)
                        }
                    } else {
                        ResolveResult::ExternalFile(canonical)
                    }
                }
                Err(_) => {
                    // Path doesn't exist on disk — try source fallback on the raw path
                    if let Some(file_id) = try_source_fallback(resolved_path, ctx.path_to_id) {
                        ResolveResult::InternalModule(file_id)
                    } else if let Some(file_id) = try_pnpm_workspace_fallback(
                        resolved_path,
                        ctx.path_to_id,
                        ctx.workspace_roots,
                    ) {
                        ResolveResult::InternalModule(file_id)
                    } else if let Some(pkg_name) =
                        extract_package_name_from_node_modules_path(resolved_path)
                    {
                        if ctx.workspace_roots.contains_key(pkg_name.as_str())
                            && let Some(result) = try_workspace_package_fallback(ctx, specifier)
                        {
                            return result;
                        }
                        if should_preserve_node_modules_style_file(
                            specifier,
                            from_file,
                            resolved_path,
                        ) {
                            ResolveResult::ExternalFile(resolved_path.to_path_buf())
                        } else {
                            ResolveResult::NpmPackage(pkg_name)
                        }
                    } else {
                        ResolveResult::ExternalFile(resolved_path.to_path_buf())
                    }
                }
            }
        }
        ResolveFileAttempt::Failed {
            used_tsconfig_fallback,
        } => {
            if let Some(result) = try_scss_fallbacks(ctx, from_file, specifier, from_style) {
                return result;
            }

            if used_tsconfig_fallback
                && let Some(result) = try_tsconfig_root_dirs(ctx, from_file, specifier)
            {
                return result;
            }

            if used_tsconfig_fallback
                && let Some(result) = try_nearest_tsconfig_path_alias(ctx, from_file, specifier)
            {
                return result;
            }

            if used_tsconfig_fallback
                && matches_nearest_tsconfig_path_alias(ctx.root, from_file, specifier)
            {
                // The tsconfig chain was broken, so alias-aware resolution is unavailable.
                // Keep these imports unresolved instead of misclassifying them as npm packages.
                return ResolveResult::Unresolvable(specifier.to_string());
            }

            if is_alias || matches_plugin_alias {
                // Try plugin-provided path aliases before giving up.
                // This covers both built-in alias shapes (`~/`, `@/`, `#foo`) and
                // custom prefixes discovered from framework config files such as
                // `@shared/*` or `$utils/*`.
                // Path aliases that fail resolution are unresolvable, not npm packages.
                // Classifying them as NpmPackage would cause false "unlisted dependency" reports.
                try_path_alias_fallback(ctx, specifier)
                    .unwrap_or_else(|| ResolveResult::Unresolvable(specifier.to_string()))
            } else if let Some(result) =
                try_css_relative_subpath_fallback(ctx, from_file, specifier, from_style)
            {
                result
            } else if is_plain_css_file(from_file) && is_bare_style_subpath(specifier) {
                ResolveResult::Unresolvable(specifier.to_string())
            } else if is_bare && is_valid_package_name(specifier) {
                // Workspace package fallback: self-referencing and cross-workspace
                // imports without node_modules symlinks. Resolves `@org/pkg/sub`
                // against the workspace root's source tree. See issue #106.
                if let Some(result) = try_workspace_package_fallback(ctx, specifier) {
                    return result;
                }
                let pkg_name = extract_package_name(specifier);
                ResolveResult::NpmPackage(pkg_name)
            } else {
                ResolveResult::Unresolvable(specifier.to_string())
            }
        }
    }
}

fn try_tsconfig_root_dirs(
    ctx: &ResolveContext<'_>,
    from_file: &Path,
    specifier: &str,
) -> Option<ResolveResult> {
    if !specifier.starts_with('.') {
        return None;
    }
    for tsconfig_path in local_tsconfig_chain(ctx.root, from_file) {
        let Some(json) = read_tsconfig_json(&tsconfig_path) else {
            continue;
        };
        let Some(compiler_options) = json.get("compilerOptions") else {
            continue;
        };
        let has_root_dirs = compiler_options.get("rootDirs").is_some();
        let Some(root_dirs) = compiler_options.get("rootDirs").and_then(Value::as_array) else {
            continue;
        };
        let tsconfig_dir = tsconfig_path.parent().unwrap_or(ctx.root);
        let roots: Vec<PathBuf> = root_dirs
            .iter()
            .filter_map(Value::as_str)
            .map(|root_dir| {
                let root_dir = Path::new(root_dir);
                if root_dir.is_absolute() {
                    root_dir.to_path_buf()
                } else {
                    tsconfig_dir.join(root_dir)
                }
            })
            .collect();
        let from_dir = from_file.parent().unwrap_or(from_file);
        for root in &roots {
            let Ok(relative_dir) = from_dir.strip_prefix(root) else {
                continue;
            };
            for candidate_root in &roots {
                let candidate = candidate_root.join(relative_dir).join(specifier);
                if let Some(result) = try_tsconfig_alias_target(ctx, &candidate) {
                    return Some(result);
                }
            }
        }
        if has_root_dirs {
            return None;
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};

    use oxc_resolver::{JSONError, ResolveError};
    use tempfile::tempdir;

    use super::{
        glob_values_match, is_tsconfig_error, matches_nearest_tsconfig_path_alias,
        path_alias_capture, path_alias_pattern_matches, resolve_tsconfig_extends_path,
    };

    #[test]
    fn tsconfig_not_found_is_tsconfig_error() {
        assert!(is_tsconfig_error(&ResolveError::TsconfigNotFound(
            PathBuf::from("/nonexistent/tsconfig.json")
        )));
    }

    #[test]
    fn tsconfig_self_reference_is_tsconfig_error() {
        assert!(is_tsconfig_error(&ResolveError::TsconfigSelfReference(
            PathBuf::from("/project/tsconfig.json")
        )));
    }

    // `TsconfigCircularExtend(CircularPathBufs)` is part of the matched set but
    // cannot be directly unit-tested: `CircularPathBufs` is not re-exported from
    // `oxc_resolver::lib`, so external crates cannot construct the variant. The
    // `matches!` arm is structural, so adding the variant to `is_tsconfig_error`
    // is guaranteed by the compiler to return `true` regardless of payload.

    #[test]
    fn io_error_is_tsconfig_error() {
        // An IO error (permission denied while reading a tsconfig) must trigger
        // the fallback. The variant is shared with non-tsconfig IO failures, but
        // the retry via `resolve()` is safe in either case.
        let io_err = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "denied");
        assert!(is_tsconfig_error(&ResolveError::from(io_err)));
    }

    #[test]
    fn json_error_is_tsconfig_error() {
        // Malformed tsconfig JSON surfaces as ResolveError::Json. Same variant
        // covers malformed package.json; retry via `resolve()` is safe there too.
        assert!(is_tsconfig_error(&ResolveError::Json(JSONError {
            path: PathBuf::from("/project/tsconfig.json"),
            message: "unexpected token".to_string(),
            line: 1,
            column: 1,
        })));
    }

    #[test]
    fn module_not_found_is_not_tsconfig_error() {
        // Regular "module not found" must NOT trigger the fallback —
        // the tsconfig loaded fine, the specifier just doesn't exist.
        assert!(!is_tsconfig_error(&ResolveError::NotFound(
            "./missing-module".to_string()
        )));
    }

    #[test]
    fn ignored_is_not_tsconfig_error() {
        assert!(!is_tsconfig_error(&ResolveError::Ignored(PathBuf::from(
            "/ignored"
        ))));
    }

    #[test]
    fn wildcard_tsconfig_path_alias_pattern_matches() {
        assert!(path_alias_pattern_matches("@gen/*", "@gen/foo"));
        assert!(path_alias_pattern_matches("@gen/*", "@gen/nested/foo"));
        assert!(!path_alias_pattern_matches("@gen/*", "@other/foo"));
    }

    #[test]
    fn exact_tsconfig_path_alias_pattern_matches() {
        assert!(path_alias_pattern_matches("$lib", "$lib"));
        assert!(!path_alias_pattern_matches("$lib", "$lib/utils"));
    }

    #[test]
    fn wildcard_tsconfig_path_alias_capture_matches_middle() {
        assert_eq!(
            path_alias_capture("@/*", "@/components/Button"),
            Some("components/Button")
        );
        assert_eq!(
            path_alias_capture("@app/*/test", "@app/foo/bar/test"),
            Some("foo/bar")
        );
        assert_eq!(path_alias_capture("@/*", "@"), None);
    }

    #[test]
    fn wildcard_only_tsconfig_path_alias_pattern_does_not_match_everything() {
        assert!(!path_alias_pattern_matches("*", "@gen/foo"));
    }

    #[test]
    fn wildcard_only_tsconfig_path_alias_capture_can_resolve_targets() {
        assert_eq!(path_alias_capture("*", "shared"), Some("shared"));
    }

    #[cfg_attr(miri, ignore = "tempdir is blocked by Miri isolation")]
    #[test]
    fn bare_directory_tsconfig_include_matches_nested_files() {
        let temp = tempdir().expect("temp dir");
        let root = temp.path();
        fs::create_dir_all(root.join("src/features")).expect("src dir");
        let source = root.join("src/features/button.ts");
        fs::write(&source, "export const button = true;\n").expect("source");

        assert!(glob_values_match(
            root,
            &[serde_json::json!("src")],
            &source
        ));
    }

    #[cfg_attr(miri, ignore = "tempdir is blocked by Miri isolation")]
    #[test]
    fn bare_directory_tsconfig_include_does_not_match_sibling_files() {
        let temp = tempdir().expect("temp dir");
        let root = temp.path();
        fs::create_dir_all(root.join("src")).expect("src dir");
        fs::create_dir_all(root.join("test")).expect("test dir");
        let source = root.join("test/button.ts");
        fs::write(&source, "export const button = true;\n").expect("source");

        assert!(!glob_values_match(
            root,
            &[serde_json::json!("src")],
            &source
        ));
    }

    #[test]
    fn tsconfig_extends_resolution_preserves_explicit_extension() {
        let base = Path::new("/repo/apps/mobile");
        assert_eq!(
            resolve_tsconfig_extends_path(base, "../../tsconfig.base.jsonc"),
            PathBuf::from("/repo/apps/mobile/../../tsconfig.base.jsonc")
        );
        assert_eq!(
            resolve_tsconfig_extends_path(base, "../../tsconfig.base"),
            PathBuf::from("/repo/apps/mobile/../../tsconfig.base.json")
        );
    }

    #[cfg_attr(miri, ignore = "tempdir is blocked by Miri isolation")]
    #[test]
    fn detects_alias_from_nearest_tsconfig_even_when_chain_is_broken() {
        let temp = tempdir().unwrap();
        let project_root = temp.path().join("app");
        let src_dir = project_root.join("src");
        fs::create_dir_all(&src_dir).unwrap();
        let source_file = src_dir.join("index.ts");
        fs::write(&source_file, "import '@gen/foo';").unwrap();
        fs::write(
            project_root.join("tsconfig.json"),
            r#"{
                "extends": "./.svelte-kit/tsconfig.json",
                "compilerOptions": {
                    "paths": {
                        "@gen/*": ["../generated/build/ts/*"]
                    }
                }
            }"#,
        )
        .unwrap();

        assert!(matches_nearest_tsconfig_path_alias(
            &project_root,
            &source_file,
            "@gen/foo"
        ));
        assert!(!matches_nearest_tsconfig_path_alias(
            &project_root,
            &source_file,
            "@other/foo"
        ));
    }
}
