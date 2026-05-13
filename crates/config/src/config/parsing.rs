use std::path::{Path, PathBuf};
use std::time::Duration;

use rustc_hash::FxHashSet;

use super::FallowConfig;

/// Supported config file names in priority order.
///
/// `find_and_load` checks these names in order within each directory,
/// returning the first match found. `.fallowrc.json` wins over
/// `.fallowrc.jsonc` if both exist (mirrors `tsconfig.json` >
/// `tsconfig.jsonc` precedence).
pub(super) const CONFIG_NAMES: &[&str] = &[
    ".fallowrc.json",
    ".fallowrc.jsonc",
    "fallow.toml",
    ".fallow.toml",
];

pub(super) const MAX_EXTENDS_DEPTH: usize = 10;

/// Prefix for npm package specifiers in the `extends` field.
const NPM_PREFIX: &str = "npm:";

/// Prefix for HTTPS URL specifiers in the `extends` field.
const HTTPS_PREFIX: &str = "https://";

/// Prefix for HTTP URL specifiers (rejected with a clear error).
const HTTP_PREFIX: &str = "http://";

/// Default timeout for fetching remote configs via URL extends.
const DEFAULT_URL_TIMEOUT_SECS: u64 = 5;

/// Detect config format from file extension.
pub(super) enum ConfigFormat {
    Toml,
    Json,
}

impl ConfigFormat {
    pub(super) fn from_path(path: &Path) -> Self {
        match path.extension().and_then(|e| e.to_str()) {
            Some("json" | "jsonc") => Self::Json,
            _ => Self::Toml,
        }
    }
}

/// Deep-merge two JSON values. `base` is lower-priority, `overlay` is higher.
/// Objects: merge field by field. Arrays/scalars: overlay replaces base.
pub(super) fn deep_merge_json(base: &mut serde_json::Value, overlay: serde_json::Value) {
    match (base, overlay) {
        (serde_json::Value::Object(base_map), serde_json::Value::Object(overlay_map)) => {
            for (key, value) in overlay_map {
                if let Some(base_value) = base_map.get_mut(&key) {
                    deep_merge_json(base_value, value);
                } else {
                    base_map.insert(key, value);
                }
            }
        }
        (base, overlay) => {
            *base = overlay;
        }
    }
}

pub(super) fn parse_config_to_value(path: &Path) -> Result<serde_json::Value, miette::Report> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| miette::miette!("Failed to read config file {}: {}", path.display(), e))?;
    // Strip a leading UTF-8 BOM so Windows-authored configs parse cleanly.
    // jsonc-parser and serde_yaml_ng both reject `\u{FEFF}` as an unexpected
    // token; matches the pre-existing behaviour in workspace/parsers.rs.
    let content = content.trim_start_matches('\u{FEFF}');

    match ConfigFormat::from_path(path) {
        ConfigFormat::Toml => {
            let toml_value: toml::Value = toml::from_str(content).map_err(|e| {
                miette::miette!("Failed to parse config file {}: {}", path.display(), e)
            })?;
            serde_json::to_value(toml_value).map_err(|e| {
                miette::miette!(
                    "Failed to convert TOML to JSON for {}: {}",
                    path.display(),
                    e
                )
            })
        }
        ConfigFormat::Json => crate::jsonc::parse_to_value(content)
            .map_err(|e| miette::miette!("Failed to parse config file {}: {}", path.display(), e)),
    }
}

/// Return `true` if `dir` contains a VCS marker indicating a repository root.
///
/// Used as the walk-up stop condition for config discovery. Matches `.git`
/// (directory for normal repos, file for git submodules/worktrees), `.hg`
/// (Mercurial), and `.svn` (Subversion). We intentionally do NOT treat
/// `package.json` as a stop boundary so monorepo sub-packages can inherit a
/// root config. This matches Prettier/ESLint/Biome behavior.
fn is_repo_root(dir: &Path) -> bool {
    dir.join(".git").exists() || dir.join(".hg").exists() || dir.join(".svn").exists()
}

/// Verify that `resolved` stays within `base_dir` after canonicalization.
///
/// Prevents path traversal attacks where a subpath or `package.json` field
/// like `../../etc/passwd` escapes the intended directory.
fn resolve_confined(
    base_dir: &Path,
    resolved: &Path,
    context: &str,
    source_config: &Path,
) -> Result<PathBuf, miette::Report> {
    let canonical_base = dunce::canonicalize(base_dir)
        .map_err(|e| miette::miette!("Failed to resolve base dir {}: {}", base_dir.display(), e))?;
    let canonical_file = dunce::canonicalize(resolved).map_err(|e| {
        miette::miette!(
            "Config file not found: {} ({}, referenced from {}): {}",
            resolved.display(),
            context,
            source_config.display(),
            e
        )
    })?;
    if !canonical_file.starts_with(&canonical_base) {
        return Err(miette::miette!(
            "Path traversal detected: {} escapes package directory {} ({}, referenced from {})",
            resolved.display(),
            base_dir.display(),
            context,
            source_config.display()
        ));
    }
    Ok(canonical_file)
}

/// Validate that a parsed package name is a legal npm package name.
fn validate_npm_package_name(name: &str, source_config: &Path) -> Result<(), miette::Report> {
    if name.starts_with('@') && !name.contains('/') {
        return Err(miette::miette!(
            "Invalid scoped npm package name '{}': must be '@scope/name' (referenced from {})",
            name,
            source_config.display()
        ));
    }
    if name.split('/').any(|c| c == ".." || c == ".") {
        return Err(miette::miette!(
            "Invalid npm package name '{}': path traversal components not allowed (referenced from {})",
            name,
            source_config.display()
        ));
    }
    Ok(())
}

/// Parse an npm specifier into `(package_name, optional_subpath)`.
///
/// Scoped: `@scope/name` → `("@scope/name", None)`,
///         `@scope/name/strict.json` → `("@scope/name", Some("strict.json"))`.
/// Unscoped: `name` → `("name", None)`,
///           `name/strict.json` → `("name", Some("strict.json"))`.
fn parse_npm_specifier(specifier: &str) -> (&str, Option<&str>) {
    if specifier.starts_with('@') {
        // Scoped: @scope/name[/subpath]
        // Find the second '/' which separates name from subpath.
        let mut slashes = 0;
        for (i, ch) in specifier.char_indices() {
            if ch == '/' {
                slashes += 1;
                if slashes == 2 {
                    return (&specifier[..i], Some(&specifier[i + 1..]));
                }
            }
        }
        // No subpath — entire string is the package name.
        (specifier, None)
    } else if let Some(slash) = specifier.find('/') {
        (&specifier[..slash], Some(&specifier[slash + 1..]))
    } else {
        (specifier, None)
    }
}

/// Resolve the default export path from a `package.json` `exports` field.
///
/// Handles the common patterns:
/// - `"exports": "./config.json"` (string shorthand)
/// - `"exports": {".": "./config.json"}` (object with default entry point)
/// - `"exports": {".": {"default": "./config.json"}}` (conditional exports)
fn resolve_package_exports(pkg: &serde_json::Value, package_dir: &Path) -> Option<PathBuf> {
    let exports = pkg.get("exports")?;
    match exports {
        serde_json::Value::String(s) => Some(package_dir.join(s.as_str())),
        serde_json::Value::Object(map) => {
            let dot_export = map.get(".")?;
            match dot_export {
                serde_json::Value::String(s) => Some(package_dir.join(s.as_str())),
                serde_json::Value::Object(conditions) => {
                    for key in ["default", "node", "import", "require"] {
                        if let Some(serde_json::Value::String(s)) = conditions.get(key) {
                            return Some(package_dir.join(s.as_str()));
                        }
                    }
                    None
                }
                _ => None,
            }
        }
        // Array export fallback form (e.g., `[\"./config.json\", null]`) is not supported;
        // falls through to main/config name scan.
        _ => None,
    }
}

/// Find a fallow config file inside an npm package directory.
///
/// Resolution order:
/// 1. `package.json` `exports` field (default entry point)
/// 2. `package.json` `main` field
/// 3. Standard config file names (`.fallowrc.json`, `.fallowrc.jsonc`, `fallow.toml`, `.fallow.toml`)
///
/// Paths from `exports`/`main` are confined to the package directory to prevent
/// path traversal attacks from malicious packages.
fn find_config_in_npm_package(
    package_dir: &Path,
    source_config: &Path,
) -> Result<PathBuf, miette::Report> {
    let pkg_json_path = package_dir.join("package.json");
    if pkg_json_path.exists() {
        let content = std::fs::read_to_string(&pkg_json_path)
            .map_err(|e| miette::miette!("Failed to read {}: {}", pkg_json_path.display(), e))?;
        let pkg: serde_json::Value = serde_json::from_str(&content)
            .map_err(|e| miette::miette!("Failed to parse {}: {}", pkg_json_path.display(), e))?;
        if let Some(config_path) = resolve_package_exports(&pkg, package_dir)
            && config_path.exists()
        {
            return resolve_confined(
                package_dir,
                &config_path,
                "package.json exports",
                source_config,
            );
        }
        if let Some(main) = pkg.get("main").and_then(|v| v.as_str()) {
            let main_path = package_dir.join(main);
            if main_path.exists() {
                return resolve_confined(
                    package_dir,
                    &main_path,
                    "package.json main",
                    source_config,
                );
            }
        }
    }

    for config_name in CONFIG_NAMES {
        let config_path = package_dir.join(config_name);
        if config_path.exists() {
            return resolve_confined(
                package_dir,
                &config_path,
                "config name fallback",
                source_config,
            );
        }
    }

    Err(miette::miette!(
        "No fallow config found in npm package at {}. \
         Expected package.json with main/exports pointing to a config file, \
         or one of: {}",
        package_dir.display(),
        CONFIG_NAMES.join(", ")
    ))
}

/// Resolve an npm package specifier to a config file path.
///
/// Walks up from `config_dir` looking for `node_modules/<package_name>`.
/// If a subpath is given (e.g., `@scope/name/strict.json`), resolves that file directly.
/// Otherwise, finds the config file inside the package via [`find_config_in_npm_package`].
fn resolve_npm_package(
    config_dir: &Path,
    specifier: &str,
    source_config: &Path,
) -> Result<PathBuf, miette::Report> {
    let specifier = specifier.trim();
    if specifier.is_empty() {
        return Err(miette::miette!(
            "Empty npm specifier in extends (in {})",
            source_config.display()
        ));
    }

    let (package_name, subpath) = parse_npm_specifier(specifier);
    validate_npm_package_name(package_name, source_config)?;

    let mut dir = Some(config_dir);
    while let Some(d) = dir {
        let candidate = d.join("node_modules").join(package_name);
        if candidate.is_dir() {
            return if let Some(sub) = subpath {
                let file = candidate.join(sub);
                if file.exists() {
                    resolve_confined(
                        &candidate,
                        &file,
                        &format!("subpath '{sub}'"),
                        source_config,
                    )
                } else {
                    Err(miette::miette!(
                        "File not found in npm package: {} (looked for '{}' in {}, referenced from {})",
                        file.display(),
                        sub,
                        candidate.display(),
                        source_config.display()
                    ))
                }
            } else {
                find_config_in_npm_package(&candidate, source_config)
            };
        }
        dir = d.parent();
    }

    Err(miette::miette!(
        "npm package '{}' not found. \
         Searched for node_modules/{} in ancestor directories of {} (referenced from {}). \
         If this package should be available, install it and ensure it is listed in your project's dependencies",
        package_name,
        package_name,
        config_dir.display(),
        source_config.display()
    ))
}

/// Normalize a URL for deduplication.
///
/// - Lowercase scheme and host (path casing is preserved — it's server-dependent).
/// - Strip fragment (`#...`) and query string (`?...`).
/// - Strip trailing slash from path.
/// - Normalize default HTTPS port (`:443` → omitted).
fn normalize_url_for_dedup(url: &str) -> String {
    // Split at the first `://` to get scheme, then find host boundary.
    let Some((scheme, rest)) = url.split_once("://") else {
        return url.to_string();
    };
    let scheme = scheme.to_ascii_lowercase();

    // Split host from path at the first `/` after the authority.
    let (authority, path) = rest.split_once('/').map_or((rest, ""), |(a, p)| (a, p));
    let authority = authority.to_ascii_lowercase();

    // Strip default HTTPS port.
    let authority = authority.strip_suffix(":443").unwrap_or(&authority);

    // Strip fragment and query string from path, then trailing slash.
    let path = path.split_once('#').map_or(path, |(p, _)| p);
    let path = path.split_once('?').map_or(path, |(p, _)| p);
    let path = path.strip_suffix('/').unwrap_or(path);

    if path.is_empty() {
        format!("{scheme}://{authority}")
    } else {
        format!("{scheme}://{authority}/{path}")
    }
}

