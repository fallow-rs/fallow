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
        if should_preserve_node_modules_style_file(specifier, from_file, &canonical, from_style) {
            return Some(ResolveResult::ExternalFile(canonical));
        }
        if let Some(pkg_name) = package_usage_name_for_resolved_package(specifier, &canonical)
            && !ctx.workspace_roots.contains_key(pkg_name.as_str())
        {
            return Some(ResolveResult::NpmPackage(pkg_name));
        }
        if let Some(pkg_name) = package_usage_name_for_external_bare_specifier(specifier) {
            return Some(ResolveResult::NpmPackage(pkg_name));
        }
        return Some(ResolveResult::ExternalFile(canonical));
    }

    package_usage_name_for_resolved_package(specifier, resolved_path)
        .map(ResolveResult::NpmPackage)
        .or_else(|| Some(ResolveResult::ExternalFile(resolved_path.to_path_buf())))
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
    if specifier.starts_with("jsr:") {
        return ResolveResult::ExternalFile(PathBuf::from(specifier));
    }
    let npm_normalized;
    let specifier = if let Some(rest) = specifier.strip_prefix("npm:") {
        npm_normalized = normalize_npm_specifier(rest);
        if npm_normalized.is_empty() {
            return ResolveResult::ExternalFile(PathBuf::from(specifier));
        }
        npm_normalized.as_str()
    } else {
        specifier
    };

    if specifier.contains("://") || specifier.starts_with("data:") {
        return ResolveResult::ExternalFile(PathBuf::from(specifier));
    }

    if let Some(result) = try_storybook_static_dir_mapping(ctx, from_file, specifier) {
        return result;
    }

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
        if let Some(result) = try_html_public_root_relative_asset(ctx, from_file, specifier) {
            return result;
        }
        return ResolveResult::Unresolvable(specifier.to_string());
    }

    if from_style && let Some(result) = try_scss_fallbacks(ctx, from_file, specifier, true) {
        return result;
    }

    let is_bare = is_bare_specifier(specifier);
    let is_alias = is_path_alias(specifier);
    let matches_plugin_alias = ctx
        .path_aliases
        .iter()
        .any(|(prefix, _)| specifier_matches_alias_prefix(specifier, prefix));

    if let Some(result) =
        try_style_condition_package_resolution(ctx, from_file, specifier, from_style)
    {
        return result;
    }

    match resolve_file_with_tsconfig_fallback(ctx, from_file, specifier) {
        ResolveFileAttempt::Resolved {
            resolution: resolved,
            used_tsconfig_fallback,
        } => {
            let resolved_path = resolved.path();
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
            ResolvedPathContext {
                ctx,
                from_file,
                specifier,
                from_style,
            }
            .resolve(resolved_path)
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

            if (used_tsconfig_fallback || is_bare || is_alias || matches_plugin_alias)
                && let Some(result) = try_nearest_tsconfig_path_alias(ctx, from_file, specifier)
            {
                return result;
            }

            let matches_tsconfig_path_alias =
                matches_nearest_tsconfig_path_alias(ctx.root, from_file, specifier);

            if let Some(result) = try_package_imports_fallback(ctx, from_file, specifier) {
                return result;
            }

            if let Some(result) =
                try_relative_package_root_source_fallback(ctx, from_file, specifier)
            {
                return result;
            }

            if is_alias || matches_plugin_alias {
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
            } else if let Some(result) =
                try_css_relative_subpath_fallback(ctx, from_file, specifier, from_style)
            {
                result
            } else if is_plain_css_file(from_file) && is_bare_style_subpath(specifier) {
                ResolveResult::Unresolvable(specifier.to_string())
            } else if is_bare && is_valid_package_name(specifier) {
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
        glob_values_match, is_safe_static_dir_relative_path, is_tsconfig_error,
        matches_nearest_tsconfig_path_alias, path_alias_capture, path_alias_pattern_matches,
        resolve_tsconfig_extends_path,
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
}
