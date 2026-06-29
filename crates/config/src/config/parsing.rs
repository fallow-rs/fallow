use std::path::{Path, PathBuf};
use std::time::Duration;

use fallow_types::path_util::is_absolute_path_any_platform;
use rustc_hash::FxHashSet;

use super::FallowConfig;

/// Supported config file names in priority order.
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

fn is_repo_root(dir: &Path) -> bool {
    dir.join(".git").exists() || dir.join(".hg").exists() || dir.join(".svn").exists()
}

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

fn parse_npm_specifier(specifier: &str) -> (&str, Option<&str>) {
    if specifier.starts_with('@') {
        let mut slashes = 0;
        for (i, ch) in specifier.char_indices() {
            if ch == '/' {
                slashes += 1;
                if slashes == 2 {
                    return (&specifier[..i], Some(&specifier[i + 1..]));
                }
            }
        }
        (specifier, None)
    } else if let Some(slash) = specifier.find('/') {
        (&specifier[..slash], Some(&specifier[slash + 1..]))
    } else {
        (specifier, None)
    }
}

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
        _ => None,
    }
}

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
fn normalize_url_for_dedup(url: &str) -> String {
    let Some((scheme, rest)) = url.split_once("://") else {
        return url.to_string();
    };
    let scheme = scheme.to_ascii_lowercase();

    let (authority, path) = rest.split_once('/').map_or((rest, ""), |(a, p)| (a, p));
    let authority = authority.to_ascii_lowercase();

    let authority = authority.strip_suffix(":443").unwrap_or(&authority);

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
fn url_timeout() -> Duration {
    url_timeout_from(std::env::var("FALLOW_EXTENDS_TIMEOUT_SECS").ok().as_deref())
}

/// Parse a raw `FALLOW_EXTENDS_TIMEOUT_SECS` value into a timeout, falling back
/// to [`DEFAULT_URL_TIMEOUT_SECS`] for absent, zero, or non-numeric input. Pure
/// so the parsing branches stay testable without mutating the process env.
fn url_timeout_from(raw: Option<&str>) -> Duration {
    raw.and_then(|v| v.parse::<u64>().ok().filter(|&n| n > 0))
        .map_or(
            Duration::from_secs(DEFAULT_URL_TIMEOUT_SECS),
            Duration::from_secs,
        )
}

/// Maximum response body size for fetched config files.
const MAX_URL_CONFIG_BYTES: u64 = 1024 * 1024;

/// Fetch a remote JSON config from an HTTPS URL.
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

/// Extract the `extends` array from a parsed JSON config value.
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
            let cwd = std::env::current_dir().map_err(|e| {
                miette::miette!(
                    "Cannot resolve npm: specifier from URL-sourced config: \
                     failed to determine current directory: {e}"
                )
            })?;
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

    record_extends_visit(path, visited)?;

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
    let sealed_dir_canonical = sealed_config_dir(config_dir, sealed)?;
    let mut merged = serde_json::Value::Object(serde_json::Map::new());

    for extend_path_str in &extends {
        let base = resolve_extends_file_entry(&mut ExtendsFileEntryInput {
            path,
            config_dir,
            entry: extend_path_str,
            sealed,
            sealed_dir_canonical: sealed_dir_canonical.as_deref(),
            visited,
            depth,
        })?;
        deep_merge_json(&mut merged, base);
    }

    deep_merge_json(&mut merged, value);
    Ok(merged)
}

fn record_extends_visit(
    path: &Path,
    visited: &mut FxHashSet<String>,
) -> Result<(), miette::Report> {
    let canonical = dunce::canonicalize(path).map_err(|e| {
        miette::miette!(
            "Config file not found or unresolvable: {}: {}",
            path.display(),
            e
        )
    })?;

    if visited.insert(canonical.to_string_lossy().into_owned()) {
        Ok(())
    } else {
        Err(miette::miette!(
            "Circular extends detected: {} was already visited in the extends chain",
            path.display()
        ))
    }
}

fn sealed_config_dir(config_dir: &Path, sealed: bool) -> Result<Option<PathBuf>, miette::Report> {
    if !sealed {
        return Ok(None);
    }
    dunce::canonicalize(config_dir).map(Some).map_err(|e| {
        miette::miette!(
            "Sealed config directory '{}' could not be canonicalized: {e}",
            config_dir.display()
        )
    })
}

struct ExtendsFileEntryInput<'a> {
    path: &'a Path,
    config_dir: &'a Path,
    entry: &'a str,
    sealed: bool,
    sealed_dir_canonical: Option<&'a Path>,
    visited: &'a mut FxHashSet<String>,
    depth: usize,
}

fn resolve_extends_file_entry(
    input: &mut ExtendsFileEntryInput<'_>,
) -> Result<serde_json::Value, miette::Report> {
    if input.entry.starts_with(HTTPS_PREFIX) {
        reject_sealed_remote_extends(input.path, input.entry, input.sealed, "URL")?;
        return resolve_url_extends(input.entry, input.visited, input.depth + 1);
    }
    if input.entry.starts_with(HTTP_PREFIX) {
        return Err(miette::miette!(
            "URL extends must use https://, got http:// URL '{}' (in {}). \
             Change the URL to use https:// instead",
            input.entry,
            input.path.display()
        ));
    }
    if let Some(npm_specifier) = input.entry.strip_prefix(NPM_PREFIX) {
        reject_sealed_remote_extends(input.path, input.entry, input.sealed, "npm")?;
        let npm_path = resolve_npm_package(input.config_dir, npm_specifier, input.path)?;
        return resolve_extends_file(&npm_path, input.visited, input.depth + 1);
    }
    resolve_relative_extends_file(
        input.path,
        input.config_dir,
        input.entry,
        input.sealed_dir_canonical,
        input.visited,
        input.depth,
    )
}

fn reject_sealed_remote_extends(
    path: &Path,
    entry: &str,
    sealed: bool,
    kind: &str,
) -> Result<(), miette::Report> {
    if sealed {
        Err(miette::miette!(
            "'sealed: true' config at {} rejects {} extends '{}'. \
             Sealed configs only allow file-relative extends within \
             the config's directory",
            path.display(),
            kind,
            entry
        ))
    } else {
        Ok(())
    }
}

fn resolve_relative_extends_file(
    path: &Path,
    config_dir: &Path,
    entry: &str,
    sealed_dir_canonical: Option<&Path>,
    visited: &mut FxHashSet<String>,
    depth: usize,
) -> Result<serde_json::Value, miette::Report> {
    if is_absolute_path_any_platform(Path::new(entry)) {
        return Err(miette::miette!(
            "extends paths must be relative, got absolute path: {} (in {})",
            entry,
            path.display()
        ));
    }
    let p = config_dir.join(entry);
    if !p.exists() {
        return Err(miette::miette!(
            "Extended config file not found: {} (referenced from {})",
            p.display(),
            path.display()
        ));
    }
    validate_sealed_relative_extends(path, entry, &p, sealed_dir_canonical)?;
    resolve_extends_file(&p, visited, depth + 1)
}

