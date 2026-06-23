//! Main resolution engine: creates the oxc_resolver instance and resolves individual specifiers.

use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

use globset::{Glob, GlobSetBuilder};
use oxc_resolver::{Resolution, ResolveError, ResolveOptions, Resolver};
use serde_json::Value;

use super::fallbacks::{
    extract_package_name_from_node_modules_path, try_css_extension_fallback,
    try_package_imports_fallback, try_path_alias_fallback, try_pnpm_workspace_fallback,
    try_relative_package_root_source_fallback, try_scss_include_path_fallback,
    try_scss_node_modules_fallback, try_scss_partial_fallback, try_source_fallback,
    try_workspace_package_fallback,
};
use super::path_info::{
    extract_package_name, is_bare_specifier, is_path_alias, is_valid_package_name,
    normalize_npm_specifier,
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
        extension_alias: vec![
            (
                ".js".into(),
                vec![
                    ".ts".into(),
                    ".tsx".into(),
                    ".gts".into(),
                    ".js".into(),
                    ".gjs".into(),
                ],
            ),
            (".ts".into(), vec![".gts".into(), ".ts".into()]),
            (".jsx".into(), vec![".tsx".into(), ".jsx".into()]),
            (".mjs".into(), vec![".mts".into(), ".mjs".into()]),
            (".cjs".into(), vec![".cts".into(), ".cjs".into()]),
        ],
        condition_names: build_condition_names(active_plugins, extra_conditions),
        main_fields: vec!["module".into(), "main".into()],
        ..Default::default()
    };

    options.tsconfig = Some(oxc_resolver::TsconfigDiscovery::Auto);

    Resolver::new(options)
}

/// Return `true` for errors raised while loading a tsconfig file (as opposed to
/// errors about the specifier itself). When `resolve_file` fails with one of these,
/// a broken sibling tsconfig is poisoning resolution for the current file, retrying
/// via `resolve(dir, specifier)` bypasses `TsconfigDiscovery::Auto` and restores
/// resolution for everything that does not need path aliases (relative, absolute,
/// bare package specifiers).
///
/// `IOError` and `Json` are included because a malformed or unreadable tsconfig
/// surfaces as one of these. The variants are shared with package.json parsing,
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

fn try_root_relative_specifier(
    ctx: &ResolveContext<'_>,
    from_file: &Path,
    specifier: &str,
) -> Option<ResolveResult> {
    if !specifier.starts_with('/') || !is_root_relative_importer(from_file) {
        return None;
    }

    let relative = format!(".{specifier}");
    let source_dir = from_file.parent().unwrap_or(ctx.root);
    if let Some(result) = resolve_root_relative_from_dir(ctx, source_dir, &relative) {
        return Some(result);
    }
    if source_dir != ctx.root
        && let Some(result) = resolve_root_relative_from_dir(ctx, ctx.root, &relative)
    {
        return Some(result);
    }
    if let Some(result) = try_html_public_root_relative_asset(ctx, from_file, specifier) {
        return Some(result);
    }
    Some(ResolveResult::Unresolvable(specifier.to_string()))
}