/// Read the `FALLOW_EXTENDS_TIMEOUT_SECS` env var, falling back to [`DEFAULT_URL_TIMEOUT_SECS`].
///
/// A value of `0` is treated as invalid and falls back to the default (a zero-duration
/// timeout would make every request fail immediately with an opaque timeout error).
fn url_timeout() -> Duration {
    std::env::var("FALLOW_EXTENDS_TIMEOUT_SECS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok().filter(|&n| n > 0))
        .map_or(
            Duration::from_secs(DEFAULT_URL_TIMEOUT_SECS),
            Duration::from_secs,
        )
}

/// Maximum response body size for fetched config files (1 MB).
/// Config files are never legitimately larger than a few kilobytes.
const MAX_URL_CONFIG_BYTES: u64 = 1024 * 1024;

/// Fetch a remote JSON config from an HTTPS URL.
///
/// Returns the parsed `serde_json::Value`. Only JSON (with optional JSONC comments) is
/// supported for URL-sourced configs — TOML cannot be detected without a file extension.
fn fetch_url_config(url: &str, source: &str) -> Result<serde_json::Value, miette::Report> {
    let timeout = url_timeout();
    let agent = ureq::Agent::config_builder()
        .timeout_global(Some(timeout))
        .https_only(true)
        .build()
        .new_agent();

    let mut response = agent.get(url).call().map_err(|e| {
        miette::miette!(
            "Failed to fetch remote config from {url} (referenced from {source}): {e}. \
             If this URL is unavailable, use a local path or npm: specifier instead"
        )
    })?;

    let body = response
        .body_mut()
        .with_config()
        .limit(MAX_URL_CONFIG_BYTES)
        .read_to_string()
        .map_err(|e| {
            miette::miette!(
                "Failed to read response body from {url} (referenced from {source}): {e}"
            )
        })?;

    crate::jsonc::parse_to_value(&body).map_err(|e| {
        miette::miette!(
            "Failed to parse remote config as JSON from {url} (referenced from {source}): {e}. \
             Only JSON/JSONC is supported for URL-sourced configs"
        )
    })
}

/// Extract the `extends` array from a parsed JSON config value, removing it from the object.
fn extract_extends(value: &mut serde_json::Value) -> Vec<String> {
    value
        .as_object_mut()
        .and_then(|obj| obj.remove("extends"))
        .and_then(|v| match v {
            serde_json::Value::Array(arr) => Some(
                arr.into_iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect::<Vec<_>>(),
            ),
            serde_json::Value::String(s) => Some(vec![s]),
            _ => None,
        })
        .unwrap_or_default()
}

/// Resolve extends entries from a URL-sourced config.
///
/// URL-sourced configs may extend other URLs or `npm:` packages, but NOT relative
/// paths (there is no filesystem base directory for a URL).
fn resolve_url_extends(
    url: &str,
    visited: &mut FxHashSet<String>,
    depth: usize,
) -> Result<serde_json::Value, miette::Report> {
    if depth >= MAX_EXTENDS_DEPTH {
        return Err(miette::miette!(
            "Config extends chain too deep (>={MAX_EXTENDS_DEPTH} levels) at {url}"
        ));
    }

    let normalized = normalize_url_for_dedup(url);
    if !visited.insert(normalized) {
        return Err(miette::miette!(
            "Circular extends detected: {url} was already visited in the extends chain"
        ));
    }

    let mut value = fetch_url_config(url, url)?;
    let extends = extract_extends(&mut value);

    if extends.is_empty() {
        return Ok(value);
    }

    let mut merged = serde_json::Value::Object(serde_json::Map::new());

    for entry in &extends {
        let base = if entry.starts_with(HTTPS_PREFIX) {
            resolve_url_extends(entry, visited, depth + 1)?
        } else if entry.starts_with(HTTP_PREFIX) {
            return Err(miette::miette!(
                "URL extends must use https://, got http:// URL '{}' (in remote config {}). \
                 Change the URL to use https:// instead",
                entry,
                url
            ));
        } else if let Some(npm_specifier) = entry.strip_prefix(NPM_PREFIX) {
            // npm: from URL context — no config_dir to walk up from, so we use the cwd.
            // This is a best-effort fallback; the npm package must be available in the
            // working directory's node_modules tree.
            let cwd = std::env::current_dir().map_err(|e| {
                miette::miette!(
                    "Cannot resolve npm: specifier from URL-sourced config: \
                     failed to determine current directory: {e}"
                )
            })?;
            tracing::warn!(
                "Resolving npm:{npm_specifier} from URL-sourced config ({url}) using the \
                 current working directory for node_modules lookup"
            );
            let path_placeholder = PathBuf::from(url);
            let npm_path = resolve_npm_package(&cwd, npm_specifier, &path_placeholder)?;
            resolve_extends_file(&npm_path, visited, depth + 1)?
        } else {
            return Err(miette::miette!(
                "Relative paths in 'extends' are not supported when the base config was \
                 fetched from a URL ('{url}'). Use another https:// URL or npm: reference \
                 instead. Got: '{entry}'"
            ));
        };
        deep_merge_json(&mut merged, base);
    }

    deep_merge_json(&mut merged, value);
    Ok(merged)
}

/// Resolve extends from a local config file.
///
/// This is the main recursive resolver for file-based configs. It reads the file,
/// extracts `extends`, and recursively resolves each entry (relative paths, npm
/// packages, or HTTPS URLs).
fn resolve_extends_file(
    path: &Path,
    visited: &mut FxHashSet<String>,
    depth: usize,
) -> Result<serde_json::Value, miette::Report> {
    if depth >= MAX_EXTENDS_DEPTH {
        return Err(miette::miette!(
            "Config extends chain too deep (>={MAX_EXTENDS_DEPTH} levels) at {}",
            path.display()
        ));
    }

    let canonical = dunce::canonicalize(path).map_err(|e| {
        miette::miette!(
            "Config file not found or unresolvable: {}: {}",
            path.display(),
            e
        )
    })?;

    if !visited.insert(canonical.to_string_lossy().into_owned()) {
        return Err(miette::miette!(
            "Circular extends detected: {} was already visited in the extends chain",
            path.display()
        ));
    }

    let mut value = parse_config_to_value(path)?;
    let extends = extract_extends(&mut value);

    if extends.is_empty() {
        return Ok(value);
    }

    let config_dir = path.parent().unwrap_or_else(|| Path::new("."));
    let sealed = value
        .get("sealed")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);
    // Canonicalize the config directory once when sealed; reused inside the
    // loop for each `extends` confinement check.
    let sealed_dir_canonical = if sealed {
        Some(dunce::canonicalize(config_dir).map_err(|e| {
            miette::miette!(
                "Sealed config directory '{}' could not be canonicalized: {e}",
                config_dir.display()
            )
        })?)
    } else {
        None
    };
    let mut merged = serde_json::Value::Object(serde_json::Map::new());

    for extend_path_str in &extends {
        let base = if extend_path_str.starts_with(HTTPS_PREFIX) {
            if sealed {
                return Err(miette::miette!(
                    "'sealed: true' config at {} rejects URL extends '{}'. \
                     Sealed configs only allow file-relative extends within \
                     the config's directory",
                    path.display(),
                    extend_path_str
                ));
            }
            resolve_url_extends(extend_path_str, visited, depth + 1)?
        } else if extend_path_str.starts_with(HTTP_PREFIX) {
            return Err(miette::miette!(
                "URL extends must use https://, got http:// URL '{}' (in {}). \
                 Change the URL to use https:// instead",
                extend_path_str,
                path.display()
            ));
        } else if let Some(npm_specifier) = extend_path_str.strip_prefix(NPM_PREFIX) {
            if sealed {
                return Err(miette::miette!(
                    "'sealed: true' config at {} rejects npm extends '{}'. \
                     Sealed configs only allow file-relative extends within \
                     the config's directory",
                    path.display(),
                    extend_path_str
                ));
            }
            let npm_path = resolve_npm_package(config_dir, npm_specifier, path)?;
            resolve_extends_file(&npm_path, visited, depth + 1)?
        } else {
            if Path::new(extend_path_str).is_absolute() {
                return Err(miette::miette!(
                    "extends paths must be relative, got absolute path: {} (in {})",
                    extend_path_str,
                    path.display()
                ));
            }
            let p = config_dir.join(extend_path_str);
            if !p.exists() {
                return Err(miette::miette!(
                    "Extended config file not found: {} (referenced from {})",
                    p.display(),
                    path.display()
                ));
            }
            if let Some(dir_canonical) = &sealed_dir_canonical {
                let p_canonical = dunce::canonicalize(&p).map_err(|e| {
                    miette::miette!(
                        "Sealed config extends path '{}' could not be canonicalized: {e}",
                        p.display()
                    )
                })?;
                if !p_canonical.starts_with(dir_canonical) {
                    return Err(miette::miette!(
                        "'sealed: true' config at {} rejects extends '{}' which resolves \
                         outside the config's directory ({}). Sealed configs only allow \
                         extends within the config's directory",
                        path.display(),
                        extend_path_str,
                        p_canonical.display()
                    ));
                }
            }
            resolve_extends_file(&p, visited, depth + 1)?
        };
        deep_merge_json(&mut merged, base);
    }

    deep_merge_json(&mut merged, value);
    Ok(merged)
}

/// Public entry point: resolve a config file with all its extends chain.
///
/// Delegates to [`resolve_extends_file`] with a fresh visited set.
pub(super) fn resolve_extends(
    path: &Path,
    visited: &mut FxHashSet<String>,
    depth: usize,
) -> Result<serde_json::Value, miette::Report> {
    resolve_extends_file(path, visited, depth)
}

impl FallowConfig {
    /// Load config from a fallow config file (TOML or JSON/JSONC).
    ///
    /// The format is detected from the file extension:
    /// - `.toml` → TOML
    /// - `.json` → JSON (with JSONC comment stripping)
    ///
    /// Supports `extends` for config inheritance. Extended configs are loaded
    /// and deep-merged before this config's values are applied.
    ///
    /// # Errors
    ///
    /// Returns an error when the config file cannot be read, merged, or deserialized.
    pub fn load(path: &Path) -> Result<Self, miette::Report> {
        let mut visited = FxHashSet::default();
        let merged = resolve_extends(path, &mut visited, 0)?;

        serde_json::from_value(merged).map_err(|e| {
            miette::miette!(
                "Failed to deserialize config from {}: {}",
                path.display(),
                e
            )
        })
    }

    /// Find the config file path without loading it.
    /// Searches the same locations as `find_and_load`.
    #[must_use]
    pub fn find_config_path(start: &Path) -> Option<PathBuf> {
        let mut dir = start;
        loop {
            for name in CONFIG_NAMES {
                let candidate = dir.join(name);
                if candidate.exists() {
                    return Some(candidate);
                }
            }
            if is_repo_root(dir) {
                break;
            }
            dir = dir.parent()?;
        }
        None
    }

    /// Find and load config, searching from `start` up to the project root.
    ///
    /// # Errors
    ///
    /// Returns an error if a config file is found but cannot be read or parsed.
    pub fn find_and_load(start: &Path) -> Result<Option<(Self, PathBuf)>, String> {
        let mut dir = start;
        loop {
            for name in CONFIG_NAMES {
                let candidate = dir.join(name);
                if candidate.exists() {
                    match Self::load(&candidate) {
                        Ok(config) => return Ok(Some((config, candidate))),
                        Err(e) => {
                            return Err(format!("Failed to parse {}: {e}", candidate.display()));
                        }
                    }
                }
            }
            // Stop at project root indicators (VCS markers). We intentionally
            // do NOT stop at `package.json` so that monorepo sub-packages
            // inherit a root config placed alongside the workspace root.
            if is_repo_root(dir) {
                break;
            }
            dir = match dir.parent() {
                Some(parent) => parent,
                None => break,
            };
        }
        Ok(None)
    }

    /// Generate JSON Schema for the configuration format.
    #[must_use]
    pub fn json_schema() -> serde_json::Value {
        serde_json::to_value(schemars::schema_for!(FallowConfig)).unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::PackageJson;
    use crate::config::format::OutputFormat;
    use crate::config::rules::Severity;

    /// Create a panic-safe temp directory (RAII cleanup via `tempfile::TempDir`).
    fn test_dir(_name: &str) -> tempfile::TempDir {
        tempfile::tempdir().expect("create temp dir")
    }

    #[test]
    fn fallow_config_deserialize_minimal() {
        let toml_str = r#"
entry = ["src/main.ts"]
"#;
        let config: FallowConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.entry, vec!["src/main.ts"]);
        assert!(config.ignore_patterns.is_empty());
    }