fn validate_sealed_relative_extends(
    path: &Path,
    entry: &str,
    resolved_path: &Path,
    sealed_dir_canonical: Option<&Path>,
) -> Result<(), miette::Report> {
    let Some(dir_canonical) = sealed_dir_canonical else {
        return Ok(());
    };
    let p_canonical = dunce::canonicalize(resolved_path).map_err(|e| {
        miette::miette!(
            "Sealed config extends path '{}' could not be canonicalized: {e}",
            resolved_path.display()
        )
    })?;
    if p_canonical.starts_with(dir_canonical) {
        Ok(())
    } else {
        Err(miette::miette!(
            "'sealed: true' config at {} rejects extends '{}' which resolves \
             outside the config's directory ({}). Sealed configs only allow \
             extends within the config's directory",
            path.display(),
            entry,
            p_canonical.display()
        ))
    }
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

/// Collect every unknown key under `rules` or `overrides[].rules` in a merged
/// config value (issue #467, phase 1).
///
/// Today `RulesConfig` / `PartialRulesConfig` carry serde aliases but NOT
/// `deny_unknown_fields`, so typos like `unsued-files` are silently dropped and
/// the user's intent is lost. This pass walks the merged value before
/// deserialization and surfaces every unknown key, with a Levenshtein-distance
/// suggestion when the typo is close to a known name.
///
/// Returns the findings so the caller can render them; tests can assert
/// against the list without subscribing to tracing output.
///
/// Phase 2 (a future minor release) flips both structs to
/// `#[serde(deny_unknown_fields)]` and the warning becomes a hard error.
pub(super) fn collect_unknown_rule_keys(
    merged: &serde_json::Value,
) -> Vec<super::rules::UnknownRuleKey> {
    use super::rules::find_unknown_rule_keys;

    let mut findings = Vec::new();

    if let Some(rules) = merged.get("rules") {
        findings.extend(find_unknown_rule_keys(rules, "rules"));
    }

    if let Some(overrides) = merged.get("overrides").and_then(|v| v.as_array()) {
        for (i, entry) in overrides.iter().enumerate() {
            if let Some(rules) = entry.get("rules") {
                let context = format!("overrides[{i}].rules");
                findings.extend(find_unknown_rule_keys(rules, &context));
            }
        }
    }

    findings
}

thread_local! {
    /// Per-thread capture of unknown-rule findings, for the wiring regression
    /// test in this module. Each test installs a fresh capture via
    /// [`capture_unknown_rule_warnings`], runs `FallowConfig::load`, and reads
    /// back the findings. Thread-local so parallel test execution does not
    /// race; bypassed entirely in production code (`UnknownRuleCapture::None`).
    #[cfg(test)]
    static UNKNOWN_RULE_CAPTURE: std::cell::RefCell<Option<Vec<super::rules::UnknownRuleKey>>> =
        const { std::cell::RefCell::new(None) };
}

/// Install a thread-local capture buffer and run `body`. Returns the findings
/// emitted by every `warn_on_unknown_rule_keys` call within `body`'s call tree
/// on the current thread, in order. Test-only.
#[cfg(test)]
pub(super) fn capture_unknown_rule_warnings<F: FnOnce() -> R, R>(
    body: F,
) -> (R, Vec<super::rules::UnknownRuleKey>) {
    UNKNOWN_RULE_CAPTURE.with(|cell| {
        *cell.borrow_mut() = Some(Vec::new());
    });
    let result = body();
    let findings = UNKNOWN_RULE_CAPTURE.with(|cell| cell.borrow_mut().take().unwrap_or_default());
    (result, findings)
}

/// Emit a `tracing::warn!` per finding from [`collect_unknown_rule_keys`].
///
/// `config_path` is the file the merged value originated from; it appears in
/// the warning text AND in the dedupe key so two different config files with
/// the same typo each warn once instead of the second one being silenced.
///
/// Deduplicates within the process: `FallowConfig::load` runs multiple times
/// per analysis (combined mode runs check + dupes + health, each through the
/// same config load path), so without a dedupe the same typo emits 3+ warnings
/// per run.
fn warn_on_unknown_rule_keys(config_path: &Path, merged: &serde_json::Value) {
    use std::sync::{Mutex, OnceLock};

    static WARNED: OnceLock<Mutex<FxHashSet<String>>> = OnceLock::new();
    let warned = WARNED.get_or_init(|| Mutex::new(FxHashSet::default()));

    let path_display = config_path.display().to_string();

    for finding in collect_unknown_rule_keys(merged) {
        let dedupe_key = format!("{path_display}::{}::{}", finding.context, finding.key);
        if let Ok(mut set) = warned.lock()
            && !set.insert(dedupe_key)
        {
            continue;
        }

        #[cfg(test)]
        UNKNOWN_RULE_CAPTURE.with(|cell| {
            if let Some(buf) = cell.borrow_mut().as_mut() {
                buf.push(finding.clone());
            }
        });

        if let Some(suggestion) = finding.suggestion {
            tracing::warn!(
                "unknown rule '{key}' in {context} of {path} (did you mean '{suggestion}'?); \
                 the rule will be ignored. A future release will reject unknown rule names.",
                key = finding.key,
                context = finding.context,
                path = path_display,
            );
        } else {
            tracing::warn!(
                "unknown rule '{key}' in {context} of {path}; the rule will be ignored. \
                 A future release will reject unknown rule names.",
                key = finding.key,
                context = finding.context,
                path = path_display,
            );
        }
    }
}

/// Return the lower-precedence config names from [`CONFIG_NAMES`] that ALSO
/// exist in `dir`, given that `chosen_index` is the index of the first-match
/// (winning) name.
///
/// Only indices after `chosen_index` are scanned: a higher-precedence name
/// cannot coexist undetected, because it would have been the first match.
fn shadowed_config_names(dir: &Path, chosen_index: usize) -> Vec<&'static str> {
    CONFIG_NAMES
        .iter()
        .skip(chosen_index + 1)
        .filter(|name| dir.join(name).exists())
        .copied()
        .collect()
}

/// A captured coexistence warning: `(chosen file name, shadowed file names)`.
/// Test-only; populated by `warn_on_coexisting_configs` under capture.
#[cfg(test)]
type CoexistWarning = (String, Vec<String>);

thread_local! {
    /// Per-thread capture of coexisting-config warnings, for the wiring
    /// regression test in this module. Mirrors [`UNKNOWN_RULE_CAPTURE`]: each
    /// test installs a fresh capture via
    /// [`capture_coexisting_config_warnings`], runs `find_and_load`, and reads
    /// back the `(chosen, shadowed)` pairs. Thread-local so parallel test
    /// execution does not race; bypassed entirely in production code.
    #[cfg(test)]
    static COEXIST_CAPTURE: std::cell::RefCell<Option<Vec<CoexistWarning>>> =
        const { std::cell::RefCell::new(None) };
}

/// Install a thread-local capture buffer and run `body`. Returns every
/// `(chosen, shadowed)` pair emitted by `warn_on_coexisting_configs` within
/// `body`'s call tree on the current thread, in order. Test-only.
#[cfg(test)]
pub(super) fn capture_coexisting_config_warnings<F: FnOnce() -> R, R>(
    body: F,
) -> (R, Vec<CoexistWarning>) {
    COEXIST_CAPTURE.with(|cell| {
        *cell.borrow_mut() = Some(Vec::new());
    });
    let result = body();
    let findings = COEXIST_CAPTURE.with(|cell| cell.borrow_mut().take().unwrap_or_default());
    (result, findings)
}