fn is_root_relative_importer(from_file: &Path) -> bool {
    from_file.extension().is_some_and(|extension| {
        matches!(
            extension.to_str(),
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
}

fn resolve_root_relative_from_dir(
    ctx: &ResolveContext<'_>,
    source_dir: &Path,
    relative: &str,
) -> Option<ResolveResult> {
    let resolved = ctx.resolver.resolve(source_dir, relative).ok()?;
    let resolved_path = resolved.path();
    if let Some(&file_id) = ctx.raw_path_to_id.get(resolved_path) {
        return Some(ResolveResult::InternalModule(file_id));
    }
    let canonical = dunce::canonicalize(resolved_path).ok()?;
    if let Some(&file_id) = ctx.path_to_id.get(canonical.as_path()) {
        return Some(ResolveResult::InternalModule(file_id));
    }
    if let Some(fallback) = ctx.canonical_fallback
        && let Some(file_id) = fallback.get(&canonical)
    {
        return Some(ResolveResult::InternalModule(file_id));
    }
    None
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
    if path
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext == "json" || ext == "jsonc")
    {
        return path;
    }
    if path.is_dir() {
        return path.join("tsconfig.json");
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

fn is_storybook_preview_html(from_file: &Path) -> bool {
    matches!(
        from_file.file_name().and_then(|name| name.to_str()),
        Some("preview-head.html" | "preview-body.html")
    ) && from_file
        .parent()
        .and_then(Path::file_name)
        .and_then(|name| name.to_str())
        == Some(".storybook")
}

/// Return `true` when `specifier` matches the plugin alias `prefix` at a
/// path-segment boundary.
///
/// Mirrors `alias_match_remainder` in `fallbacks.rs`: a prefix that already
/// ends with `/` matches any continuation. For bare prefixes (e.g. `@` or
/// `~`) the match is only admitted when the specifier equals the prefix
/// exactly or the remainder after stripping the prefix starts with `/`, so
/// `prefix = "@"` does NOT match `@radix-ui/react-checkbox`.
fn specifier_matches_alias_prefix(specifier: &str, prefix: &str) -> bool {
    let Some(remainder) = specifier.strip_prefix(prefix) else {
        return false;
    };
    remainder.is_empty() || prefix.ends_with('/') || remainder.starts_with('/')
}

fn strip_url_suffix(specifier: &str) -> &str {
    specifier
        .find(['?', '#'])
        .map_or(specifier, |idx| &specifier[..idx])
}

fn storybook_static_url_path(specifier: &str) -> Option<String> {
    let lookup = strip_url_suffix(specifier);
    if lookup.starts_with("../") {
        return None;
    }
    if lookup.starts_with('/') {
        return Some(lookup.to_string());
    }
    lookup
        .strip_prefix("./")
        .map(|rest| format!("/{rest}"))
        .or_else(|| (!lookup.starts_with('.')).then(|| format!("/{lookup}")))
}

fn static_dir_relative_path<'a>(url_path: &'a str, mount: &str) -> Option<&'a str> {
    if mount == "/" {
        return url_path.strip_prefix('/');
    }
    if url_path == mount {
        return Some("");
    }
    url_path
        .strip_prefix(mount)
        .and_then(|rest| rest.strip_prefix('/'))
}

fn is_safe_static_dir_relative_path(relative: &str) -> bool {
    !Path::new(relative).components().any(|component| {
        matches!(
            component,
            std::path::Component::ParentDir
                | std::path::Component::RootDir
                | std::path::Component::Prefix(_)
        )
    })
}

fn resolve_filesystem_path(ctx: &ResolveContext<'_>, path: &Path) -> Option<ResolveResult> {
    if let Some(&file_id) = ctx.raw_path_to_id.get(path) {
        return Some(ResolveResult::InternalModule(file_id));
    }
    let canonical = dunce::canonicalize(path).ok()?;
    if let Some(&file_id) = ctx.path_to_id.get(canonical.as_path()) {
        return Some(ResolveResult::InternalModule(file_id));
    }
    if let Some(fallback) = ctx.canonical_fallback
        && let Some(file_id) = fallback.get(&canonical)
    {
        return Some(ResolveResult::InternalModule(file_id));
    }
    path.exists()
        .then_some(ResolveResult::ExternalFile(canonical))
}

fn try_storybook_static_dir_mapping(
    ctx: &ResolveContext<'_>,
    from_file: &Path,
    specifier: &str,
) -> Option<ResolveResult> {
    if ctx.static_dir_mappings.is_empty() || !is_storybook_preview_html(from_file) {
        return None;
    }
    let url_path = storybook_static_url_path(specifier)?;
    ctx.static_dir_mappings
        .iter()
        .filter_map(|(from_dir, mount)| {
            let relative = static_dir_relative_path(&url_path, mount)?;
            if !is_safe_static_dir_relative_path(relative) {
                return None;
            }
            Some((mount.len(), from_dir.join(relative)))
        })
        .max_by_key(|(mount_len, _)| *mount_len)
        .and_then(|(_, path)| resolve_filesystem_path(ctx, &path))
}

fn try_html_public_root_relative_asset(
    ctx: &ResolveContext<'_>,
    from_file: &Path,
    specifier: &str,
) -> Option<ResolveResult> {
    if from_file.extension().and_then(|ext| ext.to_str()) != Some("html") {
        return None;
    }

    let relative = strip_url_suffix(specifier).strip_prefix('/')?;
    if !is_safe_static_dir_relative_path(relative) {
        return None;
    }

    resolve_filesystem_path(ctx, &ctx.root.join("public").join(relative))
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
    if path
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext == "json" || ext == "jsonc")
    {
        return path;
    }
    if path.is_dir() {
        return path.join("tsconfig.json");
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
    if let Some(result) = try_css_extension_fallback(ctx, from_file, specifier) {
        return Some(result);
    }
    if let Some(result) = try_scss_partial_fallback(ctx, from_file, specifier) {
        return Some(result);
    }
    if let Some(result) = try_scss_include_path_fallback(ctx, from_file, specifier, from_style) {
        return Some(result);
    }
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

fn is_bare_style_package_reference(specifier: &str) -> bool {
    if !is_bare_specifier(specifier) {
        return false;
    }

    is_bare_style_subpath(specifier) || extract_package_name(specifier) == specifier
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
    from_style: bool,
) -> bool {
    if !is_style_file(resolved_path) || !is_node_modules_path(resolved_path) {
        return false;
    }

    let is_bare_subpath =
        is_bare_specifier(specifier) && extract_package_name(specifier).as_str() != specifier;
    if is_bare_subpath {
        return true;
    }

    if is_bare_specifier(specifier) && (from_style || is_style_file(from_file)) {
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
    if !is_bare_style_package_reference(specifier) || (!from_style && !is_style_file(from_file)) {
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
    if !is_style_file(resolved_path) {
        return None;
    }

    if let Some(&file_id) = ctx.raw_path_to_id.get(resolved_path) {
        return Some(ResolveResult::InternalModule(file_id));
    }

    if should_preserve_node_modules_style_file(specifier, from_file, resolved_path, from_style) {
        return Some(ResolveResult::ExternalFile(resolved_path.to_path_buf()));
    }

    if let Some(pkg_name) = package_usage_name_for_resolved_package(specifier, resolved_path)
        && !ctx.workspace_roots.contains_key(pkg_name.as_str())
    {
        return Some(ResolveResult::NpmPackage(pkg_name));
    }

    if let Ok(canonical) = dunce::canonicalize(resolved_path) {
        return Some(resolve_style_canonical_path(
            ctx, from_file, specifier, from_style, canonical,
        ));
    }

    package_usage_name_for_resolved_package(specifier, resolved_path)
        .map(ResolveResult::NpmPackage)
        .or_else(|| Some(ResolveResult::ExternalFile(resolved_path.to_path_buf())))
}

/// Classify the canonicalized style-resolver target: internal module via the
/// id maps and fallbacks, preserved external `node_modules` style file, npm
/// package, or external file.
fn resolve_style_canonical_path(
    ctx: &ResolveContext<'_>,
    from_file: &Path,
    specifier: &str,
    from_style: bool,
    canonical: PathBuf,
) -> ResolveResult {
    if let Some(&file_id) = ctx.path_to_id.get(canonical.as_path()) {
        return ResolveResult::InternalModule(file_id);
    }
    if let Some(fallback) = ctx.canonical_fallback
        && let Some(file_id) = fallback.get(&canonical)
    {
        return ResolveResult::InternalModule(file_id);
    }
    if let Some(file_id) = try_source_fallback(&canonical, ctx.path_to_id) {
        return ResolveResult::InternalModule(file_id);
    }
    if let Some(file_id) =
        try_pnpm_workspace_fallback(&canonical, ctx.path_to_id, ctx.workspace_roots)
    {
        return ResolveResult::InternalModule(file_id);
    }
    if should_preserve_node_modules_style_file(specifier, from_file, &canonical, from_style) {
        return ResolveResult::ExternalFile(canonical);
    }
    if let Some(pkg_name) = package_usage_name_for_resolved_package(specifier, &canonical)
        && !ctx.workspace_roots.contains_key(pkg_name.as_str())
    {
        return ResolveResult::NpmPackage(pkg_name);
    }
    if let Some(pkg_name) = package_usage_name_for_external_bare_specifier(specifier) {
        return ResolveResult::NpmPackage(pkg_name);
    }
    ResolveResult::ExternalFile(canonical)
}

/// Package-usage key for an import that resolved into `node_modules`.
///
/// pnpm can install a package under a declared name (e.g. `unstorage`) symlinked
/// to a `.pnpm` store whose inner package has a different "source" name (e.g.
/// `unstorage-nightly`). For a plain bare specifier we credit the declared import
/// name so `unused-dependency` / `unlisted-dependency` accounting matches what the
/// user wrote in `package.json`, not the on-disk source package. Path aliases that
/// happen to be bare (Node.js `#imports`, `~/`, `@/`, PascalCase scopes) are
/// excluded: they can map to an external package whose real name is only recoverable
/// from the resolved path, so they keep the resolved-package name. Returns `None`
/// when the path is not inside a `node_modules` directory.
pub(super) fn package_usage_name_for_resolved_package(
    specifier: &str,
    resolved_path: &Path,
) -> Option<String> {
    let resolved_package = extract_package_name_from_node_modules_path(resolved_path)?;
    if is_bare_specifier(specifier) && !is_path_alias(specifier) {
        Some(extract_package_name(specifier))
    } else {
        Some(resolved_package)
    }
}

/// Package-usage key for a valid bare package specifier whose resolved file
/// canonicalized outside the analyzed root before a `node_modules` segment could
/// be observed. This preserves dependency accounting for pnpm workspace symlinks
/// when users analyze a workspace package directly.
pub(super) fn package_usage_name_for_external_bare_specifier(specifier: &str) -> Option<String> {
    if !is_bare_specifier(specifier) || is_path_alias(specifier) {
        return None;
    }
    if specifier.starts_with('@') && specifier.split('/').nth(1).is_none_or(str::is_empty) {
        return None;
    }

    let package_name = extract_package_name(specifier);
    is_valid_package_name(&package_name).then_some(package_name)
}

struct ResolvedPathContext<'ctx, 'resolve> {
    ctx: &'resolve ResolveContext<'ctx>,
    from_file: &'resolve Path,
    specifier: &'resolve str,
    from_style: bool,
}

impl ResolvedPathContext<'_, '_> {
    fn resolve(&self, resolved_path: &Path) -> ResolveResult {
        if let Some(&file_id) = self.ctx.raw_path_to_id.get(resolved_path) {
            return ResolveResult::InternalModule(file_id);
        }
        if let Some(result) = self.bare_package_result(resolved_path) {
            return result;
        }

        match dunce::canonicalize(resolved_path) {
            Ok(canonical) => self.resolve_canonical_path(canonical),
            Err(_) => self.resolve_uncanonicalized_path(resolved_path),
        }
    }

    fn bare_package_result(&self, resolved_path: &Path) -> Option<ResolveResult> {
        if !is_bare_specifier(self.specifier)
            || resolved_path
                .as_os_str()
                .as_encoded_bytes()
                .windows(7)
                .any(|window| window == b"/.pnpm/" || window == b"\\.pnpm\\")
        {
            return None;
        }
        let pkg_name = package_usage_name_for_resolved_package(self.specifier, resolved_path)?;
        if self.ctx.workspace_roots.contains_key(pkg_name.as_str()) {
            return None;
        }
        Some(self.package_result_or_preserved_file(pkg_name, resolved_path))
    }

    fn resolve_canonical_path(&self, canonical: PathBuf) -> ResolveResult {
        if let Some(&file_id) = self.ctx.path_to_id.get(canonical.as_path()) {
            ResolveResult::InternalModule(file_id)
        } else if let Some(fallback) = self.ctx.canonical_fallback
            && let Some(file_id) = fallback.get(&canonical)
        {
            ResolveResult::InternalModule(file_id)
        } else if let Some(file_id) = try_source_fallback(&canonical, self.ctx.path_to_id) {
            ResolveResult::InternalModule(file_id)
        } else if let Some(file_id) =
            try_pnpm_workspace_fallback(&canonical, self.ctx.path_to_id, self.ctx.workspace_roots)
        {
            ResolveResult::InternalModule(file_id)
        } else if let Some(result) = self.package_or_external_result(&canonical) {
            result
        } else {
            ResolveResult::ExternalFile(canonical)
        }
    }

    fn resolve_uncanonicalized_path(&self, resolved_path: &Path) -> ResolveResult {
        if let Some(file_id) = try_source_fallback(resolved_path, self.ctx.path_to_id) {
            ResolveResult::InternalModule(file_id)
        } else if let Some(file_id) = try_pnpm_workspace_fallback(
            resolved_path,
            self.ctx.path_to_id,
            self.ctx.workspace_roots,
        ) {
            ResolveResult::InternalModule(file_id)
        } else if let Some(result) = self.package_or_external_result(resolved_path) {
            result
        } else {
            ResolveResult::ExternalFile(resolved_path.to_path_buf())
        }
    }

    fn package_or_external_result(&self, path: &Path) -> Option<ResolveResult> {
        if let Some(pkg_name) = package_usage_name_for_resolved_package(self.specifier, path) {
            if self.ctx.workspace_roots.contains_key(pkg_name.as_str())
                && let Some(result) = try_workspace_package_fallback(self.ctx, self.specifier)
            {
                return Some(result);
            }
            return Some(self.package_result_or_preserved_file(pkg_name, path));
        }

        package_usage_name_for_external_bare_specifier(self.specifier)
            .map(ResolveResult::NpmPackage)
    }

    fn package_result_or_preserved_file(&self, pkg_name: String, path: &Path) -> ResolveResult {
        if should_preserve_node_modules_style_file(
            self.specifier,
            self.from_file,
            path,
            self.from_style,
        ) {
            ResolveResult::ExternalFile(path.to_path_buf())
        } else {
            ResolveResult::NpmPackage(pkg_name)
        }
    }
}

/// Outcome of normalizing a specifier's scheme prefix.
enum SpecifierNormalization {
    /// The specifier is an external scheme (`jsr:`, URL, `data:`, empty `npm:`).
    External(ResolveResult),
    /// The specifier is resolvable; carries the scheme-stripped form.
    Resolvable(String),
}

/// Strip `jsr:` / `npm:` / URL / `data:` schemes, short-circuiting to an
/// external result when the scheme is not internally resolvable.
fn normalize_resolve_specifier(specifier: &str) -> SpecifierNormalization {
    if specifier.starts_with("jsr:") {
        return SpecifierNormalization::External(ResolveResult::ExternalFile(PathBuf::from(
            specifier,
        )));
    }
    let resolvable = if let Some(rest) = specifier.strip_prefix("npm:") {
        let normalized = normalize_npm_specifier(rest);
        if normalized.is_empty() {
            return SpecifierNormalization::External(ResolveResult::ExternalFile(PathBuf::from(
                specifier,
            )));
        }
        normalized
    } else {
        specifier.to_string()
    };

    if resolvable.contains("://") || resolvable.starts_with("data:") {
        return SpecifierNormalization::External(ResolveResult::ExternalFile(PathBuf::from(
            resolvable,
        )));
    }
    SpecifierNormalization::Resolvable(resolvable)
}

/// Resolve a single import specifier to a target.
///
/// `from_style` is `true` for imports extracted from CSS contexts (currently
/// SFC `<style lang="scss">` blocks and `<style src>` references). It enables
/// SCSS partial / include-path / node_modules fallbacks for SFC importers
/// without applying them to JS-context imports from the same file.
pub(super) fn resolve_specifier(
    ctx: &ResolveContext<'_>,
    from_file: &Path,
    specifier: &str,
    from_style: bool,
) -> ResolveResult {
    let normalized;
    let specifier = match normalize_resolve_specifier(specifier) {
        SpecifierNormalization::External(result) => return result,
        SpecifierNormalization::Resolvable(spec) => {
            normalized = spec;
            normalized.as_str()
        }
    };

    if let Some(result) = try_pre_file_resolution_fallbacks(ctx, from_file, specifier, from_style) {
        return result;
    }

    let flags = FailedSpecifierFlags::new(ctx, specifier);
    resolve_file_or_failed(ctx, from_file, specifier, from_style, flags)
}

fn try_pre_file_resolution_fallbacks(
    ctx: &ResolveContext<'_>,
    from_file: &Path,
    specifier: &str,
    from_style: bool,
) -> Option<ResolveResult> {
    if let Some(result) = try_storybook_static_dir_mapping(ctx, from_file, specifier) {
        return Some(result);
    }

    if let Some(result) = try_root_relative_specifier(ctx, from_file, specifier) {
        return Some(result);
    }

    if from_style && let Some(result) = try_scss_fallbacks(ctx, from_file, specifier, true) {
        return Some(result);
    }

    try_style_condition_package_resolution(ctx, from_file, specifier, from_style)
}

fn resolve_file_or_failed(
    ctx: &ResolveContext<'_>,
    from_file: &Path,
    specifier: &str,
    from_style: bool,
    flags: FailedSpecifierFlags,
) -> ResolveResult {
    match resolve_file_with_tsconfig_fallback(ctx, from_file, specifier) {
        ResolveFileAttempt::Resolved {
            resolution: resolved,
            used_tsconfig_fallback,
        } => resolve_resolved_specifier(
            ctx,
            from_file,
            specifier,
            from_style,
            resolved.path(),
            FailedSpecifierFlags {
                used_tsconfig_fallback,
                ..flags
            },
        ),
        ResolveFileAttempt::Failed {
            used_tsconfig_fallback,
        } => resolve_failed_specifier(
            ctx,
            from_file,
            specifier,
            from_style,
            FailedSpecifierFlags {
                used_tsconfig_fallback,
                ..flags
            },
        ),
    }
}

/// Map a successfully resolved file to a `ResolveResult`, applying the
/// SCSS-importer JS/TS rejection and tsconfig-fallback alias retries before
/// classifying the resolved path.
fn resolve_resolved_specifier(
    ctx: &ResolveContext<'_>,
    from_file: &Path,
    specifier: &str,
    from_style: bool,
    resolved_path: &Path,
    flags: FailedSpecifierFlags,
) -> ResolveResult {
    let is_scss_importer = from_file
        .extension()
        .is_some_and(|e| e == "scss" || e == "sass");
    if is_scss_importer && is_js_ts_extension(resolved_path) {
        if let Some(result) = try_scss_fallbacks(ctx, from_file, specifier, from_style) {
            return result;
        }
        return ResolveResult::Unresolvable(specifier.to_string());
    }
    if flags.used_tsconfig_fallback {
        if let Some(result) = try_tsconfig_root_dirs(ctx, from_file, specifier) {
            return result;
        }
        if (flags.is_bare || flags.is_alias || flags.matches_plugin_alias)
            && let Some(result) = try_nearest_tsconfig_path_alias(ctx, from_file, specifier)
        {
            return result;
        }
        if matches_nearest_tsconfig_path_alias(ctx.root, from_file, specifier) {
            return ResolveResult::Unresolvable(specifier.to_string());
        }
    }
    ResolvedPathContext {
        ctx,
        from_file,
        specifier,
        from_style,
    }
    .resolve(resolved_path)
}

#[derive(Clone, Copy)]
struct FailedSpecifierFlags {
    used_tsconfig_fallback: bool,
    is_bare: bool,
    is_alias: bool,
    matches_plugin_alias: bool,
}

impl FailedSpecifierFlags {
    fn new(ctx: &ResolveContext<'_>, specifier: &str) -> Self {
        Self {
            used_tsconfig_fallback: false,
            is_bare: is_bare_specifier(specifier),
            is_alias: is_path_alias(specifier),
            matches_plugin_alias: ctx
                .path_aliases
                .iter()
                .any(|(prefix, _)| specifier_matches_alias_prefix(specifier, prefix)),
        }
    }
}

fn resolve_failed_specifier(
    ctx: &ResolveContext<'_>,
    from_file: &Path,
    specifier: &str,
    from_style: bool,
    flags: FailedSpecifierFlags,
) -> ResolveResult {
    if let Some(result) = try_failed_primary_fallbacks(
        ctx,
        from_file,
        specifier,
        from_style,
        flags.used_tsconfig_fallback,
        flags.is_bare || flags.is_alias || flags.matches_plugin_alias,
    ) {
        return result;
    }

    let matches_tsconfig_path_alias =
        matches_nearest_tsconfig_path_alias(ctx.root, from_file, specifier);

    if let Some(result) = try_failed_package_fallbacks(ctx, from_file, specifier) {
        return result;
    }

    if flags.is_alias || flags.matches_plugin_alias {
        return resolve_failed_alias_specifier(
            ctx,
            specifier,
            flags.is_bare,
            matches_tsconfig_path_alias,
        );
    }
    resolve_failed_regular_specifier(
        ctx,
        from_file,
        specifier,
        from_style,
        flags.is_bare,
        matches_tsconfig_path_alias,
    )
}

fn resolve_failed_regular_specifier(
    ctx: &ResolveContext<'_>,
    from_file: &Path,
    specifier: &str,
    from_style: bool,
    is_bare: bool,
    matches_tsconfig_path_alias: bool,
) -> ResolveResult {
    if let Some(result) = try_css_relative_subpath_fallback(ctx, from_file, specifier, from_style) {
        return result;
    }
    if is_plain_css_file(from_file) && is_bare_style_subpath(specifier) {
        return ResolveResult::Unresolvable(specifier.to_string());
    }
    if is_bare && is_valid_package_name(specifier) {
        return resolve_failed_bare_package_specifier(ctx, specifier, matches_tsconfig_path_alias);
    }
    ResolveResult::Unresolvable(specifier.to_string())
}

fn try_failed_primary_fallbacks(
    ctx: &ResolveContext<'_>,
    from_file: &Path,
    specifier: &str,
    from_style: bool,
    used_tsconfig_fallback: bool,
    can_try_nearest_alias: bool,
) -> Option<ResolveResult> {
    if let Some(result) = try_scss_fallbacks(ctx, from_file, specifier, from_style) {
        return Some(result);
    }

    if used_tsconfig_fallback
        && let Some(result) = try_tsconfig_root_dirs(ctx, from_file, specifier)
    {
        return Some(result);
    }

    if (used_tsconfig_fallback || can_try_nearest_alias)
        && let Some(result) = try_nearest_tsconfig_path_alias(ctx, from_file, specifier)
    {
        return Some(result);
    }

    None
}

fn try_failed_package_fallbacks(
    ctx: &ResolveContext<'_>,
    from_file: &Path,
    specifier: &str,
) -> Option<ResolveResult> {
    if let Some(result) = try_package_imports_fallback(ctx, from_file, specifier) {
        return Some(result);
    }
    try_relative_package_root_source_fallback(ctx, from_file, specifier)
}

fn resolve_failed_alias_specifier(
    ctx: &ResolveContext<'_>,
    specifier: &str,
    is_bare: bool,
    matches_tsconfig_path_alias: bool,
) -> ResolveResult {
    if let Some(result) = try_path_alias_fallback(ctx, specifier) {
        return result;
    }
    if is_bare
        && is_valid_package_name(specifier)
        && let Some(result) = try_workspace_package_fallback(ctx, specifier)
    {
        return result;
    }
    if matches_tsconfig_path_alias {
        return ResolveResult::Unresolvable(specifier.to_string());
    }
    ResolveResult::Unresolvable(specifier.to_string())
}

fn resolve_failed_bare_package_specifier(
    ctx: &ResolveContext<'_>,
    specifier: &str,
    matches_tsconfig_path_alias: bool,
) -> ResolveResult {
    if let Some(result) = try_workspace_package_fallback(ctx, specifier) {
        return result;
    }
    if matches_tsconfig_path_alias {
        return ResolveResult::Unresolvable(specifier.to_string());
    }
    let pkg_name = extract_package_name(specifier);
    if ctx.workspace_roots.contains_key(pkg_name.as_str())
        || ctx
            .package_manifests
            .iter()
            .any(|manifest| manifest.name.as_deref() == Some(pkg_name.as_str()))
    {
        ResolveResult::Unresolvable(specifier.to_string())
    } else {
        ResolveResult::NpmPackage(pkg_name)
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
        SpecifierNormalization, extension_alias_matches, glob_values_match, has_glob_meta,
        is_bare_style_package_reference, is_bare_style_subpath, is_js_ts_extension,
        is_node_modules_path, is_plain_css_file, is_relative_tsconfig_extends,
        is_safe_static_dir_relative_path, is_storybook_preview_html, is_style_file,
        is_tsconfig_error, matches_nearest_tsconfig_path_alias, normalize_resolve_specifier,
        package_usage_name_for_external_bare_specifier, package_usage_name_for_resolved_package,
        path_alias_capture, path_alias_pattern_matches, path_alias_specificity,
        resolve_tsconfig_extends_candidate, resolve_tsconfig_extends_path,
        resolve_tsconfig_reference_path, should_preserve_node_modules_style_file,
        specifier_matches_alias_prefix, static_dir_relative_path, storybook_static_url_path,
        strip_url_suffix, tsconfig_extends_values, with_appended_extension, with_exact_extension,
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

    #[test]
    fn io_error_is_tsconfig_error() {
        let io_err = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "denied");
        assert!(is_tsconfig_error(&ResolveError::from(io_err)));
    }

    #[test]
    fn json_error_is_tsconfig_error() {
        assert!(is_tsconfig_error(&ResolveError::Json(JSONError {
            path: PathBuf::from("/project/tsconfig.json"),
            message: "unexpected token".to_string(),
            line: 1,
            column: 1,
        })));
    }

    #[test]
    fn module_not_found_is_not_tsconfig_error() {
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

    #[test]
    fn static_dir_relative_path_safety_rejects_escape_paths() {
        assert!(is_safe_static_dir_relative_path("js/app.js"));
        assert!(is_safe_static_dir_relative_path("style/index.css"));
        assert!(!is_safe_static_dir_relative_path("../secret.js"));
        assert!(!is_safe_static_dir_relative_path("/absolute.js"));
    }

    #[cfg(windows)]
    #[test]
    fn static_dir_relative_path_safety_rejects_windows_prefix() {
        assert!(!is_safe_static_dir_relative_path("C:/secret.js"));
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

    // ---- normalize_resolve_specifier (lines 1285-1308) ----

    #[test]
    fn jsr_scheme_is_external() {
        let result = normalize_resolve_specifier("jsr:@std/path");
        assert!(
            matches!(result, SpecifierNormalization::External(_)),
            "jsr: scheme must short-circuit to External"
        );
    }

    #[test]
    fn npm_scheme_with_version_strips_to_bare_package() {
        let result = normalize_resolve_specifier("npm:preact@10/hooks");
        let SpecifierNormalization::Resolvable(s) = result else {
            panic!("npm: with version should be Resolvable");
        };
        assert_eq!(s, "preact/hooks");
    }

    #[test]
    fn npm_scheme_scoped_package_with_version_strips_version() {
        let result = normalize_resolve_specifier("npm:@supabase/supabase-js@2");
        let SpecifierNormalization::Resolvable(s) = result else {
            panic!("npm: scoped should be Resolvable");
        };
        assert_eq!(s, "@supabase/supabase-js");
    }

    #[test]
    fn npm_scheme_empty_body_is_external() {
        let result = normalize_resolve_specifier("npm:");
        assert!(
            matches!(result, SpecifierNormalization::External(_)),
            "bare npm: with no body must be External"
        );
    }

    #[test]
    fn https_url_is_external() {
        let result = normalize_resolve_specifier("https://deno.land/std/path.ts");
        assert!(
            matches!(result, SpecifierNormalization::External(_)),
            "https:// URL must be External"
        );
    }

    #[test]
    fn data_uri_is_external() {
        let result = normalize_resolve_specifier("data:text/javascript,export default 42;");
        assert!(
            matches!(result, SpecifierNormalization::External(_)),
            "data: URI must be External"
        );
    }

    #[test]
    fn plain_bare_specifier_is_resolvable() {
        let result = normalize_resolve_specifier("react");
        let SpecifierNormalization::Resolvable(s) = result else {
            panic!("plain bare specifier must be Resolvable");
        };
        assert_eq!(s, "react");
    }

    // ---- specifier_matches_alias_prefix (lines 412-417) ----

    #[test]
    fn alias_prefix_bare_at_sign_does_not_match_scoped_package() {
        assert!(
            !specifier_matches_alias_prefix("@radix-ui/react-checkbox", "@"),
            "bare '@' must NOT match scoped packages"
        );
    }

    #[test]
    fn alias_prefix_bare_at_sign_matches_exact() {
        assert!(
            specifier_matches_alias_prefix("@", "@"),
            "bare '@' must match exactly '@'"
        );
    }

    #[test]
    fn alias_prefix_with_trailing_slash_matches_any_continuation() {
        assert!(
            specifier_matches_alias_prefix("@/components/Button", "@/"),
            "'@/' prefix matches any continuation"
        );
    }

    #[test]
    fn alias_prefix_tilde_matches_tilde_slash_continuation() {
        assert!(
            specifier_matches_alias_prefix("~/utils", "~"),
            "'~' prefix matches '~/utils' (slash continuation)"
        );
    }

    #[test]
    fn alias_prefix_tilde_does_not_match_tilde_name() {
        assert!(
            !specifier_matches_alias_prefix("~utils", "~"),
            "'~' prefix must NOT match '~utils' (no slash)"
        );
    }

    // ---- strip_url_suffix (lines 419-423) ----

    #[test]
    fn strip_url_suffix_removes_query_string() {
        assert_eq!(strip_url_suffix("./style.css?inline"), "./style.css");
    }

    #[test]
    fn strip_url_suffix_removes_hash_fragment() {
        assert_eq!(strip_url_suffix("./page.html#section"), "./page.html");
    }

    #[test]
    fn strip_url_suffix_passes_through_plain_specifier() {
        assert_eq!(strip_url_suffix("react"), "react");
    }

    // ---- storybook_static_url_path (lines 425-437) ----

    #[test]
    fn storybook_static_url_path_root_relative_returns_unchanged() {
        assert_eq!(
            storybook_static_url_path("/js/app.js"),
            Some("/js/app.js".to_string())
        );
    }

    #[test]
    fn storybook_static_url_path_dot_slash_prepends_root() {
        assert_eq!(
            storybook_static_url_path("./icons/logo.svg"),
            Some("/icons/logo.svg".to_string())
        );
    }

    #[test]
    fn storybook_static_url_path_bare_prepends_root() {
        assert_eq!(
            storybook_static_url_path("fonts/inter.woff2"),
            Some("/fonts/inter.woff2".to_string())
        );
    }

    #[test]
    fn storybook_static_url_path_parent_relative_returns_none() {
        assert_eq!(storybook_static_url_path("../escape.js"), None);
    }

    #[test]
    fn storybook_static_url_path_strips_query_before_prefix() {
        assert_eq!(
            storybook_static_url_path("/js/app.js?v=1"),
            Some("/js/app.js".to_string())
        );
    }

    // ---- static_dir_relative_path (lines 439-449) ----

    #[test]
    fn static_dir_relative_path_root_mount_strips_leading_slash() {
        assert_eq!(
            static_dir_relative_path("/icons/logo.svg", "/"),
            Some("icons/logo.svg")
        );
    }

    #[test]
    fn static_dir_relative_path_exact_mount_returns_empty() {
        assert_eq!(static_dir_relative_path("/assets", "/assets"), Some(""));
    }

    #[test]
    fn static_dir_relative_path_subdirectory_strips_mount_and_slash() {
        assert_eq!(
            static_dir_relative_path("/assets/logo.png", "/assets"),
            Some("logo.png")
        );
    }

    #[test]
    fn static_dir_relative_path_non_matching_mount_returns_none() {
        assert_eq!(static_dir_relative_path("/other/file.js", "/assets"), None);
    }

    // ---- is_node_modules_path (lines 1011-1016) ----

    #[test]
    fn is_node_modules_path_detects_node_modules_segment() {
        let p = PathBuf::from("/project/node_modules/react/index.js");
        assert!(is_node_modules_path(&p));
    }

    #[test]
    fn is_node_modules_path_rejects_project_src() {
        let p = PathBuf::from("/project/src/index.ts");
        assert!(!is_node_modules_path(&p));
    }

    #[test]
    fn is_node_modules_path_detects_nested_node_modules() {
        let p = PathBuf::from("/project/packages/ui/node_modules/.pnpm/lodash/index.js");
        assert!(is_node_modules_path(&p));
    }

    // ---- is_style_file / is_js_ts_extension / is_plain_css_file ----

    #[test]
    fn is_style_file_recognizes_css_scss_sass() {
        assert!(is_style_file(Path::new("style.css")));
        assert!(is_style_file(Path::new("theme.scss")));
        assert!(is_style_file(Path::new("mixin.sass")));
        assert!(!is_style_file(Path::new("index.ts")));
    }

    #[test]
    fn is_js_ts_extension_recognizes_ts_family() {
        assert!(is_js_ts_extension(Path::new("file.ts")));
        assert!(is_js_ts_extension(Path::new("component.tsx")));
        assert!(is_js_ts_extension(Path::new("module.mts")));
        assert!(is_js_ts_extension(Path::new("module.cts")));
        assert!(!is_js_ts_extension(Path::new("style.css")));
        assert!(!is_js_ts_extension(Path::new("file.scss")));
    }

    #[test]
    fn is_plain_css_file_matches_only_css_extension() {
        assert!(is_plain_css_file(Path::new("main.css")));
        assert!(!is_plain_css_file(Path::new("main.scss")));
        assert!(!is_plain_css_file(Path::new("main.ts")));
    }

    // ---- is_bare_style_subpath / is_bare_style_package_reference ----

    #[test]
    fn is_bare_style_subpath_matches_package_with_css_subpath() {
        assert!(is_bare_style_subpath(
            "bootstrap/dist/css/bootstrap.min.css"
        ));
        assert!(is_bare_style_subpath("@fontsource/roboto/400.css"));
    }

    #[test]
    fn is_bare_style_subpath_rejects_bare_package_root() {
        assert!(!is_bare_style_subpath("bootstrap"));
    }

    #[test]
    fn is_bare_style_subpath_rejects_relative_path() {
        assert!(!is_bare_style_subpath("./local.css"));
    }

    #[test]
    fn is_bare_style_package_reference_accepts_bare_root() {
        assert!(is_bare_style_package_reference("bootstrap"));
    }

    #[test]
    fn is_bare_style_package_reference_rejects_relative_specifier() {
        assert!(!is_bare_style_package_reference("./styles.css"));
    }

    // ---- should_preserve_node_modules_style_file (lines 1018-1039) ----

    #[test]
    fn should_preserve_preserves_bare_subpath_css_from_node_modules() {
        let specifier = "bootstrap/dist/css/bootstrap.min.css";
        let from_file = Path::new("/project/src/main.css");
        let resolved = Path::new("/project/node_modules/bootstrap/dist/css/bootstrap.min.css");
        assert!(should_preserve_node_modules_style_file(
            specifier, from_file, resolved, false
        ));
    }

    #[test]
    fn should_preserve_preserves_bare_root_css_from_style_importer() {
        let specifier = "animate.css";
        let from_file = Path::new("/project/src/main.scss");
        let resolved = Path::new("/project/node_modules/animate.css/animate.min.css");
        assert!(should_preserve_node_modules_style_file(
            specifier, from_file, resolved, false
        ));
    }

    #[test]
    fn should_preserve_does_not_preserve_non_style_file() {
        let specifier = "react";
        let from_file = Path::new("/project/src/index.ts");
        let resolved = Path::new("/project/node_modules/react/index.js");
        assert!(!should_preserve_node_modules_style_file(
            specifier, from_file, resolved, false
        ));
    }

    // ---- package_usage_name_for_resolved_package (lines 1144-1153) ----

    #[test]
    fn package_usage_name_bare_specifier_uses_import_name() {
        let specifier = "lodash";
        let resolved = Path::new("/project/node_modules/lodash/index.js");
        let name = package_usage_name_for_resolved_package(specifier, resolved);
        assert_eq!(name, Some("lodash".to_string()));
    }

    #[test]
    fn package_usage_name_scoped_bare_specifier_uses_import_name() {
        let specifier = "@babel/core";
        let resolved = Path::new("/project/node_modules/@babel/core/lib/index.js");
        let name = package_usage_name_for_resolved_package(specifier, resolved);
        assert_eq!(name, Some("@babel/core".to_string()));
    }

    #[test]
    fn package_usage_name_non_node_modules_path_returns_none() {
        let specifier = "./local";
        let resolved = Path::new("/project/src/local.ts");
        let name = package_usage_name_for_resolved_package(specifier, resolved);
        assert_eq!(name, None);
    }

    // ---- package_usage_name_for_external_bare_specifier (lines 1160-1169) ----

    #[test]
    fn external_bare_specifier_valid_package_returns_name() {
        let result = package_usage_name_for_external_bare_specifier("react");
        assert_eq!(result, Some("react".to_string()));
    }

    #[test]
    fn external_bare_specifier_scoped_package_returns_scoped_name() {
        let result = package_usage_name_for_external_bare_specifier("@babel/core");
        assert_eq!(result, Some("@babel/core".to_string()));
    }

    #[test]
    fn external_bare_specifier_relative_path_returns_none() {
        let result = package_usage_name_for_external_bare_specifier("./local");
        assert_eq!(result, None);
    }

    #[test]
    fn external_bare_specifier_path_alias_returns_none() {
        let result = package_usage_name_for_external_bare_specifier("@/components");
        assert_eq!(result, None);
    }

    #[test]
    fn external_bare_specifier_bare_at_sign_only_returns_none() {
        let result = package_usage_name_for_external_bare_specifier("@");
        assert_eq!(result, None);
    }

    // ---- has_glob_meta (lines 370-374) ----

    #[test]
    fn has_glob_meta_detects_star_wildcard() {
        assert!(has_glob_meta("src/**/*.ts"));
    }

    #[test]
    fn has_glob_meta_detects_question_mark() {
        assert!(has_glob_meta("src/?.ts"));
    }

    #[test]
    fn has_glob_meta_detects_bracket_group() {
        assert!(has_glob_meta("src/[abc].ts"));
    }

    #[test]
    fn has_glob_meta_detects_brace_group() {
        assert!(has_glob_meta("src/{a,b}.ts"));
    }

    #[test]
    fn has_glob_meta_plain_path_returns_false() {
        assert!(!has_glob_meta("src/components"));
    }

    // ---- path_alias_specificity (lines 734-738) ----

    #[test]
    fn path_alias_specificity_exact_pattern_is_max() {
        assert_eq!(path_alias_specificity("$lib"), usize::MAX);
    }

    #[test]
    fn path_alias_specificity_wildcard_pattern_counts_prefix_and_suffix_length() {
        assert_eq!(path_alias_specificity("@app/*"), "@app/".len());
    }

    #[test]
    fn path_alias_specificity_longer_prefix_is_more_specific_than_shorter() {
        assert!(path_alias_specificity("@app/foo/*") > path_alias_specificity("@app/*"));
    }

    // ---- extension_alias_matches (lines 855-867) ----

    #[test]
    fn extension_alias_matches_js_to_ts() {
        assert!(extension_alias_matches("js", ".ts"));
        assert!(extension_alias_matches("js", ".tsx"));
        assert!(extension_alias_matches("js", ".js"));
    }

    #[test]
    fn extension_alias_matches_jsx_to_tsx() {
        assert!(extension_alias_matches("jsx", ".tsx"));
        assert!(extension_alias_matches("jsx", ".jsx"));
        assert!(!extension_alias_matches("jsx", ".ts"));
    }

    #[test]
    fn extension_alias_matches_mjs_to_mts() {
        assert!(extension_alias_matches("mjs", ".mts"));
        assert!(extension_alias_matches("mjs", ".mjs"));
        assert!(!extension_alias_matches("mjs", ".ts"));
    }

    #[test]
    fn extension_alias_matches_cjs_to_cts() {
        assert!(extension_alias_matches("cjs", ".cts"));
        assert!(extension_alias_matches("cjs", ".cjs"));
        assert!(!extension_alias_matches("cjs", ".js"));
    }

    #[test]
    fn extension_alias_matches_unknown_import_ext_returns_false() {
        assert!(!extension_alias_matches("ts", ".js"));
    }

    // ---- with_appended_extension / with_exact_extension (lines 808-876) ----

    #[test]
    fn with_appended_extension_appends_to_extensionless_path() {
        let path = Path::new("/project/src/Button");
        let result = with_appended_extension(path, ".ts");
        assert_eq!(
            result.to_string_lossy().replace('\\', "/"),
            "/project/src/Button.ts"
        );
    }

    #[test]
    fn with_exact_extension_replaces_existing_extension() {
        let path = Path::new("/project/src/Button.js");
        let result = with_exact_extension(path, ".ts");
        assert_eq!(
            result.to_string_lossy().replace('\\', "/"),
            "/project/src/Button.ts"
        );
    }

    // ---- resolve_tsconfig_reference_path (lines 293-313) ----

    #[cfg_attr(miri, ignore = "tempdir is blocked by Miri isolation")]
    #[test]
    fn resolve_tsconfig_reference_path_directory_appends_tsconfig_json() {
        let temp = tempdir().unwrap();
        let base = temp.path();
        fs::create_dir_all(base.join("packages/ui")).unwrap();
        let result = resolve_tsconfig_reference_path(base, "packages/ui");
        assert_eq!(
            result.to_string_lossy().replace('\\', "/"),
            base.join("packages/ui/tsconfig.json")
                .to_string_lossy()
                .replace('\\', "/")
        );
    }

    #[test]
    fn resolve_tsconfig_reference_path_explicit_json_preserved() {
        let base = Path::new("/project");
        let result = resolve_tsconfig_reference_path(base, "tsconfig.lib.json");
        assert_eq!(
            result.to_string_lossy().replace('\\', "/"),
            "/project/tsconfig.lib.json"
        );
    }

    #[cfg_attr(miri, ignore = "tempdir is blocked by Miri isolation")]
    #[test]
    fn resolve_tsconfig_reference_path_extensionless_existing_file_appends_json() {
        let temp = tempdir().unwrap();
        let base = temp.path();
        fs::write(base.join("tsconfig.lib.json"), "{}").unwrap();
        let result = resolve_tsconfig_reference_path(base, "tsconfig.lib");
        assert_eq!(
            result.to_string_lossy().replace('\\', "/"),
            base.join("tsconfig.lib.json")
                .to_string_lossy()
                .replace('\\', "/")
        );
    }

    #[cfg_attr(miri, ignore = "filesystem metadata is blocked by Miri isolation")]
    #[test]
    fn resolve_tsconfig_reference_path_extensionless_nonexistent_falls_back_to_tsconfig_json() {
        let base = Path::new("/project");
        let result = resolve_tsconfig_reference_path(base, "tsconfig.lib");
        assert_eq!(
            result.to_string_lossy().replace('\\', "/"),
            "/project/tsconfig.lib/tsconfig.json"
        );
    }

    // ---- tsconfig_extends_values (lines 315-320) ----

    #[test]
    fn tsconfig_extends_values_single_string() {
        let json = serde_json::json!({ "extends": "../../tsconfig.base.json" });
        let values = tsconfig_extends_values(&json);
        assert_eq!(values, vec!["../../tsconfig.base.json"]);
    }

    #[test]
    fn tsconfig_extends_values_array_form() {
        let json = serde_json::json!({
            "extends": ["../../tsconfig.base.json", "@tsconfig/node18/tsconfig.json"]
        });
        let values = tsconfig_extends_values(&json);
        assert_eq!(
            values,
            vec!["../../tsconfig.base.json", "@tsconfig/node18/tsconfig.json"]
        );
    }

    #[test]
    fn tsconfig_extends_values_missing_extends_returns_empty() {
        let json = serde_json::json!({ "compilerOptions": {} });
        let values = tsconfig_extends_values(&json);
        assert!(values.is_empty());
    }

    // ---- is_relative_tsconfig_extends (lines 539-541) ----

    #[test]
    fn is_relative_tsconfig_extends_dot_slash_is_relative() {
        assert!(is_relative_tsconfig_extends("./tsconfig.base.json"));
    }

    #[test]
    fn is_relative_tsconfig_extends_double_dot_slash_is_relative() {
        assert!(is_relative_tsconfig_extends("../../tsconfig.base.json"));
    }

    #[test]
    fn is_relative_tsconfig_extends_package_name_is_not_relative() {
        assert!(!is_relative_tsconfig_extends(
            "@tsconfig/node18/tsconfig.json"
        ));
    }

    // ---- resolve_tsconfig_extends_candidate (lines 554-568) ----

    #[test]
    fn resolve_tsconfig_extends_candidate_with_json_ext_unchanged() {
        let path = PathBuf::from("/project/tsconfig.base.json");
        let result = resolve_tsconfig_extends_candidate(path.clone());
        assert_eq!(
            result.to_string_lossy().replace('\\', "/"),
            path.to_string_lossy().replace('\\', "/")
        );
    }

    #[cfg_attr(miri, ignore = "tempdir is blocked by Miri isolation")]
    #[test]
    fn resolve_tsconfig_extends_candidate_extensionless_with_existing_json_file_appends_json() {
        let temp = tempdir().unwrap();
        let base = temp.path();
        fs::write(base.join("tsconfig.base.json"), "{}").unwrap();
        let path = base.join("tsconfig.base");
        let result = resolve_tsconfig_extends_candidate(path);
        assert_eq!(
            result.to_string_lossy().replace('\\', "/"),
            base.join("tsconfig.base.json")
                .to_string_lossy()
                .replace('\\', "/")
        );
    }

    #[cfg_attr(miri, ignore = "filesystem metadata is blocked by Miri isolation")]
    #[test]
    fn resolve_tsconfig_extends_candidate_extensionless_nonexistent_returns_original_path() {
        let path = PathBuf::from("/project/tsconfig.base");
        let result = resolve_tsconfig_extends_candidate(path.clone());
        assert_eq!(
            result.to_string_lossy().replace('\\', "/"),
            path.to_string_lossy().replace('\\', "/")
        );
    }

    // ---- is_storybook_preview_html (lines 393-401) ----

    #[test]
    fn is_storybook_preview_html_matches_preview_head_html() {
        let path = Path::new("/project/.storybook/preview-head.html");
        assert!(is_storybook_preview_html(path));
    }

    #[test]
    fn is_storybook_preview_html_matches_preview_body_html() {
        let path = Path::new("/project/.storybook/preview-body.html");
        assert!(is_storybook_preview_html(path));
    }

    #[test]
    fn is_storybook_preview_html_rejects_non_storybook_dir() {
        let path = Path::new("/project/src/preview-head.html");
        assert!(!is_storybook_preview_html(path));
    }

    #[test]
    fn is_storybook_preview_html_rejects_other_filenames() {
        let path = Path::new("/project/.storybook/manager-head.html");
        assert!(!is_storybook_preview_html(path));
    }

    // ---- tsconfig_extends_values array with non-string entries (line 318) ----

    #[test]
    fn tsconfig_extends_values_array_filters_out_non_string_entries() {
        let json = serde_json::json!({ "extends": ["valid.json", 42, null] });
        let values = tsconfig_extends_values(&json);
        assert_eq!(values, vec!["valid.json"]);
    }

    // ---- path_alias_capture: exact match returns empty capture (line 615) ----

    #[test]
    fn path_alias_capture_exact_pattern_returns_empty_capture() {
        assert_eq!(path_alias_capture("$lib", "$lib"), Some(""));
        assert_eq!(path_alias_capture("$lib", "$lib/utils"), None);
    }

    // ---- path_alias_pattern_matches: bare wildcard (lines 596-599) ----

    #[test]
    fn wildcard_only_pattern_does_not_match_via_pattern_matches() {
        assert!(!path_alias_pattern_matches("*", "anything"));
    }

    // ---- package_usage_name_for_resolved_package with path alias specifier ----

    #[test]
    fn package_usage_name_path_alias_specifier_uses_resolved_package() {
        let specifier = "@/utils";
        let resolved = Path::new("/project/node_modules/some-pkg/utils/index.js");
        let name = package_usage_name_for_resolved_package(specifier, resolved);
        assert_eq!(name, Some("some-pkg".to_string()));
    }

    // ---- static_dir_relative_path: mount without trailing slash subdir (line 447) ----

    #[test]
    fn static_dir_relative_path_prefix_collision_does_not_match() {
        assert_eq!(
            static_dir_relative_path("/assetsExtra/file.js", "/assets"),
            None
        );
    }

    // ---- glob_values_match: invalid glob is skipped (line 362) ----

    #[test]
    fn glob_values_match_invalid_glob_pattern_is_skipped() {
        let base = Path::new("/project");
        let source = Path::new("/project/src/index.ts");
        let result = glob_values_match(base, &[serde_json::json!("[invalid")], source);
        assert!(
            !result,
            "an invalid glob pattern must not accidentally match"
        );
    }

    // ---- glob_values_match: no patterns at all returns false ----

    #[test]
    fn glob_values_match_empty_values_returns_false() {
        let base = Path::new("/project");
        let source = Path::new("/project/src/index.ts");
        assert!(!glob_values_match(base, &[], source));
    }

    // ---- should_preserve: from_style flag enables preservation for bare package from scss importer ----

    #[test]
    fn should_preserve_from_style_flag_enables_preservation() {
        let specifier = "normalize.css";
        let from_file = Path::new("/project/src/App.vue");
        let resolved = Path::new("/project/node_modules/normalize.css/normalize.css");
        assert!(should_preserve_node_modules_style_file(
            specifier, from_file, resolved, true
        ));
    }

    // ---- normalize_resolve_specifier: http:// URL is external ----

    #[test]
    fn http_url_is_external() {
        let result = normalize_resolve_specifier("http://cdn.example.com/lib.js");
        assert!(
            matches!(result, SpecifierNormalization::External(_)),
            "http:// URL must be External"
        );
    }

    // ---- normalize_resolve_specifier: npm: with subpath (lines 1291-1301) ----

    #[test]
    fn npm_scheme_plain_package_is_resolvable() {
        let result = normalize_resolve_specifier("npm:react");
        let SpecifierNormalization::Resolvable(s) = result else {
            panic!("npm:react should be Resolvable");
        };
        assert_eq!(s, "react");
    }

    // ---- resolve_tsconfig_extends_path: relative path resolution (lines 519-536) ----

    #[test]
    fn resolve_tsconfig_extends_path_relative_dot_slash_preserves_dot_slash() {
        let base = Path::new("/project");
        let result = resolve_tsconfig_extends_path(base, "./tsconfig.base.json");
        assert!(
            result
                .to_string_lossy()
                .replace('\\', "/")
                .contains("tsconfig.base.json"),
            "relative dot-slash path must resolve to the base.json file"
        );
    }

    #[test]
    fn resolve_tsconfig_extends_path_adds_json_extension_for_extensionless_relative() {
        let base = Path::new("/project");
        let result = resolve_tsconfig_extends_path(base, "../shared/tsconfig.base");
        assert!(
            result.to_string_lossy().ends_with(".json"),
            "extensionless extends must get .json appended"
        );
    }

    // ---- is_bare_style_subpath edge cases: scss extension ----

    #[test]
    fn is_bare_style_subpath_scss_extension_matches() {
        assert!(is_bare_style_subpath("bootstrap/scss/bootstrap.scss"));
    }

    #[test]
    fn is_bare_style_subpath_sass_extension_matches() {
        assert!(is_bare_style_subpath("bulma/sass/helpers/index.sass"));
    }

    #[test]
    fn is_bare_style_subpath_less_extension_matches() {
        assert!(is_bare_style_subpath("antd/lib/style/index.less"));
    }

    // ---- has_glob_meta: closing brace or bracket alone ----

    #[test]
    fn has_glob_meta_closing_bracket_detected() {
        assert!(has_glob_meta("src/[a-z]"));
    }

    // ---- path_alias_specificity: wildcard-only pattern ----

    #[test]
    fn path_alias_specificity_pure_wildcard_has_zero_specificity() {
        assert_eq!(path_alias_specificity("*"), 0);
    }

    // ---- with_exact_extension: path without extension ----

    #[test]
    fn with_exact_extension_path_without_extension_does_not_add_double_ext() {
        let path = Path::new("/project/src/Button");
        let result = with_exact_extension(path, ".ts");
        assert_eq!(
            result.to_string_lossy().replace('\\', "/"),
            "/project/src/Button.ts"
        );
    }
}