    #[test]
    fn fallow_config_deserialize_ignore_exports() {
        let toml_str = r#"
[[ignoreExports]]
file = "src/types/*.ts"
exports = ["*"]

[[ignoreExports]]
file = "src/constants.ts"
exports = ["FOO", "BAR"]
"#;
        let config: FallowConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.ignore_exports.len(), 2);
        assert_eq!(config.ignore_exports[0].file, "src/types/*.ts");
        assert_eq!(config.ignore_exports[0].exports, vec!["*"]);
        assert_eq!(config.ignore_exports[1].exports, vec!["FOO", "BAR"]);
    }

    #[test]
    fn fallow_config_deserialize_ignore_dependencies() {
        let toml_str = r#"
ignoreDependencies = ["autoprefixer", "postcss"]
"#;
        let config: FallowConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.ignore_dependencies, vec!["autoprefixer", "postcss"]);
    }

    #[test]
    fn fallow_config_resolve_default_ignores() {
        let config = FallowConfig::default();
        let resolved = config.resolve(
            PathBuf::from("/tmp/test"),
            OutputFormat::Human,
            4,
            true,
            true,
        );

        // Default ignores should be compiled
        assert!(resolved.ignore_patterns.is_match("node_modules/foo/bar.ts"));
        assert!(resolved.ignore_patterns.is_match("dist/bundle.js"));
        assert!(resolved.ignore_patterns.is_match("build/output.js"));
        assert!(resolved.ignore_patterns.is_match(".git/config"));
        assert!(resolved.ignore_patterns.is_match("coverage/report.js"));
        assert!(resolved.ignore_patterns.is_match("foo.min.js"));
        assert!(resolved.ignore_patterns.is_match("bar.min.mjs"));
    }

    #[test]
    fn fallow_config_resolve_custom_ignores() {
        let config = FallowConfig {
            entry: vec!["src/**/*.ts".to_string()],
            ignore_patterns: vec!["**/*.generated.ts".to_string()],
            ..Default::default()
        };
        let resolved = config.resolve(
            PathBuf::from("/tmp/test"),
            OutputFormat::Json,
            4,
            false,
            true,
        );

        assert!(resolved.ignore_patterns.is_match("src/foo.generated.ts"));
        assert_eq!(resolved.entry_patterns, vec!["src/**/*.ts"]);
        assert!(matches!(resolved.output, OutputFormat::Json));
        assert!(!resolved.no_cache);
    }

    #[test]
    fn fallow_config_resolve_cache_dir() {
        let config = FallowConfig::default();
        let resolved = config.resolve(
            PathBuf::from("/tmp/project"),
            OutputFormat::Human,
            4,
            true,
            true,
        );
        assert_eq!(resolved.cache_dir, PathBuf::from("/tmp/project/.fallow"));
        assert!(resolved.no_cache);
    }

    #[test]
    fn package_json_entry_points_main() {
        let pkg: PackageJson = serde_json::from_str(r#"{"main": "dist/index.js"}"#).unwrap();
        let entries = pkg.entry_points();
        assert!(entries.contains(&"dist/index.js".to_string()));
    }

    #[test]
    fn package_json_entry_points_module() {
        let pkg: PackageJson = serde_json::from_str(r#"{"module": "dist/index.mjs"}"#).unwrap();
        let entries = pkg.entry_points();
        assert!(entries.contains(&"dist/index.mjs".to_string()));
    }

    #[test]
    fn package_json_entry_points_types() {
        let pkg: PackageJson = serde_json::from_str(r#"{"types": "dist/index.d.ts"}"#).unwrap();
        let entries = pkg.entry_points();
        assert!(entries.contains(&"dist/index.d.ts".to_string()));
    }

    #[test]
    fn package_json_entry_points_bin_string() {
        let pkg: PackageJson = serde_json::from_str(r#"{"bin": "bin/cli.js"}"#).unwrap();
        let entries = pkg.entry_points();
        assert!(entries.contains(&"bin/cli.js".to_string()));
    }

    #[test]
    fn package_json_entry_points_bin_object() {
        let pkg: PackageJson =
            serde_json::from_str(r#"{"bin": {"cli": "bin/cli.js", "serve": "bin/serve.js"}}"#)
                .unwrap();
        let entries = pkg.entry_points();
        assert!(entries.contains(&"bin/cli.js".to_string()));
        assert!(entries.contains(&"bin/serve.js".to_string()));
    }

    #[test]
    fn package_json_entry_points_exports_string() {
        let pkg: PackageJson = serde_json::from_str(r#"{"exports": "./dist/index.js"}"#).unwrap();
        let entries = pkg.entry_points();
        assert!(entries.contains(&"./dist/index.js".to_string()));
    }

    #[test]
    fn package_json_entry_points_exports_object() {
        let pkg: PackageJson = serde_json::from_str(
            r#"{"exports": {".": {"import": "./dist/index.mjs", "require": "./dist/index.cjs"}}}"#,
        )
        .unwrap();
        let entries = pkg.entry_points();
        assert!(entries.contains(&"./dist/index.mjs".to_string()));
        assert!(entries.contains(&"./dist/index.cjs".to_string()));
    }

    #[test]
    fn package_json_dependency_names() {
        let pkg: PackageJson = serde_json::from_str(
            r#"{
            "dependencies": {"react": "^18", "lodash": "^4"},
            "devDependencies": {"typescript": "^5"},
            "peerDependencies": {"react-dom": "^18"}
        }"#,
        )
        .unwrap();

        let all = pkg.all_dependency_names();
        assert!(all.contains(&"react".to_string()));
        assert!(all.contains(&"lodash".to_string()));
        assert!(all.contains(&"typescript".to_string()));
        assert!(all.contains(&"react-dom".to_string()));

        let prod = pkg.production_dependency_names();
        assert!(prod.contains(&"react".to_string()));
        assert!(!prod.contains(&"typescript".to_string()));

        let dev = pkg.dev_dependency_names();
        assert!(dev.contains(&"typescript".to_string()));
        assert!(!dev.contains(&"react".to_string()));
    }

    #[test]
    fn package_json_no_dependencies() {
        let pkg: PackageJson = serde_json::from_str(r#"{"name": "test"}"#).unwrap();
        assert!(pkg.all_dependency_names().is_empty());
        assert!(pkg.production_dependency_names().is_empty());
        assert!(pkg.dev_dependency_names().is_empty());
        assert!(pkg.entry_points().is_empty());
    }

    #[test]
    fn rules_deserialize_toml_kebab_case() {
        let toml_str = r#"
[rules]
unused-files = "error"
unused-exports = "warn"
unused-types = "off"
"#;
        let config: FallowConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.rules.unused_files, Severity::Error);
        assert_eq!(config.rules.unused_exports, Severity::Warn);
        assert_eq!(config.rules.unused_types, Severity::Off);
        // Unset fields default to error
        assert_eq!(config.rules.unresolved_imports, Severity::Error);
    }

    #[test]
    fn config_without_rules_defaults_to_error() {
        let toml_str = r#"
entry = ["src/main.ts"]
"#;
        let config: FallowConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.rules.unused_files, Severity::Error);
        assert_eq!(config.rules.unused_exports, Severity::Error);
    }

    #[test]
    fn fallow_config_denies_unknown_fields() {
        let toml_str = r"
unknown_field = true
";
        let result: Result<FallowConfig, _> = toml::from_str(toml_str);
        assert!(result.is_err());
    }

    #[test]
    fn fallow_config_deserialize_json() {
        let json_str = r#"{"entry": ["src/main.ts"]}"#;
        let config: FallowConfig = serde_json::from_str(json_str).unwrap();
        assert_eq!(config.entry, vec!["src/main.ts"]);
    }

    #[test]
    fn fallow_config_deserialize_jsonc() {
        let jsonc_str = r#"{
            // This is a comment
            "entry": ["src/main.ts"],
            "rules": {
                "unused-files": "warn"
            }
        }"#;
        let config: FallowConfig = crate::jsonc::parse_to_value(jsonc_str).unwrap();
        assert_eq!(config.entry, vec!["src/main.ts"]);
        assert_eq!(config.rules.unused_files, Severity::Warn);
    }

    #[test]
    fn fallow_config_json_with_schema_field() {
        let json_str = r#"{"$schema": "https://fallow.dev/schema.json", "entry": ["src/main.ts"]}"#;
        let config: FallowConfig = serde_json::from_str(json_str).unwrap();
        assert_eq!(config.entry, vec!["src/main.ts"]);
    }

    #[test]
    fn fallow_config_json_schema_generation() {
        let schema = FallowConfig::json_schema();
        assert!(schema.is_object());
        let obj = schema.as_object().unwrap();
        assert!(obj.contains_key("properties"));
    }

    #[test]
    fn config_format_detection() {
        assert!(matches!(
            ConfigFormat::from_path(Path::new("fallow.toml")),
            ConfigFormat::Toml
        ));
        assert!(matches!(
            ConfigFormat::from_path(Path::new(".fallowrc.json")),
            ConfigFormat::Json
        ));
        assert!(matches!(
            ConfigFormat::from_path(Path::new(".fallowrc.jsonc")),
            ConfigFormat::Json
        ));
        assert!(matches!(
            ConfigFormat::from_path(Path::new(".fallow.toml")),
            ConfigFormat::Toml
        ));
    }

    #[test]
    fn config_names_priority_order() {
        assert_eq!(CONFIG_NAMES[0], ".fallowrc.json");
        assert_eq!(CONFIG_NAMES[1], ".fallowrc.jsonc");
        assert_eq!(CONFIG_NAMES[2], "fallow.toml");
        assert_eq!(CONFIG_NAMES[3], ".fallow.toml");
    }

    #[test]
    fn load_json_config_file() {
        let dir = test_dir("json-config");
        let config_path = dir.path().join(".fallowrc.json");
        std::fs::write(
            &config_path,
            r#"{"entry": ["src/index.ts"], "rules": {"unused-exports": "warn"}}"#,
        )
        .unwrap();

        let config = FallowConfig::load(&config_path).unwrap();
        assert_eq!(config.entry, vec!["src/index.ts"]);
        assert_eq!(config.rules.unused_exports, Severity::Warn);
    }

    #[test]
    fn load_jsonc_config_file() {
        let dir = test_dir("jsonc-config");
        let config_path = dir.path().join(".fallowrc.json");
        std::fs::write(
            &config_path,
            r#"{
                // Entry points for analysis
                "entry": ["src/index.ts"],
                /* Block comment */
                "rules": {
                    "unused-exports": "warn"
                }
            }"#,
        )
        .unwrap();

        let config = FallowConfig::load(&config_path).unwrap();
        assert_eq!(config.entry, vec!["src/index.ts"]);
        assert_eq!(config.rules.unused_exports, Severity::Warn);
    }

    #[test]
    fn load_fallowrc_jsonc_extension() {
        let dir = test_dir("jsonc-extension");
        let config_path = dir.path().join(".fallowrc.jsonc");
        std::fs::write(
            &config_path,
            r#"{
                // editors that recognize the .jsonc extension show
                // proper JSON-with-comments syntax highlighting
                "ignoreDependencies": ["tailwindcss-react-aria-components"],
                "entry": ["src/index.ts"]
            }"#,
        )
        .unwrap();

        let config = FallowConfig::load(&config_path).unwrap();
        assert_eq!(config.entry, vec!["src/index.ts"]);
        assert_eq!(
            config.ignore_dependencies,
            vec!["tailwindcss-react-aria-components"]
        );
    }

    #[test]
    fn json_config_ignore_dependencies_camel_case() {
        let json_str = r#"{"ignoreDependencies": ["autoprefixer", "postcss"]}"#;
        let config: FallowConfig = serde_json::from_str(json_str).unwrap();
        assert_eq!(config.ignore_dependencies, vec!["autoprefixer", "postcss"]);
    }

    #[test]
    fn json_config_all_fields() {
        let json_str = r#"{
            "ignoreDependencies": ["lodash"],
            "ignoreExports": [{"file": "src/*.ts", "exports": ["*"]}],
            "rules": {
                "unused-files": "off",
                "unused-exports": "warn",
                "unused-dependencies": "error",
                "unused-dev-dependencies": "off",
                "unused-types": "warn",
                "unused-enum-members": "error",
                "unused-class-members": "off",
                "unresolved-imports": "warn",
                "unlisted-dependencies": "error",
                "duplicate-exports": "off"
            },
            "duplicates": {
                "minTokens": 100,
                "minLines": 10,
                "skipLocal": true
            }
        }"#;
        let config: FallowConfig = serde_json::from_str(json_str).unwrap();
        assert_eq!(config.ignore_dependencies, vec!["lodash"]);
        assert_eq!(config.rules.unused_files, Severity::Off);
        assert_eq!(config.rules.unused_exports, Severity::Warn);
        assert_eq!(config.rules.unused_dependencies, Severity::Error);
        assert_eq!(config.duplicates.min_tokens, 100);
        assert_eq!(config.duplicates.min_lines, 10);
        assert!(config.duplicates.skip_local);
    }

    // ── extends tests ──────────────────────────────────────────────

    #[test]
    fn extends_single_base() {
        let dir = test_dir("extends-single");

        std::fs::write(
            dir.path().join("base.json"),
            r#"{"rules": {"unused-files": "warn"}}"#,
        )
        .unwrap();
        std::fs::write(
            dir.path().join(".fallowrc.json"),
            r#"{"extends": ["base.json"], "entry": ["src/index.ts"]}"#,
        )
        .unwrap();

        let config = FallowConfig::load(&dir.path().join(".fallowrc.json")).unwrap();
        assert_eq!(config.rules.unused_files, Severity::Warn);
        assert_eq!(config.entry, vec!["src/index.ts"]);
        // Unset fields from base still default
        assert_eq!(config.rules.unused_exports, Severity::Error);
    }

    #[test]
    fn extends_overlay_overrides_base() {
        let dir = test_dir("extends-overlay");

        std::fs::write(
            dir.path().join("base.json"),
            r#"{"rules": {"unused-files": "warn", "unused-exports": "off"}}"#,
        )
        .unwrap();
        std::fs::write(
            dir.path().join(".fallowrc.json"),
            r#"{"extends": ["base.json"], "rules": {"unused-files": "error"}}"#,
        )
        .unwrap();

        let config = FallowConfig::load(&dir.path().join(".fallowrc.json")).unwrap();
        // Overlay overrides base
        assert_eq!(config.rules.unused_files, Severity::Error);
        // Base value preserved when not overridden
        assert_eq!(config.rules.unused_exports, Severity::Off);
    }

    #[test]
    fn extends_chained() {
        let dir = test_dir("extends-chained");

        std::fs::write(
            dir.path().join("grandparent.json"),
            r#"{"rules": {"unused-files": "off", "unused-exports": "warn"}}"#,
        )
        .unwrap();
        std::fs::write(
            dir.path().join("parent.json"),
            r#"{"extends": ["grandparent.json"], "rules": {"unused-files": "warn"}}"#,
        )
        .unwrap();
        std::fs::write(
            dir.path().join(".fallowrc.json"),
            r#"{"extends": ["parent.json"]}"#,
        )
        .unwrap();

        let config = FallowConfig::load(&dir.path().join(".fallowrc.json")).unwrap();
        // grandparent: off -> parent: warn -> child: inherits warn
        assert_eq!(config.rules.unused_files, Severity::Warn);
        // grandparent: warn, not overridden
        assert_eq!(config.rules.unused_exports, Severity::Warn);
    }

    #[test]
    fn extends_circular_detected() {
        let dir = test_dir("extends-circular");

        std::fs::write(dir.path().join("a.json"), r#"{"extends": ["b.json"]}"#).unwrap();
        std::fs::write(dir.path().join("b.json"), r#"{"extends": ["a.json"]}"#).unwrap();

        let result = FallowConfig::load(&dir.path().join("a.json"));
        assert!(result.is_err());
        let err_msg = format!("{}", result.unwrap_err());
        assert!(
            err_msg.contains("Circular extends"),
            "Expected circular error, got: {err_msg}"
        );
    }

    #[test]
    fn extends_missing_file_errors() {
        let dir = test_dir("extends-missing");

        std::fs::write(
            dir.path().join(".fallowrc.json"),
            r#"{"extends": ["nonexistent.json"]}"#,
        )
        .unwrap();

        let result = FallowConfig::load(&dir.path().join(".fallowrc.json"));
        assert!(result.is_err());
        let err_msg = format!("{}", result.unwrap_err());
        assert!(
            err_msg.contains("not found"),
            "Expected not found error, got: {err_msg}"
        );
    }

    // ── sealed: true tests ──────────────────────────────────────────

    #[test]
    fn sealed_allows_in_directory_extends() {
        let dir = test_dir("sealed-allows-local");
        std::fs::write(
            dir.path().join("base.json"),
            r#"{"ignorePatterns": ["gen/**"]}"#,
        )
        .unwrap();
        std::fs::write(
            dir.path().join(".fallowrc.json"),
            r#"{"sealed": true, "extends": ["./base.json"]}"#,
        )
        .unwrap();

        let config = FallowConfig::load(&dir.path().join(".fallowrc.json")).unwrap();
        assert!(config.sealed);
        assert_eq!(config.ignore_patterns, vec!["gen/**"]);
    }

    #[test]
    fn sealed_rejects_extends_escaping_directory() {
        let dir = test_dir("sealed-rejects-escape");
        let sub = dir.path().join("packages").join("app");
        std::fs::create_dir_all(&sub).unwrap();

        // Base config above the sealed config's directory
        std::fs::write(
            dir.path().join("base.json"),
            r#"{"ignorePatterns": ["dist/**"]}"#,
        )
        .unwrap();
        std::fs::write(
            sub.join(".fallowrc.json"),
            r#"{"sealed": true, "extends": ["../../base.json"]}"#,
        )
        .unwrap();

        let result = FallowConfig::load(&sub.join(".fallowrc.json"));
        assert!(
            result.is_err(),
            "Expected sealed config to reject escaping extends"
        );
        let err_msg = format!("{}", result.unwrap_err());
        assert!(
            err_msg.contains("sealed"),
            "Error must mention sealed: {err_msg}"
        );
        assert!(
            err_msg.contains("outside the config's directory"),
            "Error must explain the constraint: {err_msg}"
        );
    }

    #[test]
    fn sealed_rejects_https_extends() {
        let dir = test_dir("sealed-rejects-https");
        std::fs::write(
            dir.path().join(".fallowrc.json"),
            r#"{"sealed": true, "extends": ["https://example.com/base.json"]}"#,
        )
        .unwrap();

        let result = FallowConfig::load(&dir.path().join(".fallowrc.json"));
        assert!(result.is_err());
        let err_msg = format!("{}", result.unwrap_err());
        assert!(
            err_msg.contains("sealed"),
            "Error must mention sealed: {err_msg}"
        );
        assert!(
            err_msg.contains("URL extends"),
            "Error must mention URL: {err_msg}"
        );
    }

    #[test]
    fn sealed_rejects_npm_extends() {
        let dir = test_dir("sealed-rejects-npm");
        std::fs::write(
            dir.path().join(".fallowrc.json"),
            r#"{"sealed": true, "extends": ["npm:@scope/config"]}"#,
        )
        .unwrap();

        let result = FallowConfig::load(&dir.path().join(".fallowrc.json"));
        assert!(result.is_err());
        let err_msg = format!("{}", result.unwrap_err());
        assert!(
            err_msg.contains("sealed"),
            "Error must mention sealed: {err_msg}"
        );
        assert!(
            err_msg.contains("npm extends"),
            "Error must mention npm: {err_msg}"
        );
    }

    #[test]
    fn sealed_default_is_false() {
        let dir = test_dir("sealed-default");
        std::fs::write(dir.path().join(".fallowrc.json"), "{}").unwrap();
        let config = FallowConfig::load(&dir.path().join(".fallowrc.json")).unwrap();
        assert!(!config.sealed);
    }

    #[test]
    fn sealed_false_allows_escaping_extends() {
        // Without sealed (or sealed: false), escaping extends works fine
        let dir = test_dir("sealed-false-allows");
        let sub = dir.path().join("packages").join("app");
        std::fs::create_dir_all(&sub).unwrap();

        std::fs::write(
            dir.path().join("base.json"),
            r#"{"ignorePatterns": ["dist/**"]}"#,
        )
        .unwrap();
        std::fs::write(
            sub.join(".fallowrc.json"),
            r#"{"extends": ["../../base.json"]}"#,
        )
        .unwrap();

        let config = FallowConfig::load(&sub.join(".fallowrc.json")).unwrap();
        assert!(!config.sealed);
        assert_eq!(config.ignore_patterns, vec!["dist/**"]);
    }

    #[test]
    fn extends_string_sugar() {
        let dir = test_dir("extends-string");

        std::fs::write(
            dir.path().join("base.json"),
            r#"{"ignorePatterns": ["gen/**"]}"#,
        )
        .unwrap();
        // String form instead of array
        std::fs::write(
            dir.path().join(".fallowrc.json"),
            r#"{"extends": "base.json"}"#,
        )
        .unwrap();

        let config = FallowConfig::load(&dir.path().join(".fallowrc.json")).unwrap();
        assert_eq!(config.ignore_patterns, vec!["gen/**"]);
    }

    #[test]
    fn extends_deep_merge_preserves_arrays() {
        let dir = test_dir("extends-array");

        std::fs::write(dir.path().join("base.json"), r#"{"entry": ["src/a.ts"]}"#).unwrap();
        std::fs::write(
            dir.path().join(".fallowrc.json"),
            r#"{"extends": ["base.json"], "entry": ["src/b.ts"]}"#,
        )
        .unwrap();

        let config = FallowConfig::load(&dir.path().join(".fallowrc.json")).unwrap();
        // Arrays are replaced, not merged (overlay replaces base)
        assert_eq!(config.entry, vec!["src/b.ts"]);
    }

    // ── npm extends tests ────────────────────────────────────────────

    /// Set up a fake npm package in `node_modules/<name>` under `root`.
    fn create_npm_package(root: &Path, name: &str, config_json: &str) {
        let pkg_dir = root.join("node_modules").join(name);
        std::fs::create_dir_all(&pkg_dir).unwrap();
        std::fs::write(pkg_dir.join(".fallowrc.json"), config_json).unwrap();
    }

    /// Set up a fake npm package with `package.json` `main` field.
    fn create_npm_package_with_main(root: &Path, name: &str, main: &str, config_json: &str) {
        let pkg_dir = root.join("node_modules").join(name);
        std::fs::create_dir_all(&pkg_dir).unwrap();
        std::fs::write(
            pkg_dir.join("package.json"),
            format!(r#"{{"name": "{name}", "main": "{main}"}}"#),
        )
        .unwrap();
        std::fs::write(pkg_dir.join(main), config_json).unwrap();
    }

    #[test]
    fn extends_npm_basic_unscoped() {
        let dir = test_dir("npm-basic");
        create_npm_package(
            dir.path(),
            "fallow-config-acme",
            r#"{"rules": {"unused-files": "warn"}}"#,
        );
        std::fs::write(
            dir.path().join(".fallowrc.json"),
            r#"{"extends": "npm:fallow-config-acme"}"#,
        )
        .unwrap();

        let config = FallowConfig::load(&dir.path().join(".fallowrc.json")).unwrap();
        assert_eq!(config.rules.unused_files, Severity::Warn);
    }

    #[test]
    fn extends_npm_scoped_package() {
        let dir = test_dir("npm-scoped");
        create_npm_package(
            dir.path(),
            "@company/fallow-config",
            r#"{"rules": {"unused-exports": "off"}, "ignorePatterns": ["generated/**"]}"#,
        );
        std::fs::write(
            dir.path().join(".fallowrc.json"),
            r#"{"extends": "npm:@company/fallow-config"}"#,
        )
        .unwrap();

        let config = FallowConfig::load(&dir.path().join(".fallowrc.json")).unwrap();
        assert_eq!(config.rules.unused_exports, Severity::Off);
        assert_eq!(config.ignore_patterns, vec!["generated/**"]);
    }

    #[test]
    fn extends_npm_with_subpath() {
        let dir = test_dir("npm-subpath");
        let pkg_dir = dir.path().join("node_modules/@company/fallow-config");
        std::fs::create_dir_all(&pkg_dir).unwrap();
        std::fs::write(
            pkg_dir.join("strict.json"),
            r#"{"rules": {"unused-files": "error", "unused-exports": "error"}}"#,
        )
        .unwrap();

        std::fs::write(
            dir.path().join(".fallowrc.json"),
            r#"{"extends": "npm:@company/fallow-config/strict.json"}"#,
        )
        .unwrap();

        let config = FallowConfig::load(&dir.path().join(".fallowrc.json")).unwrap();
        assert_eq!(config.rules.unused_files, Severity::Error);
        assert_eq!(config.rules.unused_exports, Severity::Error);
    }

    #[test]
    fn extends_npm_package_json_main() {
        let dir = test_dir("npm-main");
        create_npm_package_with_main(
            dir.path(),
            "fallow-config-acme",
            "config.json",
            r#"{"rules": {"unused-types": "off"}}"#,
        );
        std::fs::write(
            dir.path().join(".fallowrc.json"),
            r#"{"extends": "npm:fallow-config-acme"}"#,
        )
        .unwrap();

        let config = FallowConfig::load(&dir.path().join(".fallowrc.json")).unwrap();
        assert_eq!(config.rules.unused_types, Severity::Off);
    }

    #[test]
    fn extends_npm_package_json_exports_string() {
        let dir = test_dir("npm-exports-str");
        let pkg_dir = dir.path().join("node_modules/fallow-config-co");
        std::fs::create_dir_all(&pkg_dir).unwrap();
        std::fs::write(
            pkg_dir.join("package.json"),
            r#"{"name": "fallow-config-co", "exports": "./base.json"}"#,
        )
        .unwrap();
        std::fs::write(
            pkg_dir.join("base.json"),
            r#"{"rules": {"circular-dependencies": "warn"}}"#,
        )
        .unwrap();

        std::fs::write(
            dir.path().join(".fallowrc.json"),
            r#"{"extends": "npm:fallow-config-co"}"#,
        )
        .unwrap();

        let config = FallowConfig::load(&dir.path().join(".fallowrc.json")).unwrap();
        assert_eq!(config.rules.circular_dependencies, Severity::Warn);
    }

    #[test]
    fn extends_npm_package_json_exports_object() {
        let dir = test_dir("npm-exports-obj");
        let pkg_dir = dir.path().join("node_modules/@co/cfg");
        std::fs::create_dir_all(&pkg_dir).unwrap();
        std::fs::write(
            pkg_dir.join("package.json"),
            r#"{"name": "@co/cfg", "exports": {".": {"default": "./fallow.json"}}}"#,
        )
        .unwrap();
        std::fs::write(pkg_dir.join("fallow.json"), r#"{"entry": ["src/app.ts"]}"#).unwrap();

        std::fs::write(
            dir.path().join(".fallowrc.json"),
            r#"{"extends": "npm:@co/cfg"}"#,
        )
        .unwrap();

        let config = FallowConfig::load(&dir.path().join(".fallowrc.json")).unwrap();
        assert_eq!(config.entry, vec!["src/app.ts"]);
    }

    #[test]
    fn extends_npm_exports_takes_priority_over_main() {
        let dir = test_dir("npm-exports-prio");
        let pkg_dir = dir.path().join("node_modules/my-config");
        std::fs::create_dir_all(&pkg_dir).unwrap();
        std::fs::write(
            pkg_dir.join("package.json"),
            r#"{"name": "my-config", "main": "./old.json", "exports": "./new.json"}"#,
        )
        .unwrap();
        std::fs::write(
            pkg_dir.join("old.json"),
            r#"{"rules": {"unused-files": "off"}}"#,
        )
        .unwrap();
        std::fs::write(
            pkg_dir.join("new.json"),
            r#"{"rules": {"unused-files": "warn"}}"#,
        )
        .unwrap();

        std::fs::write(
            dir.path().join(".fallowrc.json"),
            r#"{"extends": "npm:my-config"}"#,
        )
        .unwrap();

        let config = FallowConfig::load(&dir.path().join(".fallowrc.json")).unwrap();
        // exports takes priority over main
        assert_eq!(config.rules.unused_files, Severity::Warn);
    }

    #[test]
    fn extends_npm_walk_up_directories() {
        let dir = test_dir("npm-walkup");
        // node_modules at root level
        create_npm_package(
            dir.path(),
            "shared-config",
            r#"{"rules": {"unused-files": "warn"}}"#,
        );
        // Config in a nested subdirectory
        let sub = dir.path().join("packages/app");
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::write(
            sub.join(".fallowrc.json"),
            r#"{"extends": "npm:shared-config"}"#,
        )
        .unwrap();

        let config = FallowConfig::load(&sub.join(".fallowrc.json")).unwrap();
        assert_eq!(config.rules.unused_files, Severity::Warn);
    }

    #[test]
    fn extends_npm_overlay_overrides_base() {
        let dir = test_dir("npm-overlay");
        create_npm_package(
            dir.path(),
            "@company/base",
            r#"{"rules": {"unused-files": "warn", "unused-exports": "off"}, "entry": ["src/base.ts"]}"#,
        );
        std::fs::write(
            dir.path().join(".fallowrc.json"),
            r#"{"extends": "npm:@company/base", "rules": {"unused-files": "error"}, "entry": ["src/app.ts"]}"#,
        )
        .unwrap();

        let config = FallowConfig::load(&dir.path().join(".fallowrc.json")).unwrap();
        assert_eq!(config.rules.unused_files, Severity::Error);
        assert_eq!(config.rules.unused_exports, Severity::Off);
        assert_eq!(config.entry, vec!["src/app.ts"]);
    }

    #[test]
    fn extends_npm_chained_with_relative() {
        let dir = test_dir("npm-chained");
        // npm package extends a relative file inside itself
        let pkg_dir = dir.path().join("node_modules/my-config");
        std::fs::create_dir_all(&pkg_dir).unwrap();
        std::fs::write(
            pkg_dir.join("base.json"),
            r#"{"rules": {"unused-files": "warn"}}"#,
        )
        .unwrap();
        std::fs::write(
            pkg_dir.join(".fallowrc.json"),
            r#"{"extends": ["base.json"], "rules": {"unused-exports": "off"}}"#,
        )
        .unwrap();

        std::fs::write(
            dir.path().join(".fallowrc.json"),
            r#"{"extends": "npm:my-config"}"#,
        )
        .unwrap();

        let config = FallowConfig::load(&dir.path().join(".fallowrc.json")).unwrap();
        assert_eq!(config.rules.unused_files, Severity::Warn);
        assert_eq!(config.rules.unused_exports, Severity::Off);
    }

    #[test]
    fn extends_npm_mixed_with_relative_paths() {
        let dir = test_dir("npm-mixed");
        create_npm_package(
            dir.path(),
            "shared-base",
            r#"{"rules": {"unused-files": "off"}}"#,
        );
        std::fs::write(
            dir.path().join("local-overrides.json"),
            r#"{"rules": {"unused-files": "warn"}}"#,
        )
        .unwrap();
        std::fs::write(
            dir.path().join(".fallowrc.json"),
            r#"{"extends": ["npm:shared-base", "local-overrides.json"]}"#,
        )
        .unwrap();

        let config = FallowConfig::load(&dir.path().join(".fallowrc.json")).unwrap();
        // local-overrides is later in the array, so it wins
        assert_eq!(config.rules.unused_files, Severity::Warn);
    }

    #[test]
    fn extends_npm_missing_package_errors() {
        let dir = test_dir("npm-missing");
        std::fs::write(
            dir.path().join(".fallowrc.json"),
            r#"{"extends": "npm:nonexistent-package"}"#,
        )
        .unwrap();

        let result = FallowConfig::load(&dir.path().join(".fallowrc.json"));
        assert!(result.is_err());
        let err_msg = format!("{}", result.unwrap_err());
        assert!(
            err_msg.contains("not found"),
            "Expected 'not found' error, got: {err_msg}"
        );
        assert!(
            err_msg.contains("nonexistent-package"),
            "Expected package name in error, got: {err_msg}"
        );
        assert!(
            err_msg.contains("install it"),
            "Expected install hint in error, got: {err_msg}"
        );
    }

    #[test]
    fn extends_npm_no_config_in_package_errors() {
        let dir = test_dir("npm-no-config");
        let pkg_dir = dir.path().join("node_modules/empty-pkg");
        std::fs::create_dir_all(&pkg_dir).unwrap();
        // Package exists but has no config files and no package.json
        std::fs::write(pkg_dir.join("README.md"), "# empty").unwrap();

        std::fs::write(
            dir.path().join(".fallowrc.json"),
            r#"{"extends": "npm:empty-pkg"}"#,
        )
        .unwrap();

        let result = FallowConfig::load(&dir.path().join(".fallowrc.json"));
        assert!(result.is_err());
        let err_msg = format!("{}", result.unwrap_err());
        assert!(
            err_msg.contains("No fallow config found"),
            "Expected 'No fallow config found' error, got: {err_msg}"
        );
    }

    #[test]
    fn extends_npm_missing_subpath_errors() {
        let dir = test_dir("npm-missing-sub");
        let pkg_dir = dir.path().join("node_modules/@co/config");
        std::fs::create_dir_all(&pkg_dir).unwrap();

        std::fs::write(
            dir.path().join(".fallowrc.json"),
            r#"{"extends": "npm:@co/config/nonexistent.json"}"#,
        )
        .unwrap();

        let result = FallowConfig::load(&dir.path().join(".fallowrc.json"));
        assert!(result.is_err());
        let err_msg = format!("{}", result.unwrap_err());
        assert!(
            err_msg.contains("nonexistent.json"),
            "Expected subpath in error, got: {err_msg}"
        );
    }

    #[test]
    fn extends_npm_empty_specifier_errors() {
        let dir = test_dir("npm-empty");
        std::fs::write(dir.path().join(".fallowrc.json"), r#"{"extends": "npm:"}"#).unwrap();

        let result = FallowConfig::load(&dir.path().join(".fallowrc.json"));
        assert!(result.is_err());
        let err_msg = format!("{}", result.unwrap_err());
        assert!(
            err_msg.contains("Empty npm specifier"),
            "Expected 'Empty npm specifier' error, got: {err_msg}"
        );
    }

    #[test]
    fn extends_npm_space_after_colon_trimmed() {
        let dir = test_dir("npm-space");
        create_npm_package(
            dir.path(),
            "fallow-config-acme",
            r#"{"rules": {"unused-files": "warn"}}"#,
        );
        // Space after npm: — should be trimmed and resolve correctly
        std::fs::write(
            dir.path().join(".fallowrc.json"),
            r#"{"extends": "npm: fallow-config-acme"}"#,
        )
        .unwrap();

        let config = FallowConfig::load(&dir.path().join(".fallowrc.json")).unwrap();
        assert_eq!(config.rules.unused_files, Severity::Warn);
    }

    #[test]
    fn extends_npm_exports_node_condition() {
        let dir = test_dir("npm-node-cond");
        let pkg_dir = dir.path().join("node_modules/node-config");
        std::fs::create_dir_all(&pkg_dir).unwrap();
        std::fs::write(
            pkg_dir.join("package.json"),
            r#"{"name": "node-config", "exports": {".": {"node": "./node.json"}}}"#,
        )
        .unwrap();
        std::fs::write(
            pkg_dir.join("node.json"),
            r#"{"rules": {"unused-files": "off"}}"#,
        )
        .unwrap();

        std::fs::write(
            dir.path().join(".fallowrc.json"),
            r#"{"extends": "npm:node-config"}"#,
        )
        .unwrap();

        let config = FallowConfig::load(&dir.path().join(".fallowrc.json")).unwrap();
        assert_eq!(config.rules.unused_files, Severity::Off);
    }

    // ── parse_npm_specifier unit tests ──────────────────────────────

    #[test]
    fn parse_npm_specifier_unscoped() {
        assert_eq!(parse_npm_specifier("my-config"), ("my-config", None));
    }

    #[test]
    fn parse_npm_specifier_unscoped_with_subpath() {
        assert_eq!(
            parse_npm_specifier("my-config/strict.json"),
            ("my-config", Some("strict.json"))
        );
    }

    #[test]
    fn parse_npm_specifier_scoped() {
        assert_eq!(
            parse_npm_specifier("@company/fallow-config"),
            ("@company/fallow-config", None)
        );
    }

    #[test]
    fn parse_npm_specifier_scoped_with_subpath() {
        assert_eq!(
            parse_npm_specifier("@company/fallow-config/strict.json"),
            ("@company/fallow-config", Some("strict.json"))
        );
    }

    #[test]
    fn parse_npm_specifier_scoped_with_nested_subpath() {
        assert_eq!(
            parse_npm_specifier("@company/fallow-config/presets/strict.json"),
            ("@company/fallow-config", Some("presets/strict.json"))
        );
    }

    // ── npm extends security tests ──────────────────────────────────

    #[test]
    fn extends_npm_subpath_traversal_rejected() {
        let dir = test_dir("npm-traversal-sub");
        let pkg_dir = dir.path().join("node_modules/evil-pkg");
        std::fs::create_dir_all(&pkg_dir).unwrap();
        // Create a file outside the package that the traversal would reach
        std::fs::write(
            dir.path().join("secret.json"),
            r#"{"entry": ["stolen.ts"]}"#,
        )
        .unwrap();

        std::fs::write(
            dir.path().join(".fallowrc.json"),
            r#"{"extends": "npm:evil-pkg/../../secret.json"}"#,
        )
        .unwrap();

        let result = FallowConfig::load(&dir.path().join(".fallowrc.json"));
        assert!(result.is_err());
        let err_msg = format!("{}", result.unwrap_err());
        assert!(
            err_msg.contains("traversal") || err_msg.contains("not found"),
            "Expected traversal or not-found error, got: {err_msg}"
        );
    }

    #[test]
    fn extends_npm_dotdot_package_name_rejected() {
        let dir = test_dir("npm-dotdot-name");
        std::fs::write(
            dir.path().join(".fallowrc.json"),
            r#"{"extends": "npm:../relative"}"#,
        )
        .unwrap();

        let result = FallowConfig::load(&dir.path().join(".fallowrc.json"));
        assert!(result.is_err());
        let err_msg = format!("{}", result.unwrap_err());
        assert!(
            err_msg.contains("path traversal"),
            "Expected 'path traversal' error, got: {err_msg}"
        );
    }

    #[test]
    fn extends_npm_scoped_without_name_rejected() {
        let dir = test_dir("npm-scope-only");
        std::fs::write(
            dir.path().join(".fallowrc.json"),
            r#"{"extends": "npm:@scope"}"#,
        )
        .unwrap();

        let result = FallowConfig::load(&dir.path().join(".fallowrc.json"));
        assert!(result.is_err());
        let err_msg = format!("{}", result.unwrap_err());
        assert!(
            err_msg.contains("@scope/name"),
            "Expected scoped name format error, got: {err_msg}"
        );
    }

    #[test]
    fn extends_npm_malformed_package_json_errors() {
        let dir = test_dir("npm-bad-pkgjson");
        let pkg_dir = dir.path().join("node_modules/bad-pkg");
        std::fs::create_dir_all(&pkg_dir).unwrap();
        std::fs::write(pkg_dir.join("package.json"), "{ not valid json }").unwrap();

        std::fs::write(
            dir.path().join(".fallowrc.json"),
            r#"{"extends": "npm:bad-pkg"}"#,
        )
        .unwrap();

        let result = FallowConfig::load(&dir.path().join(".fallowrc.json"));
        assert!(result.is_err());
        let err_msg = format!("{}", result.unwrap_err());
        assert!(
            err_msg.contains("Failed to parse"),
            "Expected parse error, got: {err_msg}"
        );
    }

    #[test]
    fn extends_npm_exports_traversal_rejected() {
        let dir = test_dir("npm-exports-escape");
        let pkg_dir = dir.path().join("node_modules/evil-exports");
        std::fs::create_dir_all(&pkg_dir).unwrap();
        std::fs::write(
            pkg_dir.join("package.json"),
            r#"{"name": "evil-exports", "exports": "../../secret.json"}"#,
        )
        .unwrap();
        // Create the target file outside the package
        std::fs::write(
            dir.path().join("secret.json"),
            r#"{"entry": ["stolen.ts"]}"#,
        )
        .unwrap();

        std::fs::write(
            dir.path().join(".fallowrc.json"),
            r#"{"extends": "npm:evil-exports"}"#,
        )
        .unwrap();

        let result = FallowConfig::load(&dir.path().join(".fallowrc.json"));
        assert!(result.is_err());
        let err_msg = format!("{}", result.unwrap_err());
        assert!(
            err_msg.contains("traversal"),
            "Expected traversal error, got: {err_msg}"
        );
    }

    // ── deep_merge_json unit tests ───────────────────────────────────

    #[test]
    fn deep_merge_scalar_overlay_replaces_base() {
        let mut base = serde_json::json!("hello");
        deep_merge_json(&mut base, serde_json::json!("world"));
        assert_eq!(base, serde_json::json!("world"));
    }

    #[test]
    fn deep_merge_array_overlay_replaces_base() {
        let mut base = serde_json::json!(["a", "b"]);
        deep_merge_json(&mut base, serde_json::json!(["c"]));
        assert_eq!(base, serde_json::json!(["c"]));
    }

    #[test]
    fn deep_merge_nested_object_merge() {
        let mut base = serde_json::json!({
            "level1": {
                "level2": {
                    "a": 1,
                    "b": 2
                }
            }
        });
        let overlay = serde_json::json!({
            "level1": {
                "level2": {
                    "b": 99,
                    "c": 3
                }
            }
        });
        deep_merge_json(&mut base, overlay);
        assert_eq!(base["level1"]["level2"]["a"], 1);
        assert_eq!(base["level1"]["level2"]["b"], 99);
        assert_eq!(base["level1"]["level2"]["c"], 3);
    }

    #[test]
    fn deep_merge_overlay_adds_new_fields() {
        let mut base = serde_json::json!({"existing": true});
        let overlay = serde_json::json!({"new_field": "added", "another": 42});
        deep_merge_json(&mut base, overlay);
        assert_eq!(base["existing"], true);
        assert_eq!(base["new_field"], "added");
        assert_eq!(base["another"], 42);
    }

    #[test]
    fn deep_merge_null_overlay_replaces_object() {
        let mut base = serde_json::json!({"key": "value"});
        deep_merge_json(&mut base, serde_json::json!(null));
        assert_eq!(base, serde_json::json!(null));
    }

    #[test]
    fn deep_merge_empty_object_overlay_preserves_base() {
        let mut base = serde_json::json!({"a": 1, "b": 2});
        deep_merge_json(&mut base, serde_json::json!({}));
        assert_eq!(base, serde_json::json!({"a": 1, "b": 2}));
    }

    // ── rule severity parsing via JSON config ────────────────────────

    #[test]
    fn rules_severity_error_warn_off_from_json() {
        let json_str = r#"{
            "rules": {
                "unused-files": "error",
                "unused-exports": "warn",
                "unused-types": "off"
            }
        }"#;
        let config: FallowConfig = serde_json::from_str(json_str).unwrap();
        assert_eq!(config.rules.unused_files, Severity::Error);
        assert_eq!(config.rules.unused_exports, Severity::Warn);
        assert_eq!(config.rules.unused_types, Severity::Off);
    }

    #[test]
    fn rules_omitted_default_to_error() {
        let json_str = r#"{
            "rules": {
                "unused-files": "warn"
            }
        }"#;
        let config: FallowConfig = serde_json::from_str(json_str).unwrap();
        assert_eq!(config.rules.unused_files, Severity::Warn);
        // All other rules default to error
        assert_eq!(config.rules.unused_exports, Severity::Error);
        assert_eq!(config.rules.unused_types, Severity::Error);
        assert_eq!(config.rules.unused_dependencies, Severity::Error);
        assert_eq!(config.rules.unresolved_imports, Severity::Error);
        assert_eq!(config.rules.unlisted_dependencies, Severity::Error);
        assert_eq!(config.rules.duplicate_exports, Severity::Error);
        assert_eq!(config.rules.circular_dependencies, Severity::Error);
        // type_only_dependencies defaults to warn, not error
        assert_eq!(config.rules.type_only_dependencies, Severity::Warn);
    }

    // ── find_and_load tests ───────────────────────────────────────

    #[test]
    fn find_and_load_returns_none_when_no_config() {
        let dir = test_dir("find-none");
        // Create a .git dir so it stops searching
        std::fs::create_dir(dir.path().join(".git")).unwrap();

        let result = FallowConfig::find_and_load(dir.path()).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn find_and_load_finds_fallowrc_json() {
        let dir = test_dir("find-json");
        std::fs::create_dir(dir.path().join(".git")).unwrap();
        std::fs::write(
            dir.path().join(".fallowrc.json"),
            r#"{"entry": ["src/main.ts"]}"#,
        )
        .unwrap();

        let (config, path) = FallowConfig::find_and_load(dir.path()).unwrap().unwrap();
        assert_eq!(config.entry, vec!["src/main.ts"]);
        assert!(path.ends_with(".fallowrc.json"));
    }

    #[test]
    fn find_and_load_finds_fallowrc_jsonc() {
        let dir = test_dir("find-jsonc");
        std::fs::create_dir(dir.path().join(".git")).unwrap();
        std::fs::write(
            dir.path().join(".fallowrc.jsonc"),
            r#"{
                // jsonc with comments, picked up by auto-discovery
                "entry": ["src/main.ts"]
            }"#,
        )
        .unwrap();

        let (config, path) = FallowConfig::find_and_load(dir.path()).unwrap().unwrap();
        assert_eq!(config.entry, vec!["src/main.ts"]);
        assert!(path.ends_with(".fallowrc.jsonc"));
    }

    #[test]
    fn find_and_load_prefers_fallowrc_json_over_jsonc() {
        // First-match-wins: `.fallowrc.json` ranks above `.fallowrc.jsonc`
        // in `CONFIG_NAMES`, mirroring tsconfig.json > tsconfig.jsonc precedence.
        let dir = test_dir("find-json-vs-jsonc");
        std::fs::create_dir(dir.path().join(".git")).unwrap();
        std::fs::write(
            dir.path().join(".fallowrc.json"),
            r#"{"entry": ["from-json.ts"]}"#,
        )
        .unwrap();
        std::fs::write(
            dir.path().join(".fallowrc.jsonc"),
            r#"{"entry": ["from-jsonc.ts"]}"#,
        )
        .unwrap();

        let (config, path) = FallowConfig::find_and_load(dir.path()).unwrap().unwrap();
        assert_eq!(config.entry, vec!["from-json.ts"]);
        assert!(path.ends_with(".fallowrc.json"));
    }

    #[test]
    fn find_and_load_prefers_fallowrc_json_over_toml() {
        let dir = test_dir("find-priority");
        std::fs::create_dir(dir.path().join(".git")).unwrap();
        std::fs::write(
            dir.path().join(".fallowrc.json"),
            r#"{"entry": ["from-json.ts"]}"#,
        )
        .unwrap();
        std::fs::write(
            dir.path().join("fallow.toml"),
            "entry = [\"from-toml.ts\"]\n",
        )
        .unwrap();

        let (config, path) = FallowConfig::find_and_load(dir.path()).unwrap().unwrap();
        assert_eq!(config.entry, vec!["from-json.ts"]);
        assert!(path.ends_with(".fallowrc.json"));
    }

    #[test]
    fn find_and_load_finds_fallow_toml() {
        let dir = test_dir("find-toml");
        std::fs::create_dir(dir.path().join(".git")).unwrap();
        std::fs::write(
            dir.path().join("fallow.toml"),
            "entry = [\"src/index.ts\"]\n",
        )
        .unwrap();

        let (config, _) = FallowConfig::find_and_load(dir.path()).unwrap().unwrap();
        assert_eq!(config.entry, vec!["src/index.ts"]);
    }

    #[test]
    fn find_and_load_stops_at_git_dir() {
        let dir = test_dir("find-git-stop");
        let sub = dir.path().join("sub");
        std::fs::create_dir(&sub).unwrap();
        // .git marker in root stops search
        std::fs::create_dir(dir.path().join(".git")).unwrap();
        // Config file above .git should not be found from sub
        // (sub has no .git or package.json, so it keeps searching up to parent)
        // But parent has .git, so it stops there without finding config
        let result = FallowConfig::find_and_load(&sub).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn find_and_load_walks_past_package_json_in_monorepo() {
        // Simulate a pnpm/npm/yarn workspace: root has `.git` + `.fallowrc.json`,
        // sub-package has its own `package.json`. Config search from the
        // sub-package must walk past its `package.json` and find the root config.
        let dir = test_dir("find-monorepo");
        std::fs::create_dir(dir.path().join(".git")).unwrap();
        std::fs::write(
            dir.path().join(".fallowrc.json"),
            r#"{"entry": ["src/index.ts"]}"#,
        )
        .unwrap();

        let sub = dir.path().join("packages").join("app");
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::write(sub.join("package.json"), r#"{"name": "@scope/app"}"#).unwrap();

        let (config, path) = FallowConfig::find_and_load(&sub).unwrap().unwrap();
        assert_eq!(config.entry, vec!["src/index.ts"]);
        assert_eq!(path, dir.path().join(".fallowrc.json"));
    }

    #[test]
    fn find_and_load_sub_package_config_wins_over_root() {
        // Regression guard: if a monorepo sub-package has its own config,
        // it must be preferred over the root config (first-match-wins).
        let dir = test_dir("find-monorepo-override");
        std::fs::create_dir(dir.path().join(".git")).unwrap();
        std::fs::write(
            dir.path().join(".fallowrc.json"),
            r#"{"entry": ["src/root.ts"]}"#,
        )
        .unwrap();

        let sub = dir.path().join("packages").join("app");
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::write(sub.join("package.json"), r#"{"name": "@scope/app"}"#).unwrap();
        std::fs::write(sub.join(".fallowrc.json"), r#"{"entry": ["src/sub.ts"]}"#).unwrap();

        let (config, path) = FallowConfig::find_and_load(&sub).unwrap().unwrap();
        assert_eq!(config.entry, vec!["src/sub.ts"]);
        assert_eq!(path, sub.join(".fallowrc.json"));
    }

    #[test]
    fn find_and_load_stops_at_git_file_submodule() {
        // Git submodules / worktrees have `.git` as a file (not a directory)
        // pointing to the real gitdir. `.exists()` matches both, so submodule
        // roots correctly stop the walk — config in the parent repo should
        // NOT leak into a vendored submodule.
        let dir = test_dir("find-git-file");
        std::fs::create_dir(dir.path().join(".git")).unwrap();
        std::fs::write(
            dir.path().join(".fallowrc.json"),
            r#"{"entry": ["src/parent.ts"]}"#,
        )
        .unwrap();

        let submodule = dir.path().join("vendor").join("lib");
        std::fs::create_dir_all(&submodule).unwrap();
        // Simulate submodule: `.git` as a file pointing to parent's .git/modules
        std::fs::write(submodule.join(".git"), "gitdir: ../../.git/modules/lib\n").unwrap();

        let result = FallowConfig::find_and_load(&submodule).unwrap();
        assert!(
            result.is_none(),
            "submodule boundary should stop config walk",
        );
    }

    #[test]
    fn find_and_load_stops_at_hg_dir() {
        let dir = test_dir("find-hg-stop");
        let sub = dir.path().join("sub");
        std::fs::create_dir(&sub).unwrap();
        std::fs::create_dir(dir.path().join(".hg")).unwrap();

        let result = FallowConfig::find_and_load(&sub).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn find_and_load_returns_error_for_invalid_config() {
        let dir = test_dir("find-invalid");
        std::fs::create_dir(dir.path().join(".git")).unwrap();
        std::fs::write(
            dir.path().join(".fallowrc.json"),
            r"{ this is not valid json }",
        )
        .unwrap();

        let result = FallowConfig::find_and_load(dir.path());
        assert!(result.is_err());
    }

    // ── load TOML config file ────────────────────────────────────

    #[test]
    fn load_toml_config_file() {
        let dir = test_dir("toml-config");
        let config_path = dir.path().join("fallow.toml");
        std::fs::write(
            &config_path,
            r#"
entry = ["src/index.ts"]
ignorePatterns = ["dist/**"]

[rules]
unused-files = "warn"

[duplicates]
minTokens = 100
"#,
        )
        .unwrap();

        let config = FallowConfig::load(&config_path).unwrap();
        assert_eq!(config.entry, vec!["src/index.ts"]);
        assert_eq!(config.ignore_patterns, vec!["dist/**"]);
        assert_eq!(config.rules.unused_files, Severity::Warn);
        assert_eq!(config.duplicates.min_tokens, 100);
    }

    // ── extends absolute path rejection ──────────────────────────

    #[test]
    fn extends_absolute_path_rejected() {
        let dir = test_dir("extends-absolute");

        // Use a platform-appropriate absolute path
        #[cfg(unix)]
        let abs_path = "/absolute/path/config.json";
        #[cfg(windows)]
        let abs_path = "C:\\absolute\\path\\config.json";

        let json = format!(r#"{{"extends": ["{}"]}}"#, abs_path.replace('\\', "\\\\"));
        std::fs::write(dir.path().join(".fallowrc.json"), json).unwrap();

        let result = FallowConfig::load(&dir.path().join(".fallowrc.json"));
        assert!(result.is_err());
        let err_msg = format!("{}", result.unwrap_err());
        assert!(
            err_msg.contains("must be relative"),
            "Expected 'must be relative' error, got: {err_msg}"
        );
    }

    // ── resolve production mode ─────────────────────────────────

    #[test]
    fn resolve_production_mode_disables_dev_deps() {
        let config = FallowConfig {
            production: true.into(),
            ..Default::default()
        };
        let resolved = config.resolve(
            PathBuf::from("/tmp/test"),
            OutputFormat::Human,
            4,
            false,
            true,
        );
        assert!(resolved.production);
        assert_eq!(resolved.rules.unused_dev_dependencies, Severity::Off);
        assert_eq!(resolved.rules.unused_optional_dependencies, Severity::Off);
        // Other rules should remain at default (Error)
        assert_eq!(resolved.rules.unused_files, Severity::Error);
        assert_eq!(resolved.rules.unused_exports, Severity::Error);
    }

    // ── include-entry-exports config support (issue #249) ──────

    #[test]
    fn include_entry_exports_deserializes_from_camelcase_json() {
        let json = r#"{ "includeEntryExports": true }"#;
        let config: FallowConfig = serde_json::from_str(json).unwrap();
        assert!(config.include_entry_exports);
    }

    #[test]
    fn include_entry_exports_deserializes_from_camelcase_toml() {
        let toml_str = "includeEntryExports = true\n";
        let config: FallowConfig = toml::from_str(toml_str).unwrap();
        assert!(config.include_entry_exports);
    }

    #[test]
    fn include_entry_exports_default_is_false() {
        let config: FallowConfig = serde_json::from_str("{}").unwrap();
        assert!(!config.include_entry_exports);
    }

    #[test]
    fn include_entry_exports_propagates_through_resolve() {
        let config = FallowConfig {
            include_entry_exports: true,
            ..Default::default()
        };
        let resolved = config.resolve(
            PathBuf::from("/tmp/test"),
            OutputFormat::Human,
            1,
            true,
            true,
        );
        assert!(resolved.include_entry_exports);
    }

    // ── config format fallback to TOML for unknown extensions ───

    #[test]
    fn config_format_defaults_to_toml_for_unknown() {
        assert!(matches!(
            ConfigFormat::from_path(Path::new("config.yaml")),
            ConfigFormat::Toml
        ));
        assert!(matches!(
            ConfigFormat::from_path(Path::new("config")),
            ConfigFormat::Toml
        ));
    }

    // ── deep_merge type coercion ─────────────────────────────────

    #[test]
    fn deep_merge_object_over_scalar_replaces() {
        let mut base = serde_json::json!("just a string");
        let overlay = serde_json::json!({"key": "value"});
        deep_merge_json(&mut base, overlay);
        assert_eq!(base, serde_json::json!({"key": "value"}));
    }

    #[test]
    fn deep_merge_scalar_over_object_replaces() {
        let mut base = serde_json::json!({"key": "value"});
        let overlay = serde_json::json!(42);
        deep_merge_json(&mut base, overlay);
        assert_eq!(base, serde_json::json!(42));
    }

    // ── extends with non-string/array extends field ──────────────

    #[test]
    fn extends_non_string_non_array_ignored() {
        let dir = test_dir("extends-numeric");
        std::fs::write(
            dir.path().join(".fallowrc.json"),
            r#"{"extends": 42, "entry": ["src/index.ts"]}"#,
        )
        .unwrap();

        // extends=42 is neither string nor array, so it's treated as no extends
        let config = FallowConfig::load(&dir.path().join(".fallowrc.json")).unwrap();
        assert_eq!(config.entry, vec!["src/index.ts"]);
    }

    // ── extends with multiple bases (later overrides earlier) ────

    #[test]
    fn extends_multiple_bases_later_wins() {
        let dir = test_dir("extends-multi-base");

        std::fs::write(
            dir.path().join("base-a.json"),
            r#"{"rules": {"unused-files": "warn"}}"#,
        )
        .unwrap();
        std::fs::write(
            dir.path().join("base-b.json"),
            r#"{"rules": {"unused-files": "off"}}"#,
        )
        .unwrap();
        std::fs::write(
            dir.path().join(".fallowrc.json"),
            r#"{"extends": ["base-a.json", "base-b.json"]}"#,
        )
        .unwrap();

        let config = FallowConfig::load(&dir.path().join(".fallowrc.json")).unwrap();
        // base-b is later in the array, so its value should win
        assert_eq!(config.rules.unused_files, Severity::Off);
    }

    // ── config with production flag ──────────────────────────────

    #[test]
    fn fallow_config_deserialize_production() {
        let json_str = r#"{"production": true}"#;
        let config: FallowConfig = serde_json::from_str(json_str).unwrap();
        assert!(config.production);
    }

    #[test]
    fn fallow_config_production_defaults_false() {
        let config: FallowConfig = serde_json::from_str("{}").unwrap();
        assert!(!config.production);
    }

    // ── optional dependency names ────────────────────────────────

    #[test]
    fn package_json_optional_dependency_names() {
        let pkg: PackageJson = serde_json::from_str(
            r#"{"optionalDependencies": {"fsevents": "^2", "chokidar": "^3"}}"#,
        )
        .unwrap();
        let opt = pkg.optional_dependency_names();
        assert_eq!(opt.len(), 2);
        assert!(opt.contains(&"fsevents".to_string()));
        assert!(opt.contains(&"chokidar".to_string()));
    }

    #[test]
    fn package_json_optional_deps_empty_when_missing() {
        let pkg: PackageJson = serde_json::from_str(r#"{"name": "test"}"#).unwrap();
        assert!(pkg.optional_dependency_names().is_empty());
    }

    // ── find_config_path ────────────────────────────────────────────

    #[test]
    fn find_config_path_returns_fallowrc_json() {
        let dir = test_dir("find-path-json");
        std::fs::create_dir(dir.path().join(".git")).unwrap();
        std::fs::write(
            dir.path().join(".fallowrc.json"),
            r#"{"entry": ["src/main.ts"]}"#,
        )
        .unwrap();

        let path = FallowConfig::find_config_path(dir.path());
        assert!(path.is_some());
        assert!(path.unwrap().ends_with(".fallowrc.json"));
    }

    #[test]
    fn find_config_path_returns_fallow_toml() {
        let dir = test_dir("find-path-toml");
        std::fs::create_dir(dir.path().join(".git")).unwrap();
        std::fs::write(
            dir.path().join("fallow.toml"),
            "entry = [\"src/main.ts\"]\n",
        )
        .unwrap();

        let path = FallowConfig::find_config_path(dir.path());
        assert!(path.is_some());
        assert!(path.unwrap().ends_with("fallow.toml"));
    }

    #[test]
    fn find_config_path_returns_dot_fallow_toml() {
        let dir = test_dir("find-path-dot-toml");
        std::fs::create_dir(dir.path().join(".git")).unwrap();
        std::fs::write(
            dir.path().join(".fallow.toml"),
            "entry = [\"src/main.ts\"]\n",
        )
        .unwrap();

        let path = FallowConfig::find_config_path(dir.path());
        assert!(path.is_some());
        assert!(path.unwrap().ends_with(".fallow.toml"));
    }

    #[test]
    fn find_config_path_prefers_json_over_toml() {
        let dir = test_dir("find-path-priority");
        std::fs::create_dir(dir.path().join(".git")).unwrap();
        std::fs::write(
            dir.path().join(".fallowrc.json"),
            r#"{"entry": ["json.ts"]}"#,
        )
        .unwrap();
        std::fs::write(dir.path().join("fallow.toml"), "entry = [\"toml.ts\"]\n").unwrap();

        let path = FallowConfig::find_config_path(dir.path());
        assert!(path.unwrap().ends_with(".fallowrc.json"));
    }

    #[test]
    fn find_config_path_none_when_no_config() {
        let dir = test_dir("find-path-none");
        std::fs::create_dir(dir.path().join(".git")).unwrap();

        let path = FallowConfig::find_config_path(dir.path());
        assert!(path.is_none());
    }

    #[test]
    fn find_config_path_walks_past_package_json_in_monorepo() {
        let dir = test_dir("find-path-monorepo");
        std::fs::create_dir(dir.path().join(".git")).unwrap();
        std::fs::write(
            dir.path().join(".fallowrc.json"),
            r#"{"entry": ["src/index.ts"]}"#,
        )
        .unwrap();

        let sub = dir.path().join("packages").join("app");
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::write(sub.join("package.json"), r#"{"name": "@scope/app"}"#).unwrap();

        let path = FallowConfig::find_config_path(&sub).unwrap();
        assert_eq!(path, dir.path().join(".fallowrc.json"));
    }

    // ── TOML extends support ────────────────────────────────────────

    #[test]
    fn extends_toml_base() {
        let dir = test_dir("extends-toml");

        std::fs::write(
            dir.path().join("base.json"),
            r#"{"rules": {"unused-files": "warn"}}"#,
        )
        .unwrap();
        std::fs::write(
            dir.path().join("fallow.toml"),
            "extends = [\"base.json\"]\nentry = [\"src/index.ts\"]\n",
        )
        .unwrap();

        let config = FallowConfig::load(&dir.path().join("fallow.toml")).unwrap();
        assert_eq!(config.rules.unused_files, Severity::Warn);
        assert_eq!(config.entry, vec!["src/index.ts"]);
    }

    // ── deep_merge_json edge cases ──────────────────────────────────

    #[test]
    fn deep_merge_boolean_overlay() {
        let mut base = serde_json::json!(true);
        deep_merge_json(&mut base, serde_json::json!(false));
        assert_eq!(base, serde_json::json!(false));
    }

    #[test]
    fn deep_merge_number_overlay() {
        let mut base = serde_json::json!(42);
        deep_merge_json(&mut base, serde_json::json!(99));
        assert_eq!(base, serde_json::json!(99));
    }

    #[test]
    fn deep_merge_disjoint_objects() {
        let mut base = serde_json::json!({"a": 1});
        let overlay = serde_json::json!({"b": 2});
        deep_merge_json(&mut base, overlay);
        assert_eq!(base, serde_json::json!({"a": 1, "b": 2}));
    }

    // ── MAX_EXTENDS_DEPTH constant ──────────────────────────────────

    #[test]
    fn max_extends_depth_is_reasonable() {
        assert_eq!(MAX_EXTENDS_DEPTH, 10);
    }

    // ── Config names constant ───────────────────────────────────────

    #[test]
    fn config_names_has_four_entries() {
        assert_eq!(CONFIG_NAMES.len(), 4);
        // All names should start with "." or "fallow"
        for name in CONFIG_NAMES {
            assert!(
                name.starts_with('.') || name.starts_with("fallow"),
                "unexpected config name: {name}"
            );
        }
    }

    // ── package.json peer dependency names ───────────────────────────

    #[test]
    fn package_json_peer_dependency_names() {
        let pkg: PackageJson = serde_json::from_str(
            r#"{
            "dependencies": {"react": "^18"},
            "peerDependencies": {"react-dom": "^18", "react-native": "^0.72"}
        }"#,
        )
        .unwrap();
        let all = pkg.all_dependency_names();
        assert!(all.contains(&"react".to_string()));
        assert!(all.contains(&"react-dom".to_string()));
        assert!(all.contains(&"react-native".to_string()));
    }

    // ── package.json scripts field ──────────────────────────────────

    #[test]
    fn package_json_scripts_field() {
        let pkg: PackageJson = serde_json::from_str(
            r#"{
            "scripts": {
                "build": "tsc",
                "test": "vitest",
                "lint": "fallow check"
            }
        }"#,
        )
        .unwrap();
        let scripts = pkg.scripts.unwrap();
        assert_eq!(scripts.len(), 3);
        assert_eq!(scripts.get("build"), Some(&"tsc".to_string()));
        assert_eq!(scripts.get("lint"), Some(&"fallow check".to_string()));
    }

    // ── Extends with TOML-to-TOML chain ─────────────────────────────

    #[test]
    fn extends_toml_chain() {
        let dir = test_dir("extends-toml-chain");

        std::fs::write(
            dir.path().join("base.json"),
            r#"{"entry": ["src/base.ts"]}"#,
        )
        .unwrap();
        std::fs::write(
            dir.path().join("middle.json"),
            r#"{"extends": ["base.json"], "rules": {"unused-files": "off"}}"#,
        )
        .unwrap();
        std::fs::write(
            dir.path().join("fallow.toml"),
            "extends = [\"middle.json\"]\n",
        )
        .unwrap();

        let config = FallowConfig::load(&dir.path().join("fallow.toml")).unwrap();
        assert_eq!(config.entry, vec!["src/base.ts"]);
        assert_eq!(config.rules.unused_files, Severity::Off);
    }

    // ── find_and_load walks up to parent ────────────────────────────

    #[test]
    fn find_and_load_walks_up_directories() {
        let dir = test_dir("find-walk-up");
        let sub = dir.path().join("src").join("deep");
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::write(
            dir.path().join(".fallowrc.json"),
            r#"{"entry": ["src/main.ts"]}"#,
        )
        .unwrap();
        // Create .git in root to stop search there
        std::fs::create_dir(dir.path().join(".git")).unwrap();

        let (config, path) = FallowConfig::find_and_load(&sub).unwrap().unwrap();
        assert_eq!(config.entry, vec!["src/main.ts"]);
        assert!(path.ends_with(".fallowrc.json"));
    }

    // ── JSON schema generation ──────────────────────────────────────

    #[test]
    fn json_schema_contains_entry_field() {
        let schema = FallowConfig::json_schema();
        let obj = schema.as_object().unwrap();
        let props = obj.get("properties").and_then(|v| v.as_object());
        assert!(props.is_some(), "schema should have properties");
        assert!(
            props.unwrap().contains_key("entry"),
            "schema should contain entry property"
        );
    }

    // ── Duplicates config via JSON in FallowConfig ──────────────────

    #[test]
    fn fallow_config_json_duplicates_all_fields() {
        let json = r#"{
            "duplicates": {
                "enabled": true,
                "mode": "semantic",
                "minTokens": 200,
                "minLines": 20,
                "threshold": 10.5,
                "ignore": ["**/*.test.ts"],
                "skipLocal": true,
                "crossLanguage": true,
                "normalization": {
                    "ignoreIdentifiers": true,
                    "ignoreStringValues": false
                }
            }
        }"#;
        let config: FallowConfig = serde_json::from_str(json).unwrap();
        assert!(config.duplicates.enabled);
        assert_eq!(
            config.duplicates.mode,
            crate::config::DetectionMode::Semantic
        );
        assert_eq!(config.duplicates.min_tokens, 200);
        assert_eq!(config.duplicates.min_lines, 20);
        assert!((config.duplicates.threshold - 10.5).abs() < f64::EPSILON);
        assert!(config.duplicates.skip_local);
        assert!(config.duplicates.cross_language);
        assert_eq!(
            config.duplicates.normalization.ignore_identifiers,
            Some(true)
        );
        assert_eq!(
            config.duplicates.normalization.ignore_string_values,
            Some(false)
        );
    }

    // ── URL extends tests ───────────────────────────────────────────

    #[test]
    fn normalize_url_basic() {
        assert_eq!(
            normalize_url_for_dedup("https://example.com/config.json"),
            "https://example.com/config.json"
        );
    }

    #[test]
    fn normalize_url_trailing_slash() {
        assert_eq!(
            normalize_url_for_dedup("https://example.com/config/"),
            "https://example.com/config"
        );
    }

    #[test]
    fn normalize_url_uppercase_scheme_and_host() {
        assert_eq!(
            normalize_url_for_dedup("HTTPS://Example.COM/Config.json"),
            "https://example.com/Config.json"
        );
    }

    #[test]
    fn normalize_url_root_path() {
        assert_eq!(
            normalize_url_for_dedup("https://example.com/"),
            "https://example.com"
        );
        assert_eq!(
            normalize_url_for_dedup("https://example.com"),
            "https://example.com"
        );
    }

    #[test]
    fn normalize_url_preserves_path_case() {
        // Path component casing is significant (server-dependent), only scheme+host lowercase.
        assert_eq!(
            normalize_url_for_dedup("https://GitHub.COM/Org/Repo/Fallow.json"),
            "https://github.com/Org/Repo/Fallow.json"
        );
    }

    #[test]
    fn normalize_url_strips_query_string() {
        assert_eq!(
            normalize_url_for_dedup("https://example.com/config.json?v=1"),
            "https://example.com/config.json"
        );
    }

    #[test]
    fn normalize_url_strips_fragment() {
        assert_eq!(
            normalize_url_for_dedup("https://example.com/config.json#section"),
            "https://example.com/config.json"
        );
    }

    #[test]
    fn normalize_url_strips_query_and_fragment() {
        assert_eq!(
            normalize_url_for_dedup("https://example.com/config.json?v=1#section"),
            "https://example.com/config.json"
        );
    }

    #[test]
    fn normalize_url_default_https_port() {
        assert_eq!(
            normalize_url_for_dedup("https://example.com:443/config.json"),
            "https://example.com/config.json"
        );
        // Non-default port is preserved.
        assert_eq!(
            normalize_url_for_dedup("https://example.com:8443/config.json"),
            "https://example.com:8443/config.json"
        );
    }

    #[test]
    fn extends_http_rejected() {
        let dir = test_dir("http-rejected");
        std::fs::write(
            dir.path().join(".fallowrc.json"),
            r#"{"extends": "http://example.com/config.json"}"#,
        )
        .unwrap();

        let result = FallowConfig::load(&dir.path().join(".fallowrc.json"));
        assert!(result.is_err());
        let err_msg = format!("{}", result.unwrap_err());
        assert!(
            err_msg.contains("https://"),
            "Expected https hint in error, got: {err_msg}"
        );
        assert!(
            err_msg.contains("http://"),
            "Expected http:// mention in error, got: {err_msg}"
        );
    }

    #[test]
    fn extends_url_circular_detection() {
        // Verify that the same URL appearing twice in the visited set is detected.
        let mut visited = FxHashSet::default();
        let url = "https://example.com/config.json";
        let normalized = normalize_url_for_dedup(url);
        visited.insert(normalized.clone());

        // Inserting the same normalized URL should return false.
        assert!(
            !visited.insert(normalized),
            "Same URL should be detected as duplicate"
        );
    }

    #[test]
    fn extends_url_circular_case_insensitive() {
        // URLs differing only in scheme/host casing should be detected as circular.
        let mut visited = FxHashSet::default();
        visited.insert(normalize_url_for_dedup("https://Example.COM/config.json"));

        let normalized = normalize_url_for_dedup("HTTPS://example.com/config.json");
        assert!(
            !visited.insert(normalized),
            "Case-different URLs should normalize to the same key"
        );
    }

    #[test]
    fn extract_extends_array() {
        let mut value = serde_json::json!({
            "extends": ["a.json", "b.json"],
            "entry": ["src/index.ts"]
        });
        let extends = extract_extends(&mut value);
        assert_eq!(extends, vec!["a.json", "b.json"]);
        // extends should be removed from the value.
        assert!(value.get("extends").is_none());
        assert!(value.get("entry").is_some());
    }

    #[test]
    fn extract_extends_string_sugar() {
        let mut value = serde_json::json!({
            "extends": "base.json",
            "entry": ["src/index.ts"]
        });
        let extends = extract_extends(&mut value);
        assert_eq!(extends, vec!["base.json"]);
    }

    #[test]
    fn extract_extends_none() {
        let mut value = serde_json::json!({"entry": ["src/index.ts"]});
        let extends = extract_extends(&mut value);
        assert!(extends.is_empty());
    }

    #[test]
    fn url_timeout_default() {
        // Without the env var set, should return the default.
        let timeout = url_timeout();
        // We can't assert exact value since the env var might be set in the test environment,
        // but we can assert it's a reasonable duration.
        assert!(timeout.as_secs() <= 300, "Timeout should be reasonable");
    }

    #[test]
    fn extends_url_mixed_with_file_and_npm() {
        // Test that a config with a mix of file, npm, and URL extends parses correctly
        // for the non-URL parts, and produces a clear error for the URL part (no server).
        let dir = test_dir("url-mixed");
        std::fs::write(
            dir.path().join("local.json"),
            r#"{"rules": {"unused-files": "warn"}}"#,
        )
        .unwrap();
        std::fs::write(
            dir.path().join(".fallowrc.json"),
            r#"{"extends": ["local.json", "https://unreachable.invalid/config.json"]}"#,
        )
        .unwrap();

        let result = FallowConfig::load(&dir.path().join(".fallowrc.json"));
        assert!(result.is_err());
        let err_msg = format!("{}", result.unwrap_err());
        assert!(
            err_msg.contains("unreachable.invalid"),
            "Expected URL in error message, got: {err_msg}"
        );
    }

    #[test]
    fn extends_https_url_unreachable_errors() {
        let dir = test_dir("url-unreachable");
        std::fs::write(
            dir.path().join(".fallowrc.json"),
            r#"{"extends": "https://unreachable.invalid/config.json"}"#,
        )
        .unwrap();

        let result = FallowConfig::load(&dir.path().join(".fallowrc.json"));
        assert!(result.is_err());
        let err_msg = format!("{}", result.unwrap_err());
        assert!(
            err_msg.contains("unreachable.invalid"),
            "Expected URL in error, got: {err_msg}"
        );
        assert!(
            err_msg.contains("local path or npm:"),
            "Expected remediation hint, got: {err_msg}"
        );
    }
}