/// Emit a `tracing::warn!` when `find_and_load` picked `chosen_path` while one
/// or more lower-precedence config files (`shadowed`) coexist in the same
/// directory. Silent precedence is the worst class of config bug: the user
/// sees correct-looking output produced from the wrong source (#458).
///
/// `chosen_path` is the absolute candidate path of the winning config;
/// `shadowed` are the bare names of the lower-precedence files that also exist.
///
/// Deduplicates within the process keyed on the canonical directory, because
/// `find_and_load` runs multiple times per analysis (combined mode loads config
/// for check + dupes + health); without the dedupe the same directory would
/// warn 3+ times per run. Two different directories with coexisting configs
/// warn independently.
fn warn_on_coexisting_configs(chosen_path: &Path, shadowed: &[&str]) {
    use std::sync::{Mutex, OnceLock};

    if shadowed.is_empty() {
        return;
    }

    let chosen_name = chosen_path.file_name().map_or_else(
        || chosen_path.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    );
    let dir = chosen_path.parent().unwrap_or(chosen_path);

    #[cfg(test)]
    COEXIST_CAPTURE.with(|cell| {
        if let Some(buf) = cell.borrow_mut().as_mut() {
            buf.push((
                chosen_name.clone(),
                shadowed.iter().map(|s| (*s).to_owned()).collect(),
            ));
        }
    });

    static WARNED: OnceLock<Mutex<FxHashSet<String>>> = OnceLock::new();
    let warned = WARNED.get_or_init(|| Mutex::new(FxHashSet::default()));
    let dedupe_key = std::fs::canonicalize(dir)
        .unwrap_or_else(|_| dir.to_path_buf())
        .display()
        .to_string();
    if let Ok(mut set) = warned.lock()
        && !set.insert(dedupe_key)
    {
        return;
    }

    tracing::warn!(
        "multiple fallow config files in {dir}: loaded '{chosen}', ignoring '{shadowed}'. \
         fallow uses the first match in precedence order \
         (.fallowrc.json > .fallowrc.jsonc > fallow.toml > .fallow.toml); \
         remove the unused file(s) to silence this warning.",
        dir = dir.display(),
        chosen = chosen_name,
        shadowed = shadowed.join(", "),
    );
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
    /// User-supplied glob patterns (`entry`, `ignorePatterns`,
    /// `dynamicallyLoaded`, `duplicates.ignore`, `health.ignore`,
    /// `health.thresholdOverrides[].files`, `boundaries.zones[].patterns`, `overrides[].files`,
    /// `ignoreExports[].file`, `ignoreCatalogReferences[].consumer`) are
    /// validated against absolute paths, `..` traversal segments, and invalid
    /// glob syntax. Loading fails loud on any rejection so silent no-match
    /// configs surface to the user. See issue #463.
    ///
    /// # Errors
    ///
    /// Returns an error when the config file cannot be read, merged, or
    /// deserialized, or when any user-supplied glob pattern is rejected.
    pub fn load(path: &Path) -> Result<Self, miette::Report> {
        let mut visited = FxHashSet::default();
        let merged = resolve_extends(path, &mut visited, 0)?;

        warn_on_unknown_rule_keys(path, &merged);

        let config: Self = serde_json::from_value(merged).map_err(|e| {
            miette::miette!(
                "Failed to deserialize config from {}: {}",
                path.display(),
                e
            )
        })?;

        config.validate_user_globs().map_err(|errors| {
            let joined = errors
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join("\n  - ");
            miette::miette!("invalid config:\n  - {}", joined)
        })?;
        if !config.security.request_receivers_are_valid() {
            return Err(miette::miette!(
                "invalid config:\n  - security.requestReceivers entries must be non-empty strings"
            ));
        }
        let threshold_override_errors = config.health.threshold_override_errors();
        if !threshold_override_errors.is_empty() {
            return Err(miette::miette!(
                "invalid config:\n  - {}",
                threshold_override_errors.join("\n  - ")
            ));
        }
        if let Some(pattern) = &config.unused_component_props.ignore_pattern
            && let Err(e) = regex::Regex::new(pattern)
        {
            return Err(miette::miette!(
                "invalid config:\n  - unusedComponentProps.ignorePattern is not a valid regex: {e}"
            ));
        }

        Ok(config)
    }

    /// Validate all user-supplied glob patterns and directory paths in this config.
    ///
    /// Accumulates errors from every glob- or path-bearing field so the user
    /// sees ALL offending values in one run rather than fixing them one at a
    /// time.
    ///
    /// Covered filesystem glob fields: `entry`, `ignorePatterns`,
    /// `dynamicallyLoaded`, `duplicates.ignore`, `health.ignore`,
    /// `health.thresholdOverrides[].files`, `overrides[].files`, `ignoreExports[].file`,
    /// `ignoreCatalogReferences[].consumer`, `boundaries.zones[].patterns`,
    /// `boundaries.coverage.allowUnmatched`,
    /// plus every glob-bearing field on inline `framework[]` plugin
    /// definitions (entry points, always-used, config patterns, used-exports
    /// patterns, and `fileExists` detection patterns; the last reaches
    /// `glob::glob` on disk so a `..` segment there is a real path traversal).
    ///
    /// Covered specifier glob fields: `ignoreUnresolvedImports`. These match
    /// raw import strings, so parent-relative specifiers like `../generated/**`
    /// are valid and only glob syntax is checked.
    ///
    /// Covered directory-path fields: `boundaries.zones[].root` and
    /// `boundaries.zones[].autoDiscover`. These are literal paths (not
    /// globs), so only the absolute-path + traversal checks apply.
    ///
    /// # Errors
    ///
    /// Returns a non-empty `Vec` of
    /// [`glob_validation::GlobValidationError`](super::glob_validation::GlobValidationError)
    /// when any field contains a rejected value.
    pub fn validate_user_globs(
        &self,
    ) -> Result<(), Vec<super::glob_validation::GlobValidationError>> {
        let mut errors = Vec::new();

        self.validate_top_level_globs(&mut errors);
        self.validate_ignore_rule_globs(&mut errors);
        self.validate_boundary_globs(&mut errors);

        for plugin in &self.framework {
            if let Err(mut plugin_errors) = plugin.validate_user_globs() {
                errors.append(&mut plugin_errors);
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }

    /// Validate the top-level filesystem and specifier glob fields plus the
    /// per-override and threshold-override file globs.
    fn validate_top_level_globs(
        &self,
        errors: &mut Vec<super::glob_validation::GlobValidationError>,
    ) {
        use super::glob_validation::{validate_user_globs, validate_user_specifier_globs};

        validate_user_globs(&self.entry, "entry", errors);
        validate_user_globs(&self.ignore_patterns, "ignorePatterns", errors);
        validate_user_globs(&self.dynamically_loaded, "dynamicallyLoaded", errors);
        validate_user_specifier_globs(
            &self.ignore_unresolved_imports,
            "ignoreUnresolvedImports",
            errors,
        );
        validate_user_globs(&self.duplicates.ignore, "duplicates.ignore", errors);
        validate_user_globs(&self.health.ignore, "health.ignore", errors);
        for override_entry in &self.health.threshold_overrides {
            validate_user_globs(
                &override_entry.files,
                "health.thresholdOverrides[].files",
                errors,
            );
        }
        for override_entry in &self.overrides {
            validate_user_globs(&override_entry.files, "overrides[].files", errors);
        }
    }

    /// Validate the `ignoreExports` and `ignoreCatalogReferences` rule globs.
    fn validate_ignore_rule_globs(
        &self,
        errors: &mut Vec<super::glob_validation::GlobValidationError>,
    ) {
        use super::glob_validation::compile_user_glob;

        for rule in &self.ignore_exports {
            if let Err(e) = compile_user_glob(&rule.file, "ignoreExports[].file") {
                errors.push(e);
            }
        }

        for rule in &self.ignore_catalog_references {
            if let Some(consumer) = &rule.consumer
                && let Err(e) = compile_user_glob(consumer, "ignoreCatalogReferences[].consumer")
            {
                errors.push(e);
            }
        }
    }

    /// Validate the `boundaries.zones[]` patterns/roots/autoDiscover and the
    /// coverage `allowUnmatched` globs.
    fn validate_boundary_globs(
        &self,
        errors: &mut Vec<super::glob_validation::GlobValidationError>,
    ) {
        use super::glob_validation::{
            validate_user_globs, validate_user_path, validate_user_paths,
        };

        for zone in &self.boundaries.zones {
            validate_user_globs(&zone.patterns, "boundaries.zones[].patterns", errors);
            if let Some(root) = &zone.root
                && let Err(e) = validate_user_path(root, "boundaries.zones[].root")
            {
                errors.push(e);
            }
            validate_user_paths(
                &zone.auto_discover,
                "boundaries.zones[].autoDiscover",
                errors,
            );
        }
        validate_user_globs(
            &self.boundaries.coverage.allow_unmatched,
            "boundaries.coverage.allowUnmatched",
            errors,
        );
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
            for (idx, name) in CONFIG_NAMES.iter().enumerate() {
                let candidate = dir.join(name);
                if candidate.exists() {
                    warn_on_coexisting_configs(&candidate, &shadowed_config_names(dir, idx));
                    match Self::load(&candidate) {
                        Ok(config) => return Ok(Some((config, candidate))),
                        Err(e) => {
                            return Err(format!("Failed to parse {}: {e}", candidate.display()));
                        }
                    }
                }
            }
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

    /// Validate boundary zone references and zone-root-prefix conflicts AFTER
    /// preset and auto-discover expansion.
    ///
    /// Runs the same expand sequence as [`FallowConfig::resolve`] (preset
    /// expansion gated on tsconfig `rootDir`, then `expand_auto_discover`)
    /// before invoking
    /// [`BoundaryConfig::validate_zone_references`](super::boundaries::BoundaryConfig::validate_zone_references)
    /// and
    /// [`BoundaryConfig::validate_root_prefixes`](super::boundaries::BoundaryConfig::validate_root_prefixes),
    /// so Bulletproof-style presets whose authored rule references logical
    /// groups (`features`) still load cleanly.
    ///
    /// Call sites (`runtime_support::load_config_for_analysis` in the CLI,
    /// `core::lib::config_for_project` for LSP and programmatic embedders)
    /// surface every collected error in a single rendered diagnostic, then
    /// exit with code 2. Previously these failures emitted `tracing::error!`
    /// and continued, producing a flood of false-positive boundary violations
    /// at analysis time (#468).
    ///
    /// `root` is the project root used by `expand_auto_discover` to scan for
    /// child directories. Caller is responsible for passing the same root it
    /// later hands to `resolve()`.
    ///
    /// # Errors
    ///
    /// Returns a non-empty `Vec<ZoneValidationError>` aggregating every
    /// offending zone reference and redundant-root-prefix pattern; the empty
    /// case becomes `Ok(())`.
    pub fn validate_resolved_boundaries(
        &self,
        root: &Path,
    ) -> Result<(), Vec<super::boundaries::ZoneValidationError>> {
        use super::boundaries::ZoneValidationError;

        let mut boundaries = self.boundaries.clone();
        if boundaries.preset.is_some() {
            let source_root = crate::workspace::parse_tsconfig_root_dir(root)
                .filter(|r| r != "." && !r.starts_with("..") && !Path::new(r).is_absolute())
                .unwrap_or_else(|| "src".to_owned());
            boundaries.expand(&source_root);
        }
        let _logical_groups = boundaries.expand_auto_discover(root);

        let mut errors: Vec<ZoneValidationError> = boundaries
            .validate_zone_references()
            .into_iter()
            .map(ZoneValidationError::UnknownZoneReference)
            .collect();
        errors.extend(
            boundaries
                .validate_root_prefixes()
                .into_iter()
                .map(ZoneValidationError::RedundantRootPrefix),
        );
        errors.extend(
            boundaries
                .validate_call_rules()
                .into_iter()
                .map(ZoneValidationError::InvalidForbiddenCallee),
        );

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::CacheConfig;
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
    fn fallow_config_deserialize_ignore_unresolved_imports() {
        let toml_str = r#"
ignoreUnresolvedImports = ["@example/icons", "@example/icons/**", "../generated/**"]
"#;
        let config: FallowConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(
            config.ignore_unresolved_imports,
            vec!["@example/icons", "@example/icons/**", "../generated/**"]
        );
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
            None,
        );

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
            None,
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
            None,
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
    fn load_json_config_file_with_health_threshold_override() {
        let dir = test_dir("json-health-threshold-override");
        let config_path = dir.path().join(".fallowrc.json");
        std::fs::write(
            &config_path,
            r#"{
                "health": {
                    "thresholdOverrides": [
                        {
                            "files": ["src/legacy.ts"],
                            "functions": ["legacyFlow"],
                            "maxCyclomatic": 30,
                            "maxCognitive": 25,
                            "maxCrap": 80.5,
                            "reason": "legacy migration"
                        }
                    ]
                }
            }"#,
        )
        .unwrap();

        let config = FallowConfig::load(&config_path).unwrap();
        let override_config = &config.health.threshold_overrides[0];
        assert_eq!(override_config.files, vec!["src/legacy.ts"]);
        assert_eq!(override_config.functions, vec!["legacyFlow"]);
        assert_eq!(override_config.max_cyclomatic, Some(30));
        assert_eq!(override_config.max_cognitive, Some(25));
        assert_eq!(override_config.max_crap, Some(80.5));
        assert_eq!(override_config.reason.as_deref(), Some("legacy migration"));
    }

    #[test]
    fn load_jsonc_config_file() {
        let dir = test_dir("jsonc-config");
        let config_path = dir.path().join(".fallowrc.json");
        std::fs::write(
            &config_path,
            r#"{
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
    fn load_jsonc_config_file_with_health_threshold_override() {
        let dir = test_dir("jsonc-health-threshold-override");
        let config_path = dir.path().join(".fallowrc.jsonc");
        std::fs::write(
            &config_path,
            r#"{
                "health": {
                    // Empty functions means every function in matching files.
                    "thresholdOverrides": [
                        { "files": ["src/legacy.ts"], "maxCognitive": 25 }
                    ]
                }
            }"#,
        )
        .unwrap();

        let config = FallowConfig::load(&config_path).unwrap();
        let override_config = &config.health.threshold_overrides[0];
        assert_eq!(override_config.files, vec!["src/legacy.ts"]);
        assert!(override_config.functions.is_empty());
        assert_eq!(override_config.max_cognitive, Some(25));
    }

    #[test]
    fn load_fallowrc_jsonc_extension() {
        let dir = test_dir("jsonc-extension");
        let config_path = dir.path().join(".fallowrc.jsonc");
        std::fs::write(
            &config_path,
            r#"{
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
    fn json_config_ignore_unresolved_imports_camel_case() {
        let json_str = r#"{"ignoreUnresolvedImports": ["@example/icons", "@example/icons/**"]}"#;
        let config: FallowConfig = serde_json::from_str(json_str).unwrap();
        assert_eq!(
            config.ignore_unresolved_imports,
            vec!["@example/icons", "@example/icons/**"]
        );
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
        assert_eq!(config.rules.unused_files, Severity::Error);
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
        assert_eq!(config.rules.unused_files, Severity::Warn);
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
    fn load_rejects_invalid_boundary_coverage_allow_unmatched_glob() {
        let dir = test_dir("boundary-coverage-invalid-glob");
        std::fs::write(
            dir.path().join(".fallowrc.json"),
            r#"{"boundaries":{"coverage":{"allowUnmatched":["[invalid"]}}}"#,
        )
        .unwrap();

        let result = FallowConfig::load(&dir.path().join(".fallowrc.json"));
        assert!(result.is_err());
        let err_msg = format!("{}", result.unwrap_err());
        assert!(
            err_msg.contains("boundaries.coverage.allowUnmatched"),
            "expected coverage field in error, got: {err_msg}"
        );
    }

    #[test]
    fn sealed_rejects_extends_escaping_directory() {
        let dir = test_dir("sealed-rejects-escape");
        let sub = dir.path().join("packages").join("app");
        std::fs::create_dir_all(&sub).unwrap();

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
        assert_eq!(config.entry, vec!["src/b.ts"]);
    }

    fn create_npm_package(root: &Path, name: &str, config_json: &str) {
        let pkg_dir = root.join("node_modules").join(name);
        std::fs::create_dir_all(&pkg_dir).unwrap();
        std::fs::write(pkg_dir.join(".fallowrc.json"), config_json).unwrap();
    }

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
        assert_eq!(config.rules.unused_files, Severity::Warn);
    }

    #[test]
    fn extends_npm_walk_up_directories() {
        let dir = test_dir("npm-walkup");
        create_npm_package(
            dir.path(),
            "shared-config",
            r#"{"rules": {"unused-files": "warn"}}"#,
        );
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

    #[test]
    fn extends_npm_subpath_traversal_rejected() {
        let dir = test_dir("npm-traversal-sub");
        let pkg_dir = dir.path().join("node_modules/evil-pkg");
        std::fs::create_dir_all(&pkg_dir).unwrap();
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
        assert_eq!(config.rules.unused_exports, Severity::Error);
        assert_eq!(config.rules.unused_types, Severity::Error);
        assert_eq!(config.rules.unused_dependencies, Severity::Error);
        assert_eq!(config.rules.unresolved_imports, Severity::Error);
        assert_eq!(config.rules.unlisted_dependencies, Severity::Error);
        assert_eq!(config.rules.duplicate_exports, Severity::Error);
        assert_eq!(config.rules.circular_dependencies, Severity::Error);
        assert_eq!(config.rules.type_only_dependencies, Severity::Warn);
    }

    #[test]
    fn find_and_load_returns_none_when_no_config() {
        let dir = test_dir("find-none");
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
    fn shadowed_config_names_empty_when_single_config() {
        let dir = test_dir("shadow-single");
        std::fs::write(dir.path().join(".fallowrc.json"), "").unwrap();
        assert!(shadowed_config_names(dir.path(), 0).is_empty());
    }

    #[test]
    fn shadowed_config_names_reports_lower_precedence_toml() {
        let dir = test_dir("shadow-json-toml");
        std::fs::write(dir.path().join(".fallowrc.json"), "").unwrap();
        std::fs::write(dir.path().join("fallow.toml"), "").unwrap();
        assert_eq!(shadowed_config_names(dir.path(), 0), vec!["fallow.toml"]);
    }

    #[test]
    fn shadowed_config_names_reports_jsonc_sibling() {
        let dir = test_dir("shadow-json-jsonc");
        std::fs::write(dir.path().join(".fallowrc.json"), "").unwrap();
        std::fs::write(dir.path().join(".fallowrc.jsonc"), "").unwrap();
        assert_eq!(
            shadowed_config_names(dir.path(), 0),
            vec![".fallowrc.jsonc"]
        );
    }

    #[test]
    fn shadowed_config_names_reports_all_lower_when_four_coexist() {
        let dir = test_dir("shadow-all-four");
        for name in CONFIG_NAMES {
            std::fs::write(dir.path().join(name), "").unwrap();
        }
        assert_eq!(
            shadowed_config_names(dir.path(), 0),
            vec![".fallowrc.jsonc", "fallow.toml", ".fallow.toml"],
        );
    }

    #[test]
    fn shadowed_config_names_scoped_to_indices_after_winner() {
        let dir = test_dir("shadow-toml-dottoml");
        std::fs::write(dir.path().join("fallow.toml"), "").unwrap();
        std::fs::write(dir.path().join(".fallow.toml"), "").unwrap();
        assert_eq!(shadowed_config_names(dir.path(), 2), vec![".fallow.toml"]);
    }

    #[test]
    fn find_and_load_warns_when_configs_coexist() {
        let dir = test_dir("coexist-warn");
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

        let (result, captured) =
            capture_coexisting_config_warnings(|| FallowConfig::find_and_load(dir.path()));

        let (config, path) = result.unwrap().unwrap();
        assert_eq!(config.entry, vec!["from-json.ts"]);
        assert!(path.ends_with(".fallowrc.json"));

        assert_eq!(captured.len(), 1);
        let (chosen, shadowed) = &captured[0];
        assert_eq!(chosen, ".fallowrc.json");
        assert_eq!(shadowed, &vec!["fallow.toml".to_owned()]);
    }

    #[test]
    fn find_and_load_does_not_warn_for_single_config() {
        let dir = test_dir("coexist-none");
        std::fs::create_dir(dir.path().join(".git")).unwrap();
        std::fs::write(
            dir.path().join(".fallowrc.json"),
            r#"{"entry": ["only.ts"]}"#,
        )
        .unwrap();

        let (result, captured) =
            capture_coexisting_config_warnings(|| FallowConfig::find_and_load(dir.path()));
        assert!(result.unwrap().is_some());
        assert!(captured.is_empty());
    }

    #[test]
    fn find_and_load_warns_per_directory_independently() {
        let make = |name: &str| {
            let dir = test_dir(name);
            std::fs::create_dir(dir.path().join(".git")).unwrap();
            std::fs::write(dir.path().join(".fallowrc.json"), r#"{"entry": ["a.ts"]}"#).unwrap();
            std::fs::write(dir.path().join("fallow.toml"), "entry = [\"a.ts\"]\n").unwrap();
            dir
        };
        let first = make("coexist-dir-a");
        let second = make("coexist-dir-b");

        let ((), captured) = capture_coexisting_config_warnings(|| {
            FallowConfig::find_and_load(first.path()).unwrap();
            FallowConfig::find_and_load(second.path()).unwrap();
        });

        assert_eq!(captured.len(), 2);
        assert!(captured.iter().all(|(chosen, shadowed)| {
            chosen == ".fallowrc.json" && shadowed == &vec!["fallow.toml".to_owned()]
        }));
    }

    #[test]
    fn explicit_load_does_not_warn_about_coexisting_configs() {
        let dir = test_dir("coexist-explicit");
        std::fs::write(
            dir.path().join(".fallowrc.json"),
            r#"{"entry": ["chosen.ts"]}"#,
        )
        .unwrap();
        std::fs::write(dir.path().join("fallow.toml"), "entry = [\"other.ts\"]\n").unwrap();

        let chosen = dir.path().join("fallow.toml");
        let (result, captured) = capture_coexisting_config_warnings(|| FallowConfig::load(&chosen));
        assert!(result.is_ok());
        assert!(captured.is_empty());
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
        std::fs::create_dir(dir.path().join(".git")).unwrap();
        let result = FallowConfig::find_and_load(&sub).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn find_and_load_walks_past_package_json_in_monorepo() {
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
        let dir = test_dir("find-git-file");
        std::fs::create_dir(dir.path().join(".git")).unwrap();
        std::fs::write(
            dir.path().join(".fallowrc.json"),
            r#"{"entry": ["src/parent.ts"]}"#,
        )
        .unwrap();

        let submodule = dir.path().join("vendor").join("lib");
        std::fs::create_dir_all(&submodule).unwrap();
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

    #[test]
    fn load_toml_config_file_with_health_threshold_override() {
        let dir = test_dir("toml-health-threshold-override");
        let config_path = dir.path().join("fallow.toml");
        std::fs::write(
            &config_path,
            r#"
[health]
thresholdOverrides = [
  { files = ["src/legacy.ts"], functions = ["legacyFlow"], maxCyclomatic = 30, maxCognitive = 25, maxCrap = 80.5, reason = "legacy migration" }
]
"#,
        )
        .unwrap();

        let config = FallowConfig::load(&config_path).unwrap();
        let override_config = &config.health.threshold_overrides[0];
        assert_eq!(override_config.files, vec!["src/legacy.ts"]);
        assert_eq!(override_config.functions, vec!["legacyFlow"]);
        assert_eq!(override_config.max_cyclomatic, Some(30));
        assert_eq!(override_config.max_cognitive, Some(25));
        assert_eq!(override_config.max_crap, Some(80.5));
        assert_eq!(override_config.reason.as_deref(), Some("legacy migration"));
    }

    #[test]
    fn extends_absolute_path_rejected() {
        let dir = test_dir("extends-absolute");

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

    #[test]
    fn extends_windows_drive_absolute_path_rejected_on_any_host() {
        let dir = test_dir("extends-windows-absolute");

        std::fs::write(
            dir.path().join(".fallowrc.json"),
            r#"{"extends": ["C:\\absolute\\path\\config.json"]}"#,
        )
        .unwrap();

        let result = FallowConfig::load(&dir.path().join(".fallowrc.json"));
        assert!(result.is_err());
        let err_msg = format!("{}", result.unwrap_err());
        assert!(
            err_msg.contains("must be relative"),
            "Expected 'must be relative' error, got: {err_msg}"
        );
    }

    #[cfg(windows)]
    #[test]
    fn extends_posix_rooted_absolute_path_rejected_on_windows() {
        let dir = test_dir("extends-posix-rooted-absolute");

        std::fs::write(
            dir.path().join(".fallowrc.json"),
            r#"{"extends": ["/absolute/path/config.json"]}"#,
        )
        .unwrap();

        let result = FallowConfig::load(&dir.path().join(".fallowrc.json"));
        assert!(result.is_err());
        let err_msg = format!("{}", result.unwrap_err());
        assert!(
            err_msg.contains("must be relative"),
            "Expected 'must be relative' error, got: {err_msg}"
        );
    }

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
            None,
        );
        assert!(resolved.production);
        assert_eq!(resolved.rules.unused_dev_dependencies, Severity::Off);
        assert_eq!(resolved.rules.unused_optional_dependencies, Severity::Off);
        assert_eq!(resolved.rules.unused_files, Severity::Error);
        assert_eq!(resolved.rules.unused_exports, Severity::Error);
    }

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
            auto_imports: false,
            cache: CacheConfig::default(),
            ..Default::default()
        };
        let resolved = config.resolve(
            PathBuf::from("/tmp/test"),
            OutputFormat::Human,
            1,
            true,
            true,
            None,
        );
        assert!(resolved.include_entry_exports);
    }

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

    #[test]
    fn extends_non_string_non_array_ignored() {
        let dir = test_dir("extends-numeric");
        std::fs::write(
            dir.path().join(".fallowrc.json"),
            r#"{"extends": 42, "entry": ["src/index.ts"]}"#,
        )
        .unwrap();

        let config = FallowConfig::load(&dir.path().join(".fallowrc.json")).unwrap();
        assert_eq!(config.entry, vec!["src/index.ts"]);
    }

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
        assert_eq!(config.rules.unused_files, Severity::Off);
    }

    #[test]
    fn load_rejects_empty_security_request_receivers() {
        let dir = test_dir("empty-security-request-receivers");
        std::fs::write(
            dir.path().join(".fallowrc.json"),
            r#"{"security": {"requestReceivers": ["req", "  "]}}"#,
        )
        .unwrap();

        let result = FallowConfig::load(&dir.path().join(".fallowrc.json"));
        let err = result.expect_err("empty receiver should be rejected");
        assert!(
            err.to_string().contains("security.requestReceivers"),
            "error should name security.requestReceivers: {err}"
        );
    }

    #[test]
    fn resolve_normalizes_security_request_receivers() {
        let dir = test_dir("normalize-security-request-receivers");
        std::fs::write(
            dir.path().join(".fallowrc.json"),
            r#"{"security": {"requestReceivers": [" HttpReq ", "httpreq", "R"]}}"#,
        )
        .unwrap();

        let config = FallowConfig::load(&dir.path().join(".fallowrc.json"))
            .unwrap()
            .resolve(
                dir.path().to_path_buf(),
                OutputFormat::Human,
                1,
                true,
                true,
                None,
            );
        assert_eq!(
            config.security.request_receivers,
            vec!["httpreq".to_string(), "r".to_string()]
        );
    }

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

    #[test]
    fn max_extends_depth_is_reasonable() {
        assert_eq!(MAX_EXTENDS_DEPTH, 10);
    }

    #[test]
    fn config_names_has_four_entries() {
        assert_eq!(CONFIG_NAMES.len(), 4);
        for name in CONFIG_NAMES {
            assert!(
                name.starts_with('.') || name.starts_with("fallow"),
                "unexpected config name: {name}"
            );
        }
    }

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
        std::fs::create_dir(dir.path().join(".git")).unwrap();

        let (config, path) = FallowConfig::find_and_load(&sub).unwrap().unwrap();
        assert_eq!(config.entry, vec!["src/main.ts"]);
        assert!(path.ends_with(".fallowrc.json"));
    }

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
        let mut visited = FxHashSet::default();
        let url = "https://example.com/config.json";
        let normalized = normalize_url_for_dedup(url);
        visited.insert(normalized.clone());

        assert!(
            !visited.insert(normalized),
            "Same URL should be detected as duplicate"
        );
    }

    #[test]
    fn extends_url_circular_case_insensitive() {
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
        let timeout = url_timeout();
        assert!(timeout.as_secs() <= 300, "Timeout should be reasonable");
    }

    #[test]
    fn extends_url_mixed_with_file_and_npm() {
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

    #[test]
    fn collect_unknown_rule_keys_flags_top_level_typo() {
        let merged = serde_json::json!({
            "rules": {
                "unsued-files": "warn",
                "unused-exports": "off"
            }
        });
        let findings = collect_unknown_rule_keys(&merged);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].context, "rules");
        assert_eq!(findings[0].key, "unsued-files");
        assert_eq!(findings[0].suggestion, Some("unused-files"));
    }

    #[test]
    fn collect_unknown_rule_keys_flags_overrides_typo() {
        let merged = serde_json::json!({
            "overrides": [
                {
                    "files": ["src/**/*.ts"],
                    "rules": {
                        "unsued-files": "warn"
                    }
                },
                {
                    "files": ["tests/**/*.ts"],
                    "rules": {
                        "circular-dependnecy": "off"
                    }
                }
            ]
        });
        let findings = collect_unknown_rule_keys(&merged);
        assert_eq!(findings.len(), 2);
        assert_eq!(findings[0].context, "overrides[0].rules");
        assert_eq!(findings[1].context, "overrides[1].rules");
        assert_eq!(findings[1].suggestion, Some("circular-dependency"));
    }

    #[test]
    fn collect_unknown_rule_keys_empty_for_valid_config() {
        let merged = serde_json::json!({
            "rules": {
                "unused-files": "warn",
                "unused-file": "off",
                "circular-dependency": "off",
                "boundary-violations": "warn"
            },
            "overrides": [
                {
                    "files": ["src/**"],
                    "rules": {
                        "unused-exports": "warn"
                    }
                }
            ]
        });
        let findings = collect_unknown_rule_keys(&merged);
        assert!(
            findings.is_empty(),
            "valid rule names and aliases must not be flagged: {findings:?}"
        );
    }

    #[test]
    fn collect_unknown_rule_keys_ignores_missing_rules_section() {
        let merged = serde_json::json!({
            "entry": ["src/main.ts"]
        });
        let findings = collect_unknown_rule_keys(&merged);
        assert!(findings.is_empty());
    }

    #[test]
    fn load_wires_warn_on_unknown_rule_keys_into_load_path() {
        let dir = test_dir("wiring");
        let path = dir.path().join(".fallowrc.json");
        let typo = format!(
            "wiring-probe-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_nanos())
        );
        std::fs::write(&path, format!(r#"{{"rules": {{"{typo}": "warn"}}}}"#)).unwrap();

        let (config_res, captured) = capture_unknown_rule_warnings(|| FallowConfig::load(&path));

        assert!(
            config_res.is_ok(),
            "load should succeed in phase 1: {:?}",
            config_res.err()
        );
        assert_eq!(
            captured.len(),
            1,
            "FallowConfig::load must invoke warn_on_unknown_rule_keys exactly once for one new unknown key, got: {captured:?}"
        );
        assert_eq!(captured[0].key, typo);
        assert_eq!(captured[0].context, "rules");
    }

    #[test]
    fn load_with_misspelled_rule_succeeds_and_ignores_typo() {
        let dir = test_dir("misspelled-rule");
        std::fs::write(
            dir.path().join(".fallowrc.json"),
            r#"{"rules": {"unsued-files": "warn"}}"#,
        )
        .unwrap();

        let config = FallowConfig::load(&dir.path().join(".fallowrc.json"))
            .expect("load should succeed in phase 1");

        assert_eq!(config.rules.unused_files, Severity::Error);
    }

    #[test]
    fn validate_resolved_boundaries_passes_on_valid_config() {
        let dir = test_dir("boundaries-valid");
        let config = FallowConfig {
            boundaries: crate::BoundaryConfig {
                coverage: crate::BoundaryCoverageConfig::default(),
                calls: crate::BoundaryCallsConfig::default(),
                preset: None,
                zones: vec![
                    crate::BoundaryZone {
                        name: "ui".to_string(),
                        patterns: vec!["src/components/**".to_string()],
                        auto_discover: vec![],
                        root: None,
                    },
                    crate::BoundaryZone {
                        name: "db".to_string(),
                        patterns: vec!["src/db/**".to_string()],
                        auto_discover: vec![],
                        root: None,
                    },
                ],
                rules: vec![crate::BoundaryRule {
                    from: "ui".to_string(),
                    allow: vec!["db".to_string()],
                    allow_type_only: vec![],
                }],
            },
            ..FallowConfig::default()
        };
        config
            .validate_resolved_boundaries(dir.path())
            .expect("valid config should pass");
    }

    #[test]
    fn validate_resolved_boundaries_aggregates_unknown_zone_refs() {
        let dir = test_dir("boundaries-unknown-zones");
        let config = FallowConfig {
            boundaries: crate::BoundaryConfig {
                coverage: crate::BoundaryCoverageConfig::default(),
                calls: crate::BoundaryCallsConfig::default(),
                preset: None,
                zones: vec![crate::BoundaryZone {
                    name: "ui".to_string(),
                    patterns: vec!["src/ui/**".to_string()],
                    auto_discover: vec![],
                    root: None,
                }],
                rules: vec![
                    crate::BoundaryRule {
                        from: "typo-from".to_string(),
                        allow: vec!["typo-allow".to_string()],
                        allow_type_only: vec!["typo-type-only".to_string()],
                    },
                    crate::BoundaryRule {
                        from: "ui".to_string(),
                        allow: vec!["another-typo".to_string()],
                        allow_type_only: vec![],
                    },
                ],
            },
            ..FallowConfig::default()
        };

        let errors = config
            .validate_resolved_boundaries(dir.path())
            .expect_err("invalid zone refs should fail");

        assert_eq!(errors.len(), 4, "got: {errors:?}");

        let rendered: Vec<String> = errors.iter().map(ToString::to_string).collect();
        assert!(
            rendered
                .iter()
                .any(|m| m.contains("typo-from") && m.contains("rules[0]") && m.contains("from"))
        );
        assert!(
            rendered
                .iter()
                .any(|m| m.contains("typo-allow") && m.contains("rules[0]") && m.contains("allow"))
        );
        assert!(rendered.iter().any(|m| m.contains("typo-type-only")
            && m.contains("rules[0]")
            && m.contains("allowTypeOnly")));
        assert!(
            rendered.iter().any(|m| m.contains("another-typo")
                && m.contains("rules[1]")
                && m.contains("allow"))
        );
    }

    #[test]
    fn validate_resolved_boundaries_flags_redundant_root_prefix() {
        let dir = test_dir("boundaries-redundant-prefix");
        let config = FallowConfig {
            boundaries: crate::BoundaryConfig {
                coverage: crate::BoundaryCoverageConfig::default(),
                calls: crate::BoundaryCallsConfig::default(),
                preset: None,
                zones: vec![crate::BoundaryZone {
                    name: "ui".to_string(),
                    patterns: vec!["packages/app/src/**".to_string()],
                    auto_discover: vec![],
                    root: Some("packages/app/".to_string()),
                }],
                rules: vec![],
            },
            ..FallowConfig::default()
        };

        let errors = config
            .validate_resolved_boundaries(dir.path())
            .expect_err("redundant root prefix should fail");
        assert_eq!(errors.len(), 1, "got: {errors:?}");
        let rendered = errors[0].to_string();
        assert!(rendered.contains("FALLOW-BOUNDARY-ROOT-REDUNDANT-PREFIX"));
        assert!(rendered.contains("zone 'ui'"));
    }

    #[test]
    fn validate_resolved_boundaries_aggregates_unknown_zones_and_root_prefixes() {
        let dir = test_dir("boundaries-mixed-errors");
        let config = FallowConfig {
            boundaries: crate::BoundaryConfig {
                coverage: crate::BoundaryCoverageConfig::default(),
                calls: crate::BoundaryCallsConfig::default(),
                preset: None,
                zones: vec![crate::BoundaryZone {
                    name: "ui".to_string(),
                    patterns: vec!["packages/app/src/**".to_string()],
                    auto_discover: vec![],
                    root: Some("packages/app/".to_string()),
                }],
                rules: vec![crate::BoundaryRule {
                    from: "ui".to_string(),
                    allow: vec!["typo-zone".to_string()],
                    allow_type_only: vec![],
                }],
            },
            ..FallowConfig::default()
        };
        let errors = config
            .validate_resolved_boundaries(dir.path())
            .expect_err("mixed errors should fail");
        assert_eq!(errors.len(), 2, "got: {errors:?}");
        let rendered: Vec<String> = errors.iter().map(ToString::to_string).collect();
        assert!(
            rendered
                .iter()
                .any(|m| m.contains("typo-zone") && m.contains("rules[0]"))
        );
        assert!(
            rendered
                .iter()
                .any(|m| m.contains("FALLOW-BOUNDARY-ROOT-REDUNDANT-PREFIX"))
        );
    }

    #[test]
    fn validate_resolved_boundaries_passes_on_bulletproof_preset() {
        let dir = test_dir("boundaries-bulletproof");
        std::fs::create_dir_all(dir.path().join("src/features/auth")).unwrap();
        let config = FallowConfig {
            boundaries: crate::BoundaryConfig {
                coverage: crate::BoundaryCoverageConfig::default(),
                calls: crate::BoundaryCallsConfig::default(),
                preset: Some(crate::BoundaryPreset::Bulletproof),
                zones: vec![],
                rules: vec![],
            },
            ..FallowConfig::default()
        };
        config
            .validate_resolved_boundaries(dir.path())
            .expect("Bulletproof with discoverable features should pass");
    }

    // ------------------------------------------------------------------
    // parse_config_to_value: BOM stripping, TOML parse error, JSON parse error
    // ------------------------------------------------------------------

    #[test]
    #[cfg_attr(miri, ignore)]
    fn parse_config_to_value_strips_utf8_bom() {
        let dir = test_dir("parse-bom");
        let path = dir.path().join("fallow.toml");
        // Write TOML with a UTF-8 BOM prefix
        let content_with_bom = "\u{FEFF}entry = [\"src/main.ts\"]\n";
        std::fs::write(&path, content_with_bom).unwrap();

        let value = parse_config_to_value(&path).unwrap();
        assert!(
            value.get("entry").is_some(),
            "BOM should be stripped before TOML parsing"
        );
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn parse_config_to_value_toml_parse_error() {
        let dir = test_dir("parse-toml-error");
        let path = dir.path().join("fallow.toml");
        std::fs::write(&path, "entry = [unquoted\n").unwrap();

        let result = parse_config_to_value(&path);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("Failed to parse config file"),
            "error should mention parse failure: {err}"
        );
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn parse_config_to_value_json_parse_error() {
        let dir = test_dir("parse-json-error");
        let path = dir.path().join(".fallowrc.json");
        std::fs::write(&path, "{ this is not json }").unwrap();

        let result = parse_config_to_value(&path);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("Failed to parse config file"),
            "error should mention parse failure: {err}"
        );
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn parse_config_to_value_missing_file_error() {
        let dir = test_dir("parse-missing");
        let path = dir.path().join("nonexistent.toml");

        let result = parse_config_to_value(&path);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("Failed to read config file"),
            "error should mention read failure: {err}"
        );
    }

    // ------------------------------------------------------------------
    // is_repo_root: svn boundary
    // ------------------------------------------------------------------

    #[test]
    #[cfg_attr(miri, ignore)]
    fn find_and_load_stops_at_svn_dir() {
        let dir = test_dir("find-svn-stop");
        let sub = dir.path().join("sub");
        std::fs::create_dir(&sub).unwrap();
        std::fs::create_dir(dir.path().join(".svn")).unwrap();

        let result = FallowConfig::find_and_load(&sub).unwrap();
        assert!(result.is_none(), "svn boundary should stop config walk");
    }

    // ------------------------------------------------------------------
    // validate_npm_package_name: dot-segment in the package name
    // (path traversal but using a single dot)
    // ------------------------------------------------------------------

    #[test]
    #[cfg_attr(miri, ignore)]
    fn extends_npm_single_dot_package_name_rejected() {
        let dir = test_dir("npm-dot-name");
        std::fs::write(
            dir.path().join(".fallowrc.json"),
            r#"{"extends": "npm:./relative"}"#,
        )
        .unwrap();

        let result = FallowConfig::load(&dir.path().join(".fallowrc.json"));
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("path traversal"),
            "single-dot component should be rejected as path traversal: {err}"
        );
    }

    // ------------------------------------------------------------------
    // find_config_in_npm_package: main field points to nonexistent file,
    // falls through to config-name scan
    // ------------------------------------------------------------------

    #[test]
    #[cfg_attr(miri, ignore)]
    fn extends_npm_main_points_to_nonexistent_falls_through_to_config_name() {
        let dir = test_dir("npm-main-missing");
        let pkg_dir = dir.path().join("node_modules/my-config");
        std::fs::create_dir_all(&pkg_dir).unwrap();
        // package.json with main pointing at a file that does not exist
        std::fs::write(
            pkg_dir.join("package.json"),
            r#"{"name": "my-config", "main": "./missing.json"}"#,
        )
        .unwrap();
        // But a recognized config name is present for the fallback scan
        std::fs::write(
            pkg_dir.join(".fallowrc.json"),
            r#"{"rules": {"unused-files": "warn"}}"#,
        )
        .unwrap();

        std::fs::write(
            dir.path().join(".fallowrc.json"),
            r#"{"extends": "npm:my-config"}"#,
        )
        .unwrap();

        let config = FallowConfig::load(&dir.path().join(".fallowrc.json")).unwrap();
        assert_eq!(config.rules.unused_files, Severity::Warn);
    }

    // ------------------------------------------------------------------
    // find_config_in_npm_package: exports present but exports-pointed file
    // does not exist, falls through to main then config name
    // ------------------------------------------------------------------

    #[test]
    #[cfg_attr(miri, ignore)]
    fn extends_npm_exports_nonexistent_falls_through_to_main() {
        let dir = test_dir("npm-exports-missing-file");
        let pkg_dir = dir.path().join("node_modules/cfg-pkg");
        std::fs::create_dir_all(&pkg_dir).unwrap();
        // exports points to a file that does not exist; main is valid
        std::fs::write(
            pkg_dir.join("package.json"),
            r#"{"name": "cfg-pkg", "exports": "./missing-exports.json", "main": "./real.json"}"#,
        )
        .unwrap();
        std::fs::write(
            pkg_dir.join("real.json"),
            r#"{"rules": {"unused-types": "off"}}"#,
        )
        .unwrap();

        std::fs::write(
            dir.path().join(".fallowrc.json"),
            r#"{"extends": "npm:cfg-pkg"}"#,
        )
        .unwrap();

        let config = FallowConfig::load(&dir.path().join(".fallowrc.json")).unwrap();
        assert_eq!(config.rules.unused_types, Severity::Off);
    }

    // ------------------------------------------------------------------
    // normalize_url_for_dedup: URL with no "://" scheme falls back to raw
    // ------------------------------------------------------------------

    #[test]
    fn normalize_url_no_scheme_returns_raw() {
        // A string without "://" must come back unchanged
        assert_eq!(normalize_url_for_dedup("not-a-url"), "not-a-url");
        assert_eq!(normalize_url_for_dedup("/absolute/path"), "/absolute/path");
    }

    // ------------------------------------------------------------------
    // normalize_url_for_dedup: query before fragment (fragment then query)
    // ------------------------------------------------------------------

    #[test]
    fn normalize_url_fragment_only_stripped() {
        // Fragment-only URL (no query)
        assert_eq!(
            normalize_url_for_dedup("https://example.com/file.json#anchor"),
            "https://example.com/file.json"
        );
    }

    // ------------------------------------------------------------------
    // url_timeout: env var override
    // ------------------------------------------------------------------

    // These exercise the pure `url_timeout_from` parser rather than mutating the
    // process-global env var, so they stay deterministic under parallel test
    // execution (an env-mutating version raced and failed on Windows CI).
    #[test]
    fn url_timeout_uses_env_var_when_set() {
        assert_eq!(url_timeout_from(Some("15")).as_secs(), 15);
    }

    #[test]
    fn url_timeout_zero_falls_back_to_default() {
        assert_eq!(
            url_timeout_from(Some("0")),
            Duration::from_secs(DEFAULT_URL_TIMEOUT_SECS),
            "zero should fall back to the hardcoded default"
        );
    }

    #[test]
    fn url_timeout_non_numeric_falls_back_to_default() {
        assert_eq!(
            url_timeout_from(Some("not-a-number")),
            Duration::from_secs(DEFAULT_URL_TIMEOUT_SECS),
            "non-numeric value should fall back to the hardcoded default"
        );
    }

    #[test]
    fn url_timeout_absent_uses_default() {
        assert_eq!(
            url_timeout_from(None),
            Duration::from_secs(DEFAULT_URL_TIMEOUT_SECS)
        );
    }

    // ------------------------------------------------------------------
    // resolve_url_extends: depth limit reached
    // ------------------------------------------------------------------

    #[test]
    fn resolve_url_extends_depth_limit_error() {
        let mut visited = FxHashSet::default();
        let result = resolve_url_extends(
            "https://example.invalid/config.json",
            &mut visited,
            MAX_EXTENDS_DEPTH, // at the limit
        );
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("too deep"),
            "error should mention depth limit: {err}"
        );
    }

    // ------------------------------------------------------------------
    // resolve_extends_file: depth limit reached
    // ------------------------------------------------------------------

    #[test]
    #[cfg_attr(miri, ignore)]
    fn resolve_extends_file_depth_limit_error() {
        let dir = test_dir("extends-file-depth");
        let path = dir.path().join(".fallowrc.json");
        std::fs::write(&path, r#"{"entry": []}"#).unwrap();

        let mut visited = FxHashSet::default();
        let result = resolve_extends(&path, &mut visited, MAX_EXTENDS_DEPTH);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("too deep"),
            "error should mention depth limit: {err}"
        );
    }

    // ------------------------------------------------------------------
    // resolve_extends_file_entry: http:// in file-sourced extends
    // ------------------------------------------------------------------

    #[test]
    #[cfg_attr(miri, ignore)]
    fn extends_http_url_in_file_extends_rejected() {
        let dir = test_dir("file-extends-http");
        std::fs::write(
            dir.path().join(".fallowrc.json"),
            r#"{"extends": ["http://example.com/config.json"]}"#,
        )
        .unwrap();

        let result = FallowConfig::load(&dir.path().join(".fallowrc.json"));
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("https://"),
            "error should suggest https: {err}"
        );
    }

    // ------------------------------------------------------------------
    // sealed_config_dir: when sealed = true canonicalization runs
    // ------------------------------------------------------------------

    #[test]
    #[cfg_attr(miri, ignore)]
    fn sealed_config_dir_returns_some_when_sealed() {
        let dir = test_dir("sealed-dir");
        let result = sealed_config_dir(dir.path(), true);
        assert!(result.is_ok());
        assert!(
            result.unwrap().is_some(),
            "sealed=true must return Some(canonicalized path)"
        );
    }

    #[test]
    fn sealed_config_dir_returns_none_when_not_sealed() {
        let result = sealed_config_dir(Path::new("/nonexistent/path"), false);
        assert!(result.is_ok());
        assert!(result.unwrap().is_none(), "sealed=false must return None");
    }

    // ------------------------------------------------------------------
    // collect_unknown_rule_keys: overrides entry without a rules key
    // (the inner `if let Some(rules)` branch is not taken)
    // ------------------------------------------------------------------

    #[test]
    fn collect_unknown_rule_keys_override_without_rules_key() {
        let merged = serde_json::json!({
            "overrides": [
                {
                    "files": ["src/**/*.ts"]
                    // no "rules" key here
                },
                {
                    "files": ["tests/**"],
                    "rules": {
                        "unsued-exports": "off"
                    }
                }
            ]
        });
        let findings = collect_unknown_rule_keys(&merged);
        assert_eq!(
            findings.len(),
            1,
            "only the entry with rules should produce a finding"
        );
        assert_eq!(findings[0].context, "overrides[1].rules");
    }

    // ------------------------------------------------------------------
    // FallowConfig::load: deserialization failure
    // ------------------------------------------------------------------

    #[test]
    #[cfg_attr(miri, ignore)]
    fn load_fails_on_deserialization_error() {
        let dir = test_dir("deser-error");
        let path = dir.path().join(".fallowrc.json");
        // Valid JSON but contains a field value with the wrong type for the schema
        std::fs::write(&path, r#"{"entry": "not-an-array"}"#).unwrap();

        let result = FallowConfig::load(&path);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("Failed to deserialize"),
            "error should mention deserialization: {err}"
        );
    }

    // ------------------------------------------------------------------
    // FallowConfig::load: threshold override validation failure
    // (covers lines 921-927)
    // ------------------------------------------------------------------

    #[test]
    #[cfg_attr(miri, ignore)]
    fn load_rejects_threshold_override_with_empty_files() {
        let dir = test_dir("threshold-empty-files");
        let path = dir.path().join(".fallowrc.json");
        std::fs::write(
            &path,
            r#"{
                "health": {
                    "thresholdOverrides": [
                        {"files": [], "maxCyclomatic": 30}
                    ]
                }
            }"#,
        )
        .unwrap();

        let result = FallowConfig::load(&path);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("thresholdOverrides"),
            "error should mention thresholdOverrides: {err}"
        );
        assert!(
            err.contains("files"),
            "error should name the files field: {err}"
        );
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn load_rejects_threshold_override_with_no_threshold_set() {
        let dir = test_dir("threshold-no-threshold");
        let path = dir.path().join(".fallowrc.json");
        std::fs::write(
            &path,
            r#"{
                "health": {
                    "thresholdOverrides": [
                        {"files": ["src/legacy.ts"]}
                    ]
                }
            }"#,
        )
        .unwrap();

        let result = FallowConfig::load(&path);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("maxCyclomatic")
                || err.contains("maxCognitive")
                || err.contains("maxCrap"),
            "error should name at least one threshold field: {err}"
        );
    }

    // ------------------------------------------------------------------
    // validate_ignore_rule_globs: ignoreCatalogReferences consumer glob
    // validation (covers lines 1026-1032)
    // ------------------------------------------------------------------

    #[test]
    #[cfg_attr(miri, ignore)]
    fn load_rejects_invalid_ignore_catalog_references_consumer_glob() {
        let dir = test_dir("invalid-catalog-consumer-glob");
        let path = dir.path().join(".fallowrc.json");
        std::fs::write(
            &path,
            r#"{
                "ignoreCatalogReferences": [
                    {"package": "react", "consumer": "[invalid-glob"}
                ]
            }"#,
        )
        .unwrap();

        let result = FallowConfig::load(&path);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("ignoreCatalogReferences"),
            "error should mention the field: {err}"
        );
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn load_accepts_ignore_catalog_references_without_consumer() {
        let dir = test_dir("catalog-ref-no-consumer");
        let path = dir.path().join(".fallowrc.json");
        std::fs::write(
            &path,
            r#"{"ignoreCatalogReferences": [{"package": "react"}]}"#,
        )
        .unwrap();

        let config = FallowConfig::load(&path).unwrap();
        assert_eq!(config.ignore_catalog_references.len(), 1);
        assert!(config.ignore_catalog_references[0].consumer.is_none());
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn load_accepts_unused_component_props_ignore_pattern() {
        let dir = test_dir("unused-component-props-ignore-pattern");
        let path = dir.path().join(".fallowrc.json");
        std::fs::write(
            &path,
            r#"{"unusedComponentProps": {"ignorePattern": "^_"}}"#,
        )
        .unwrap();

        let config = FallowConfig::load(&path).unwrap();
        assert_eq!(
            config.unused_component_props.ignore_pattern.as_deref(),
            Some("^_")
        );
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn load_rejects_invalid_unused_component_props_ignore_pattern() {
        let dir = test_dir("unused-component-props-bad-regex");
        let path = dir.path().join(".fallowrc.json");
        // `[` opens an unterminated character class: invalid regex.
        std::fs::write(&path, r#"{"unusedComponentProps": {"ignorePattern": "["}}"#).unwrap();

        let result = FallowConfig::load(&path);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("unusedComponentProps.ignorePattern"),
            "error should mention the field: {err}"
        );
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn load_rejects_unknown_unused_component_props_field() {
        let dir = test_dir("unused-component-props-unknown-field");
        let path = dir.path().join(".fallowrc.json");
        std::fs::write(
            &path,
            r#"{"unusedComponentProps": {"ignorePatterns": "^_"}}"#,
        )
        .unwrap();

        // `deny_unknown_fields` rejects the plural typo.
        assert!(FallowConfig::load(&path).is_err());
    }

    // ------------------------------------------------------------------
    // validate_resolved_boundaries: tsconfig rootDir filtering
    // (covers lines 1158-1160 - rootDir value is ".", starts with "..", or
    // is absolute; all should fall back to "src")
    // ------------------------------------------------------------------

    #[test]
    #[cfg_attr(miri, ignore)]
    fn validate_resolved_boundaries_with_preset_uses_src_fallback_when_no_tsconfig() {
        // No tsconfig.json present; parse_tsconfig_root_dir returns None,
        // unwrap_or_else supplies "src". This exercises the filter + fallback branch.
        let dir = test_dir("boundaries-preset-no-tsconfig");
        std::fs::create_dir_all(dir.path().join("src/features/auth")).unwrap();
        let config = FallowConfig {
            boundaries: crate::BoundaryConfig {
                coverage: crate::BoundaryCoverageConfig::default(),
                calls: crate::BoundaryCallsConfig::default(),
                preset: Some(crate::BoundaryPreset::Bulletproof),
                zones: vec![],
                rules: vec![],
            },
            ..FallowConfig::default()
        };
        // Should not panic; no zone-ref errors expected since preset adds zones
        let _ = config.validate_resolved_boundaries(dir.path());
    }

    // ------------------------------------------------------------------
    // validate_user_globs: framework plugin invalid glob triggers error path
    // (covers lines 970-974)
    // ------------------------------------------------------------------

    #[test]
    fn validate_user_globs_framework_plugin_invalid_entry_glob() {
        use crate::ExternalPluginDef;
        use crate::external_plugin::EntryPointRole;
        let config = FallowConfig {
            framework: vec![ExternalPluginDef {
                schema: None,
                name: "test-plugin".to_owned(),
                detection: None,
                enablers: vec![],
                entry_points: vec!["[invalid-glob".to_owned()],
                entry_point_role: EntryPointRole::Support,
                config_patterns: vec![],
                always_used: vec![],
                tooling_dependencies: vec![],
                used_exports: vec![],
                used_class_members: vec![],
            }],
            ..FallowConfig::default()
        };

        let result = config.validate_user_globs();
        assert!(
            result.is_err(),
            "invalid entry_points glob should fail validation"
        );
        let errors = result.unwrap_err();
        assert!(!errors.is_empty());
    }

    // ------------------------------------------------------------------
    // shadowed_config_names: no lower-precedence names after the last index
    // ------------------------------------------------------------------

    #[test]
    #[cfg_attr(miri, ignore)]
    fn shadowed_config_names_empty_when_last_config_wins() {
        let dir = test_dir("shadow-last");
        std::fs::write(dir.path().join(".fallow.toml"), "").unwrap();
        // chosen_index = 3 (last), so skip+1 = 4, nothing to check
        assert!(shadowed_config_names(dir.path(), 3).is_empty());
    }

    // ------------------------------------------------------------------
    // warn_on_coexisting_configs: path without filename (edge branch)
    // shadowed is empty -> early return without recording
    // ------------------------------------------------------------------

    #[test]
    fn warn_on_coexisting_configs_empty_shadowed_is_silent() {
        let ((), captured) = capture_coexisting_config_warnings(|| {
            warn_on_coexisting_configs(Path::new(".fallowrc.json"), &[]);
        });
        assert!(
            captured.is_empty(),
            "empty shadowed list must produce no warning"
        );
    }

    // ------------------------------------------------------------------
    // extract_extends: array with non-string entries are filtered out
    // ------------------------------------------------------------------

    #[test]
    fn extract_extends_array_filters_non_strings() {
        let mut value = serde_json::json!({
            "extends": ["a.json", 42, null, "b.json", true]
        });
        let extends = extract_extends(&mut value);
        assert_eq!(extends, vec!["a.json", "b.json"]);
    }

    // ------------------------------------------------------------------
    // record_extends_visit: circular file-extends detected
    // (secondary path: same canonical path inserted twice)
    // ------------------------------------------------------------------

    #[test]
    #[cfg_attr(miri, ignore)]
    fn record_extends_visit_circular_same_file() {
        let dir = test_dir("visit-circular");
        let path = dir.path().join("config.json");
        std::fs::write(&path, "{}").unwrap();

        let mut visited = FxHashSet::default();
        record_extends_visit(&path, &mut visited).unwrap();
        let result = record_extends_visit(&path, &mut visited);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("Circular extends"),
            "second visit of same file must report circular: {err}"
        );
    }

    // ------------------------------------------------------------------
    // find_and_load: stops at .svn dir (is_repo_root branch)
    // ------------------------------------------------------------------

    #[test]
    #[cfg_attr(miri, ignore)]
    fn find_config_path_stops_at_svn_dir() {
        let dir = test_dir("find-path-svn");
        let sub = dir.path().join("sub");
        std::fs::create_dir(&sub).unwrap();
        std::fs::create_dir(dir.path().join(".svn")).unwrap();

        let path = FallowConfig::find_config_path(&sub);
        assert!(path.is_none(), "svn root should stop config search");
    }

    // ------------------------------------------------------------------
    // deep_merge: array over object replaces
    // ------------------------------------------------------------------

    #[test]
    fn deep_merge_array_over_object_replaces() {
        let mut base = serde_json::json!({"key": "value"});
        deep_merge_json(&mut base, serde_json::json!(["a", "b"]));
        assert_eq!(base, serde_json::json!(["a", "b"]));
    }

    // ------------------------------------------------------------------
    // find_and_load: returns an error when config parses but glob validation fails
    // ------------------------------------------------------------------

    #[test]
    #[cfg_attr(miri, ignore)]
    fn find_and_load_returns_error_for_invalid_glob_in_config() {
        let dir = test_dir("find-invalid-glob");
        std::fs::create_dir(dir.path().join(".git")).unwrap();
        std::fs::write(
            dir.path().join(".fallowrc.json"),
            r#"{"entry": ["[invalid-glob"]}"#,
        )
        .unwrap();

        let result = FallowConfig::find_and_load(dir.path());
        assert!(
            result.is_err(),
            "invalid glob should surface as an error from find_and_load"
        );
    }

    // ------------------------------------------------------------------
    // resolve_package_exports: Object map with "." key that is not a
    // string or object returns None (the `_ => None` arm)
    // ------------------------------------------------------------------

    #[test]
    fn resolve_package_exports_dot_key_array_returns_none() {
        // "." value is an array, which is neither String nor Object
        let pkg = serde_json::json!({
            "exports": {".": ["array-value"]}
        });
        let result = resolve_package_exports(&pkg, Path::new("/tmp"));
        assert!(result.is_none(), "array dot-export should return None");
    }

    #[test]
    fn resolve_package_exports_exports_is_array_returns_none() {
        // top-level "exports" is an array (not String or Object)
        let pkg = serde_json::json!({
            "exports": ["./index.js"]
        });
        let result = resolve_package_exports(&pkg, Path::new("/tmp"));
        assert!(result.is_none(), "array-form exports should return None");
    }

    #[test]
    fn resolve_package_exports_object_no_dot_key_returns_none() {
        // Object exports without "." key
        let pkg = serde_json::json!({
            "exports": {"./sub": "./sub.js"}
        });
        let result = resolve_package_exports(&pkg, Path::new("/tmp"));
        assert!(result.is_none(), "no dot key should return None");
    }

    #[test]
    fn resolve_package_exports_conditions_without_known_key_returns_none() {
        // "." is an Object but none of the known condition keys are present
        let pkg = serde_json::json!({
            "exports": {".": {"browser": "./browser.js"}}
        });
        let result = resolve_package_exports(&pkg, Path::new("/tmp"));
        assert!(result.is_none(), "unknown condition key should return None");
    }

    // ------------------------------------------------------------------
    // npm package: exports condition "import" key (one of the priority keys)
    // ------------------------------------------------------------------

    #[test]
    #[cfg_attr(miri, ignore)]
    fn extends_npm_exports_import_condition() {
        let dir = test_dir("npm-import-cond");
        let pkg_dir = dir.path().join("node_modules/import-config");
        std::fs::create_dir_all(&pkg_dir).unwrap();
        std::fs::write(
            pkg_dir.join("package.json"),
            r#"{"name": "import-config", "exports": {".": {"import": "./esm.json"}}}"#,
        )
        .unwrap();
        std::fs::write(
            pkg_dir.join("esm.json"),
            r#"{"rules": {"unused-types": "warn"}}"#,
        )
        .unwrap();

        std::fs::write(
            dir.path().join(".fallowrc.json"),
            r#"{"extends": "npm:import-config"}"#,
        )
        .unwrap();

        let config = FallowConfig::load(&dir.path().join(".fallowrc.json")).unwrap();
        assert_eq!(config.rules.unused_types, Severity::Warn);
    }

    // ------------------------------------------------------------------
    // npm package: exports condition "require" key
    // ------------------------------------------------------------------

    #[test]
    #[cfg_attr(miri, ignore)]
    fn extends_npm_exports_require_condition() {
        let dir = test_dir("npm-require-cond");
        let pkg_dir = dir.path().join("node_modules/require-config");
        std::fs::create_dir_all(&pkg_dir).unwrap();
        std::fs::write(
            pkg_dir.join("package.json"),
            r#"{"name": "require-config", "exports": {".": {"require": "./cjs.json"}}}"#,
        )
        .unwrap();
        std::fs::write(
            pkg_dir.join("cjs.json"),
            r#"{"rules": {"unused-class-members": "warn"}}"#,
        )
        .unwrap();

        std::fs::write(
            dir.path().join(".fallowrc.json"),
            r#"{"extends": "npm:require-config"}"#,
        )
        .unwrap();

        let config = FallowConfig::load(&dir.path().join(".fallowrc.json")).unwrap();
        assert_eq!(config.rules.unused_class_members, Severity::Warn);
    }
}
