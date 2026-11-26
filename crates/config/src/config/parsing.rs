use std::net::Ipv6Addr;
use std::path::{Path, PathBuf};
use std::time::Duration;

use rustc_hash::{FxHashMap, FxHashSet};

use super::FallowConfig;

#[path = "parsing_extends.rs"]
mod extends_entry;
use extends_entry::{ExtendsTarget, LocalExtendsEntry, extends_target, outside_extends_error};

pub use fallow_types::config_file_names::{
    CONFIG_FILE_NAMES, UNRECOGNIZED_CONFIG_FILE_NAMES, config_file_name,
};

/// Supported config file names in priority order.
const CONFIG_NAMES: &[&str] = CONFIG_FILE_NAMES;

const MAX_EXTENDS_DEPTH: usize = 10;

/// Prefix for npm package specifiers in the `extends` field.
const NPM_PREFIX: &str = "npm:";

/// Prefix for HTTPS URL specifiers in the `extends` field.
const HTTPS_PREFIX: &str = "https://";

/// Prefix for HTTP URL specifiers (rejected with a clear error).
const HTTP_PREFIX: &str = "http://";

/// Default timeout for fetching remote configs via URL extends.
const DEFAULT_URL_TIMEOUT_SECS: u64 = 5;

/// Host-controlled trust policy and fetch limit for loading a fallow config.
///
/// The default refuses remote `extends` entries and gives each fetch the
/// default time limit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct ConfigLoadOptions {
    /// Permit `https://` entries in the config inheritance graph.
    pub allow_remote_extends: bool,
    /// The time limit of the fetch of one remote `extends` entry. The host
    /// reads it from `FALLOW_EXTENDS_TIMEOUT_SECS` with
    /// [`extends_timeout_from_env_value`].
    pub extends_timeout: Duration,
}

impl ConfigLoadOptions {
    /// The options with the trust policy `allow_remote_extends` and the
    /// default time limit of a remote fetch.
    #[must_use]
    pub const fn new(allow_remote_extends: bool) -> Self {
        Self {
            allow_remote_extends,
            extends_timeout: Duration::from_secs(DEFAULT_URL_TIMEOUT_SECS),
        }
    }

    /// The options with `timeout` as the time limit of a remote fetch.
    #[must_use]
    pub const fn with_extends_timeout(mut self, timeout: Duration) -> Self {
        self.extends_timeout = timeout;
        self
    }
}

impl Default for ConfigLoadOptions {
    fn default() -> Self {
        Self::new(false)
    }
}

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
fn deep_merge_json(base: &mut serde_json::Value, overlay: serde_json::Value) {
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
    parse_config_to_value_in(path, true)
}

/// [`parse_config_to_value`] for a file that is `inside` the project or
/// not. A parse error of a file outside the project names the position
/// only.
fn parse_config_to_value_in(
    path: &Path,
    inside: bool,
) -> Result<serde_json::Value, miette::Report> {
    // An `extends` target can be outside the project by design, so the read
    // has no root check. It still refuses devices and very large files, and
    // the parse errors below name a position but never quote the file.
    let content = fallow_types::read_guard::read_to_string_bounded(
        path,
        fallow_types::read_guard::DEFAULT_MAX_READ_BYTES,
    )
    .map_err(|e| miette::miette!("Failed to read config file: {e}"))?;
    let content = content.trim_start_matches('\u{FEFF}');

    match ConfigFormat::from_path(path) {
        ConfigFormat::Toml => {
            let toml_value: toml::Value = crate::toml_input::from_str(content).map_err(|e| {
                let reason = if inside {
                    crate::parse_error::describe_toml(&e, content)
                } else {
                    crate::parse_error::position_only(crate::parse_error::toml_position(
                        &e, content,
                    ))
                };
                miette::miette!("Failed to parse config file {}: {reason}", path.display())
            })?;
            serde_json::to_value(toml_value).map_err(|e| {
                miette::miette!(
                    "Failed to convert TOML to JSON for {}: {}",
                    path.display(),
                    crate::parse_error::describe_json(&e)
                )
            })
        }
        ConfigFormat::Json => crate::jsonc::parse_to_value(content).map_err(|e| {
            let reason = if inside {
                crate::parse_error::describe_jsonc(&e)
            } else {
                crate::parse_error::position_only(Some(crate::parse_error::jsonc_position(&e)))
            };
            miette::miette!("Failed to parse config file {}: {reason}", path.display())
        }),
    }
}

/// True when `dir` holds a `.git`, `.hg` or `.svn` entry. A link counts by
/// its own presence, not by its target.
fn is_repo_root(dir: &Path) -> bool {
    [".git", ".hg", ".svn"]
        .iter()
        .any(|marker| entry_present(&dir.join(marker)))
}

/// True when `path` names an entry, also a dangling link. The check reads
/// the entry itself and does not follow a link.
fn entry_present(path: &Path) -> bool {
    path.symlink_metadata().is_ok()
}

/// The directory that a discovered config file in `dir` must resolve in:
/// the nearest repository root at or above `dir`, else `dir` itself. A
/// repository root above `dir` at the home directory or above it holds
/// dotfiles, not the project, so it does not count.
fn discovery_base(dir: &Path) -> &Path {
    dir.ancestors()
        .find(|ancestor| is_repo_root(ancestor))
        .filter(|top| {
            *top == dir
                || dunce::canonicalize(top)
                    .is_ok_and(|top| !crate::project_read::is_home_or_above(&top))
        })
        .unwrap_or(dir)
}

/// Refuse a discovered config file that is a link which does not resolve
/// inside its [`discovery_base`], when git tracks the link or no git
/// checkout holds it. A local link that git does not track, such as a link
/// to a dotfiles config, stays allowed. The message names only the link,
/// and it is the same for a dangling link and a link to a file that exists.
fn check_discovered_config(dir: &Path, candidate: &Path) -> Result<(), String> {
    let is_link = candidate
        .symlink_metadata()
        .is_ok_and(|meta| meta.file_type().is_symlink());
    let base = discovery_base(dir);
    if !is_link
        || fallow_types::read_guard::is_inside_root(base, candidate)
        || super::outside_root::git_tracks(candidate) == Some(false)
    {
        return Ok(());
    }
    Err(format!(
        "refusing to load {}: it is a symbolic link that does not resolve to a file inside {}",
        candidate.display(),
        base.display()
    ))
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
        let scope = crate::project_scope_base(source_config.parent().unwrap_or(package_dir));
        let content = crate::PackageJson::read_source_in_scope(&scope, &pkg_json_path)
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

/// Normalize a URL for config resource identity.
fn normalize_url_for_dedup(url: &str) -> String {
    let Some((scheme, rest)) = url.split_once("://") else {
        return url.to_string();
    };
    let scheme = scheme.to_ascii_lowercase();

    let rest = rest.split_once('#').map_or(rest, |(value, _)| value);
    let authority_end = rest.find(['/', '?']).unwrap_or(rest.len());
    let (authority, tail) = rest.split_at(authority_end);
    let authority = normalize_url_authority(authority, &scheme);

    let tail = if tail == "/" {
        ""
    } else if tail.starts_with("/?") {
        &tail[1..]
    } else {
        tail
    };

    if tail.is_empty() {
        format!("{scheme}://{authority}")
    } else {
        format!("{scheme}://{authority}{tail}")
    }
}

/// Normalize only the case-insensitive parts of a URL authority and its default port.
fn normalize_url_authority(authority: &str, scheme: &str) -> String {
    let (userinfo, host_port) = authority
        .rsplit_once('@')
        .map_or((None, authority), |(userinfo, host_port)| {
            (Some(userinfo), host_port)
        });

    let (host, port) = if host_port.starts_with('[') {
        host_port.find("]:").map_or((host_port, None), |separator| {
            let (host, port) = host_port.split_at(separator + 1);
            (host, port.strip_prefix(':'))
        })
    } else {
        host_port
            .rsplit_once(':')
            .map_or((host_port, None), |(host, port)| (host, Some(port)))
    };

    let port = port.map(|value| {
        value
            .parse::<u16>()
            .map_or_else(|_| value.to_string(), |number| number.to_string())
    });
    let port =
        port.filter(|port| !matches!((scheme, port.as_str()), ("http", "80") | ("https", "443")));
    let host = host
        .strip_prefix('[')
        .and_then(|value| value.strip_suffix(']'))
        .and_then(|value| value.parse::<Ipv6Addr>().ok())
        .map_or_else(
            || host.to_ascii_lowercase(),
            |address| format!("[{address}]"),
        );

    match (userinfo, port.as_deref()) {
        (Some(userinfo), Some(port)) => format!("{userinfo}@{host}:{port}"),
        (Some(userinfo), None) => format!("{userinfo}@{host}"),
        (None, Some(port)) => format!("{host}:{port}"),
        (None, None) => host,
    }
}

/// Format a remote config location for diagnostics without exposing URL secrets.
fn remote_config_display(location: &str) -> String {
    let Some((scheme, rest)) = location.split_once("://") else {
        return location.to_string();
    };

    let authority_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let (authority, tail) = rest.split_at(authority_end);
    let authority = authority
        .rsplit_once('@')
        .map_or(authority, |(_, host)| host);
    let path_end = tail.find(['?', '#']).unwrap_or(tail.len());
    let path = &tail[..path_end];

    format!("{scheme}://{authority}{path}")
}

/// Format a fetch error without trusting the HTTP client's URL rendering.
fn remote_fetch_error_display(error: &ureq::Error, _url: &str) -> String {
    match error {
        ureq::Error::RequireHttpsOnly(redirect_target) => format!(
            "configured for https only: {}",
            remote_config_display(redirect_target)
        ),
        ureq::Error::StatusCode(code) => format!("http status {code}"),
        ureq::Error::HostNotFound => "host not found".to_string(),
        ureq::Error::Timeout(_) => "request timed out".to_string(),
        _ => "request failed".to_string(),
    }
}

fn starts_with_url_prefix(value: &str, prefix: &str) -> bool {
    value
        .get(..prefix.len())
        .is_some_and(|candidate| candidate.eq_ignore_ascii_case(prefix))
}

/// Parse a raw `FALLOW_EXTENDS_TIMEOUT_SECS` value into the time limit of a
/// remote fetch. An absent, zero or non-numeric value gives the default limit
/// of 5 seconds.
#[must_use]
pub fn extends_timeout_from_env_value(raw: Option<&str>) -> Duration {
    raw.and_then(|v| v.parse::<u64>().ok().filter(|&n| n > 0))
        .map_or(
            Duration::from_secs(DEFAULT_URL_TIMEOUT_SECS),
            Duration::from_secs,
        )
}

/// Maximum response body size for fetched config files.
const MAX_URL_CONFIG_BYTES: u64 = 1024 * 1024;

/// Fetch a remote JSON config from an HTTPS URL.
fn fetch_url_config(
    url: &str,
    source: &str,
    timeout: Duration,
) -> Result<serde_json::Value, miette::Report> {
    let url_display = remote_config_display(url);
    let source_display = remote_config_display(source);
    let agent = ureq::Agent::config_builder()
        .timeout_global(Some(timeout))
        .https_only(true)
        .build()
        .new_agent();

    let mut response = agent.get(url).call().map_err(|e| {
        let error_display = remote_fetch_error_display(&e, url);
        miette::miette!(
            "Failed to fetch remote config from {url_display} \
             (referenced from {source_display}): {error_display}. \
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
                "Failed to read response body from {url_display} \
                 (referenced from {source_display}): {e}"
            )
        })?;

    crate::jsonc::parse_to_value(&body).map_err(|e| {
        miette::miette!(
            "Failed to parse remote config as JSON from {url_display} \
             (referenced from {source_display}): {e}. \
             Only JSON/JSONC is supported for URL-sourced configs"
        )
    })
}

trait RemoteConfigFetcher {
    fn fetch(&mut self, url: &str, source: &str) -> Result<serde_json::Value, miette::Report>;
}

/// The fetcher of remote `extends` entries over HTTPS, with the time limit
/// of one fetch.
struct NetworkRemoteConfigFetcher {
    timeout: Duration,
}

impl RemoteConfigFetcher for NetworkRemoteConfigFetcher {
    fn fetch(&mut self, url: &str, source: &str) -> Result<serde_json::Value, miette::Report> {
        fetch_url_config(url, source, self.timeout)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum ConfigResourceId {
    Local(PathBuf),
    Remote(String),
}

/// A fetcher for walks that list the local config files only. It reads no
/// remote config and gives an empty one in its place.
struct NoRemoteFetcher;

impl RemoteConfigFetcher for NoRemoteFetcher {
    fn fetch(&mut self, _url: &str, _source: &str) -> Result<serde_json::Value, miette::Report> {
        Ok(serde_json::Value::Object(serde_json::Map::new()))
    }
}

struct ExtendsResolver<'a, Fetcher> {
    options: ConfigLoadOptions,
    active: FxHashSet<ConfigResourceId>,
    resolved: FxHashMap<ConfigResourceId, ResolvedConfig>,
    /// The canonical local files that the walk reads, in read order.
    local_files: Vec<PathBuf>,
    /// The merge order of each source that is resolving now, innermost
    /// last. It is empty when the walk does not record the merge order. See
    /// [`MergeOrder`].
    order_frames: Vec<MergeOrder>,
    /// The merge order of the walk after the root resolves, when the walk
    /// records it.
    merge_order: MergeOrder,
    record_merge_order: bool,
    /// The directory that the files of the walk count as inside, set from
    /// the directory that holds the first local file, before a link
    /// resolves. See [`crate::project_scope_base`].
    scope: Option<PathBuf>,
    /// Each local file of the walk with its own value, and whether it is
    /// inside `scope`.
    local_values: Vec<super::outside_root::LocalValue>,
    fetcher: &'a mut Fetcher,
}

/// Config sources in the order that the load merges their own keys: the
/// extends targets of a file come before the file itself, so a later source
/// overrides an earlier one. The merge can apply a source more than one
/// time (a diamond or a lattice of extends). Only the last time matters,
/// so each source keeps only its last position. The length is then at most
/// the number of distinct sources.
#[derive(Default, Clone)]
struct MergeOrder(Vec<ConfigResourceId>);

impl MergeOrder {
    /// Append `sources` in order. A source that is already in the order
    /// moves to its new, later position.
    fn append(&mut self, sources: &[ConfigResourceId]) {
        for source in sources {
            if let Some(position) = self.0.iter().position(|known| known == source) {
                self.0.remove(position);
            }
            self.0.push(source.clone());
        }
    }
}

/// A config source that the walk resolved once, kept for reuse.
struct ResolvedConfig {
    /// The merged value of the source and its extends chain.
    value: serde_json::Value,
    /// The merge order of the source: its extends chain, then the source
    /// itself. A reuse appends it again, because the merge applies the full
    /// cached value again. It is empty when the walk does not record the
    /// merge order.
    merge_order: MergeOrder,
}

impl<'a, Fetcher: RemoteConfigFetcher> ExtendsResolver<'a, Fetcher> {
    fn new(options: ConfigLoadOptions, fetcher: &'a mut Fetcher) -> Self {
        Self {
            options,
            active: FxHashSet::default(),
            resolved: FxHashMap::default(),
            local_files: Vec::new(),
            order_frames: Vec::new(),
            merge_order: MergeOrder::default(),
            record_merge_order: false,
            scope: None,
            local_values: Vec::new(),
            fetcher,
        }
    }

    /// Record the merge order of the walk. A normal load does not need it.
    fn recording_merge_order(mut self) -> Self {
        self.record_merge_order = true;
        self
    }

    /// Start the merge order of a source that resolves now.
    fn open_order_frame(&mut self) {
        if self.record_merge_order {
            self.order_frames.push(MergeOrder::default());
        }
    }

    /// Finish the merge order of the source that resolved now: append the
    /// source itself, then give the order to the source that extends it.
    fn close_order_frame(&mut self, identity: &ConfigResourceId, resolved: bool) -> MergeOrder {
        if !self.record_merge_order {
            return MergeOrder::default();
        }
        let mut order = self.order_frames.pop().unwrap_or_default();
        if resolved {
            order.append(std::slice::from_ref(identity));
            self.append_to_parent_order(&order);
        }
        order
    }

    fn append_to_parent_order(&mut self, order: &MergeOrder) {
        let parent = self
            .order_frames
            .last_mut()
            .unwrap_or(&mut self.merge_order);
        parent.append(&order.0);
    }

    fn resolve_local(
        &mut self,
        path: &Path,
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
        let identity = ConfigResourceId::Local(canonical.clone());
        // The first file is the config of the project, and it can be a link
        // to a file elsewhere. The scope comes from the directory that holds
        // the config entry, not from the link target.
        let scope = self.scope.get_or_insert_with(|| {
            let entry = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
            crate::project_scope_base(entry.parent().unwrap_or(&canonical))
        });
        let inside = canonical.starts_with(scope);
        if let Some(value) = self.reuse_resolved(&identity) {
            return Ok(value);
        }
        if !self.active.insert(identity.clone()) {
            return Err(miette::miette!(
                "Circular extends detected: {} is already active in the extends chain",
                path.display()
            ));
        }

        // The list holds the files that the walk reads, so a file that the
        // bounded read refuses for its type or size is not in it.
        if fallow_types::read_guard::is_bounded_regular_file(
            &canonical,
            fallow_types::read_guard::DEFAULT_MAX_READ_BYTES,
        ) {
            self.local_files.push(canonical.clone());
        }
        self.open_order_frame();
        let result = self.resolve_local_uncached(&canonical, depth, inside);
        self.active.remove(&identity);
        let order = self.close_order_frame(&identity, result.is_ok());
        if let Ok(value) = &result {
            self.remember_resolved(identity, value, order);
        }
        result
    }

    /// The cached value of a source that the walk resolved before. The merge
    /// applies the full cached value again, so the merge order gets the
    /// sources of that value again, in their first order.
    fn reuse_resolved(&mut self, identity: &ConfigResourceId) -> Option<serde_json::Value> {
        let resolved = self.resolved.get(identity)?;
        let value = resolved.value.clone();
        if self.record_merge_order {
            let order = resolved.merge_order.clone();
            self.append_to_parent_order(&order);
        }
        Some(value)
    }

    fn remember_resolved(
        &mut self,
        identity: ConfigResourceId,
        value: &serde_json::Value,
        merge_order: MergeOrder,
    ) {
        self.resolved.insert(
            identity,
            ResolvedConfig {
                value: value.clone(),
                merge_order,
            },
        );
    }

    fn resolve_local_uncached(
        &mut self,
        path: &Path,
        depth: usize,
        inside: bool,
    ) -> Result<serde_json::Value, miette::Report> {
        let mut value = parse_config_to_value_in(path, inside)?;
        let extends = extract_extends(&mut value, &path.display().to_string())?;
        self.local_values
            .push(super::outside_root::LocalValue::new(path, inside, &value));
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
        for entry in &extends {
            let target = extends_target(LocalExtendsEntry {
                path,
                config_dir,
                entry,
                sealed,
                sealed_dir_canonical: sealed_dir_canonical.as_deref(),
            })
            .map_err(|error| {
                if inside {
                    error
                } else {
                    outside_extends_error(path)
                }
            })?;
            let base = match target {
                ExtendsTarget::Remote(url) => self.resolve_remote(url, depth + 1)?,
                ExtendsTarget::Local(target) => self.resolve_local(&target, depth + 1)?,
            };
            deep_merge_json(&mut merged, base);
        }
        deep_merge_json(&mut merged, value);
        Ok(merged)
    }
    fn resolve_remote(
        &mut self,
        url: &str,
        depth: usize,
    ) -> Result<serde_json::Value, miette::Report> {
        if depth >= MAX_EXTENDS_DEPTH {
            let url_display = remote_config_display(url);
            return Err(miette::miette!(
                "Config extends chain too deep (>={MAX_EXTENDS_DEPTH} levels) at {url_display}"
            ));
        }
        if !self.options.allow_remote_extends {
            let url_display = remote_config_display(url);
            return Err(miette::miette!(
                "Remote config extends '{url_display}' is disabled by default. \
                 CLI users can pass --allow-remote-extends. Library callers can use \
                 ConfigLoadOptions::new(true)"
            ));
        }

        let identity = ConfigResourceId::Remote(normalize_url_for_dedup(url));
        if let Some(value) = self.reuse_resolved(&identity) {
            return Ok(value);
        }
        if !self.active.insert(identity.clone()) {
            let url_display = remote_config_display(url);
            return Err(miette::miette!(
                "Circular extends detected: {url_display} is already active in the extends chain"
            ));
        }

        self.open_order_frame();
        let result = self.resolve_remote_uncached(url, depth);
        self.active.remove(&identity);
        let order = self.close_order_frame(&identity, result.is_ok());
        if let Ok(value) = &result {
            self.remember_resolved(identity, value, order);
        }
        result
    }

    fn resolve_remote_uncached(
        &mut self,
        url: &str,
        depth: usize,
    ) -> Result<serde_json::Value, miette::Report> {
        let mut value = self.fetcher.fetch(url, url)?;
        let extends = extract_extends(&mut value, &remote_config_display(url))?;
        if extends.is_empty() {
            return Ok(value);
        }

        let url_display = remote_config_display(url);
        let mut merged = serde_json::Value::Object(serde_json::Map::new());
        for entry in &extends {
            let base = if starts_with_url_prefix(entry, HTTPS_PREFIX) {
                self.resolve_remote(entry, depth + 1)?
            } else if starts_with_url_prefix(entry, HTTP_PREFIX) {
                let entry_display = remote_config_display(entry);
                return Err(miette::miette!(
                    "URL extends must use https://, got http:// URL '{}' (in remote config {}). \
                     Change the URL to use https:// instead",
                    entry_display,
                    url_display
                ));
            } else if let Some(npm_specifier) = entry.strip_prefix(NPM_PREFIX) {
                let cwd = std::env::current_dir().map_err(|e| {
                    miette::miette!(
                        "Cannot resolve npm: specifier from URL-sourced config: \
                         failed to determine current directory: {e}"
                    )
                })?;
                let path_placeholder = PathBuf::from(&url_display);
                let npm_path = resolve_npm_package(&cwd, npm_specifier, &path_placeholder)?;
                self.resolve_local(&npm_path, depth + 1)?
            } else {
                return Err(miette::miette!(
                    "Relative paths in 'extends' are not supported when the base config was \
                     fetched from a URL ('{url_display}'). Use another https:// URL or npm: reference \
                     instead. Got: '{entry}'"
                ));
            };
            deep_merge_json(&mut merged, base);
        }
        deep_merge_json(&mut merged, value);
        Ok(merged)
    }
}

/// Extract the `extends` array from a parsed JSON config value.
///
/// Fails loud on malformed values: the key is removed before
/// deserialization, so `FallowConfig`'s `deny_unknown_fields` never sees a
/// bad `extends` and the base config would otherwise go unmerged silently.
fn extract_extends(
    value: &mut serde_json::Value,
    source: &str,
) -> Result<Vec<String>, miette::Report> {
    let Some(extends) = value.as_object_mut().and_then(|obj| obj.remove("extends")) else {
        return Ok(Vec::new());
    };
    match extends {
        serde_json::Value::String(s) => Ok(vec![s]),
        serde_json::Value::Array(arr) => arr
            .into_iter()
            .map(|entry| match entry {
                serde_json::Value::String(s) => Ok(s),
                other => Err(miette::miette!(
                    "extends entries must be strings, got {} (in {source})",
                    json_type_name(&other)
                )),
            })
            .collect(),
        other => Err(miette::miette!(
            "extends must be a string or an array of strings, got {} (in {source})",
            json_type_name(&other)
        )),
    }
}

fn json_type_name(value: &serde_json::Value) -> &'static str {
    match value {
        serde_json::Value::Null => "null",
        serde_json::Value::Bool(_) => "a boolean",
        serde_json::Value::Number(_) => "a number",
        serde_json::Value::String(_) => "a string",
        serde_json::Value::Array(_) => "an array",
        serde_json::Value::Object(_) => "an object",
    }
}

#[cfg(test)]
fn resolve_url_extends(
    url: &str,
    _visited: &mut FxHashSet<String>,
    depth: usize,
) -> Result<serde_json::Value, miette::Report> {
    let options = ConfigLoadOptions::new(true);
    let mut fetcher = NetworkRemoteConfigFetcher {
        timeout: options.extends_timeout,
    };
    ExtendsResolver::new(options, &mut fetcher).resolve_remote(url, depth)
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

/// Public entry point: resolve a config file with all its extends chain.
///
/// Delegates to [`resolve_extends_file`] with a fresh visited set.
#[cfg(test)]
fn resolve_extends(
    path: &Path,
    _visited: &mut FxHashSet<String>,
    depth: usize,
) -> Result<serde_json::Value, miette::Report> {
    let options = ConfigLoadOptions::default();
    let mut fetcher = NetworkRemoteConfigFetcher {
        timeout: options.extends_timeout,
    };
    ExtendsResolver::new(options, &mut fetcher).resolve_local(path, depth)
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
fn collect_unknown_rule_keys(merged: &serde_json::Value) -> Vec<super::rules::UnknownRuleKey> {
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
fn capture_unknown_rule_warnings<F: FnOnce() -> R, R>(
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
/// `MAJOR.MINOR.PATCH` as a comparable triple. Deliberately not a full semver
/// parser: the config field mirrors the release tag format, which the release
/// workflow already constrains to exactly three numbers.
pub(super) fn version_triple(value: &str) -> Option<(u64, u64, u64)> {
    let mut parts = value.split('.');
    let triple = (
        parts.next()?.parse().ok()?,
        parts.next()?.parse().ok()?,
        parts.next()?.parse().ok()?,
    );
    parts.next().is_none().then_some(triple)
}

/// Refuse a config written for a newer binary before `deny_unknown_fields`
/// reports the first field this build happens not to know. Without this a team
/// that commits a field introduced in a newer version breaks every runner still
/// on the old one with an error that names the field, so the reader's first
/// guess is a typo rather than version skew.
fn assert_binary_meets_minimum_version(
    path: &Path,
    merged: &serde_json::Value,
    outside: &super::outside_root::OutsideKeys,
) -> Result<(), miette::Report> {
    let Some(declared) = merged.get("minimumVersion") else {
        return Ok(());
    };
    let declared = declared.as_str().and_then(version_triple).ok_or_else(|| {
        if outside.covers("minimumVersion") {
            return miette::miette!(
                "{} in {}",
                outside.field_message("minimumVersion"),
                path.display()
            );
        }
        miette::miette!(
            "minimumVersion must be MAJOR.MINOR.PATCH in {}, found {declared}",
            path.display()
        )
    })?;

    let running = env!("CARGO_PKG_VERSION");
    let Some(current) = version_triple(running) else {
        return Ok(());
    };
    if current >= declared {
        return Ok(());
    }

    let (major, minor, patch) = declared;
    Err(miette::miette!(
        "{} requires fallow {major}.{minor}.{patch} or newer, running {running}. \
         Upgrade fallow, or remove minimumVersion and the fields that need it.",
        path.display()
    ))
}

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
        .filter(|name| entry_present(&dir.join(name)))
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
fn capture_coexisting_config_warnings<F: FnOnce() -> R, R>(body: F) -> (R, Vec<CoexistWarning>) {
    COEXIST_CAPTURE.with(|cell| {
        *cell.borrow_mut() = Some(Vec::new());
    });
    let result = body();
    let findings = COEXIST_CAPTURE.with(|cell| cell.borrow_mut().take().unwrap_or_default());
    (result, findings)
}

/// Warn once per config file about each key under `workspaces` that fallow
/// does not know. An error would break configs that carry a stray key today.
fn warn_on_unknown_workspace_keys(
    config_path: &Path,
    workspaces: Option<&crate::WorkspaceConfig>,
    outside: &super::outside_root::OutsideKeys,
) {
    use std::sync::{Mutex, OnceLock};

    static WARNED: OnceLock<Mutex<FxHashSet<String>>> = OnceLock::new();
    let Some(workspaces) = workspaces else {
        return;
    };
    let warned = WARNED.get_or_init(|| Mutex::new(FxHashSet::default()));
    for key in workspaces.unknown_keys.keys() {
        if outside.covers(&format!("workspaces.{key}")) {
            let dedupe_key = format!("{}::outside", config_path.display());
            if warned.lock().is_ok_and(|mut set| set.insert(dedupe_key)) {
                tracing::warn!(
                    "unknown key under 'workspaces' in a config file outside the project ({}); \
                     fallow ignores it. The text of a file outside the project is not shown.",
                    outside.file_list(),
                );
            }
            continue;
        }
        let dedupe_key = format!("{}::{key}", config_path.display());
        if let Ok(mut set) = warned.lock()
            && !set.insert(dedupe_key)
        {
            continue;
        }
        tracing::warn!(
            "unknown key 'workspaces.{key}' in {}; fallow ignores it. The known keys are \
             'patterns', 'changedSince' and 'settings'.",
            config_path.display(),
        );
    }
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

fn load_with_fetcher<Fetcher: RemoteConfigFetcher>(
    path: &Path,
    options: ConfigLoadOptions,
    fetcher: &mut Fetcher,
) -> Result<FallowConfig, miette::Report> {
    let mut resolver = ExtendsResolver::new(options, fetcher);
    let merged = resolver.resolve_local(path, 0)?;
    FallowConfig::from_merged(path, merged, &resolver.local_values)
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
    /// `dynamicallyLoaded`, `duplicates.ignore`, `similarCode.ignore`, `health.ignore`,
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
        Self::load_with_options(path, ConfigLoadOptions::default())
    }

    /// Load config with a host-controlled inheritance trust policy.
    ///
    /// Remote `https://` extends are denied unless
    /// [`ConfigLoadOptions::allow_remote_extends`] is explicitly enabled for
    /// this call. Local and `npm:` extends are unaffected.
    ///
    /// # Errors
    ///
    /// Returns the same errors as [`Self::load`], plus a trust-policy error
    /// when a remote extends target is encountered without opt-in.
    pub fn load_with_options(
        path: &Path,
        options: ConfigLoadOptions,
    ) -> Result<Self, miette::Report> {
        let mut fetcher = NetworkRemoteConfigFetcher {
            timeout: options.extends_timeout,
        };
        load_with_fetcher(path, options, &mut fetcher)
    }

    /// The local files that a load of the config file at `path` reads.
    ///
    /// The list holds `path` and each local or `npm:` `extends` target, as
    /// canonical paths in the order that the load reads them. It leaves out
    /// remote `https://` targets and does not fetch them. The walk stops at
    /// the first entry that does not resolve, so a broken chain gives the
    /// files up to that entry.
    ///
    /// A host that keeps a loaded config across runs compares these files to
    /// know when to load the config again.
    #[must_use]
    pub fn local_source_files(path: &Path) -> Vec<PathBuf> {
        let mut fetcher = NoRemoteFetcher;
        let mut resolver = ExtendsResolver::new(ConfigLoadOptions::new(true), &mut fetcher);
        let _ = resolver.resolve_local(path, 0);
        resolver.local_files
    }

    /// The config sources of the file at `path` in merge order, or `None`
    /// when the extends chain does not resolve. A later entry overrides an
    /// earlier one. A remote `https://` source is `None` and is not fetched.
    pub(super) fn merge_order(path: &Path) -> Option<Vec<Option<PathBuf>>> {
        let mut fetcher = NoRemoteFetcher;
        let mut resolver = ExtendsResolver::new(ConfigLoadOptions::new(true), &mut fetcher)
            .recording_merge_order();
        resolver.resolve_local(path, 0).ok()?;
        let sources = resolver
            .merge_order
            .0
            .into_iter()
            .map(|source| match source {
                ConfigResourceId::Local(path) => Some(path),
                ConfigResourceId::Remote(_) => None,
            });
        Some(sources.collect())
    }

    fn from_merged(
        path: &Path,
        merged: serde_json::Value,
        local_values: &[super::outside_root::LocalValue],
    ) -> Result<Self, miette::Report> {
        let outside_keys = super::outside_root::OutsideKeys::from_values(local_values);
        assert_binary_meets_minimum_version(path, &merged, &outside_keys)?;
        let outside = super::outside_root::has_outside(local_values);
        if outside {
            for value in local_values.iter().filter(|value| value.inside) {
                warn_on_unknown_rule_keys(path, &value.value);
            }
        } else {
            warn_on_unknown_rule_keys(path, &merged);
        }

        let private_type_leaks_configured = merged
            .get("rules")
            .and_then(serde_json::Value::as_object)
            .is_some_and(|rules| {
                rules.contains_key("private-type-leaks") || rules.contains_key("private-type-leak")
            });
        let mut config: Self = serde_json::from_value(merged).map_err(|e| {
            if outside {
                return super::outside_root::merged_error(path, local_values);
            }
            let reason = crate::parse_error::describe_json(&e);
            // Unknown fields on overrides/ignoreExports entries are usually
            // inline annotations; point at JSONC comments as the fix.
            let hint = if matches!(ConfigFormat::from_path(path), ConfigFormat::Json)
                && reason.contains("unknown field")
                && (reason.contains("expected `files` or `rules`")
                    || reason.contains("expected `file` or `exports`"))
            {
                " (annotations belong in a // comment; .fallowrc.json accepts JSONC)"
            } else {
                ""
            };
            miette::miette!("{reason} in {}{hint}", path.display())
        })?;
        config.rules.private_type_leaks_configured = private_type_leaks_configured;
        config.outside_keys = outside_keys;
        warn_on_unknown_workspace_keys(path, config.workspaces.as_ref(), &config.outside_keys);
        config.check_merged_values()?;
        Ok(config)
    }

    /// The checks of the merged config that serde cannot express. A message
    /// for a value that can come from a config file outside the project
    /// names the key and that file only, see
    /// [`super::outside_root::OutsideKeys`].
    fn check_merged_values(&self) -> Result<(), miette::Report> {
        let config = self;
        let invalid = |errors: Vec<String>| {
            let joined = errors
                .into_iter()
                .map(|error| config.outside_keys.redact(error))
                .collect::<Vec<_>>()
                .join("\n  - ");
            miette::miette!("invalid config:\n  - {}", joined)
        };
        config
            .validate_user_globs()
            .map_err(|errors| invalid(errors.iter().map(ToString::to_string).collect()))?;
        if !config.security.request_receivers_are_valid() {
            return Err(invalid(vec![
                "security.requestReceivers entries must be non-empty strings".to_owned(),
            ]));
        }
        let mut health_errors = config.health.threshold_override_errors();
        health_errors.extend(config.health.change_coupling.errors());
        health_errors.extend(config.health.shallow_directories.errors());
        for (index, pattern) in config.health.tailwind_ignore_patterns.iter().enumerate() {
            if let Err(error) = regex::Regex::new(pattern) {
                health_errors.push(format!(
                    "health.tailwindIgnorePatterns[{index}] is not a valid regex: {error}"
                ));
            }
        }
        if !health_errors.is_empty() {
            return Err(invalid(health_errors));
        }
        let allow_errors = config.circular_dependencies.allow_errors();
        if !allow_errors.is_empty() {
            return Err(invalid(allow_errors));
        }
        if let Some(pattern) = &config.unused_component_props.ignore_pattern
            && let Err(e) = regex::Regex::new(pattern)
        {
            return Err(invalid(vec![format!(
                "unusedComponentProps.ignorePattern is not a valid regex: {e}"
            )]));
        }
        Ok(())
    }

    /// Validate all user-supplied glob patterns and directory paths in this config.
    ///
    /// Accumulates errors from every glob- or path-bearing field so the user
    /// sees ALL offending values in one run rather than fixing them one at a
    /// time.
    ///
    /// Covered filesystem glob fields: `entry`, `ignorePatterns`, `ignoreFindings`,
    /// `dynamicallyLoaded`, `duplicates.ignore`, `similarCode.ignore`, `health.ignore`,
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
        self.validate_workspace_baseline_keys(&mut errors);

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

    /// The config keys that name one file, with their values.
    pub(super) fn file_path_keys(&self) -> [(Option<String>, &'static str); 5] {
        let coverage = self
            .health
            .coverage
            .as_ref()
            .map(|path| path.to_string_lossy().into_owned());
        [
            (self.codeowners.clone(), "codeowners"),
            (coverage, "health.coverage"),
            (
                self.audit.dead_code_baseline.clone(),
                "audit.deadCodeBaseline",
            ),
            (self.audit.health_baseline.clone(), "audit.healthBaseline"),
            (self.audit.dupes_baseline.clone(), "audit.dupesBaseline"),
        ]
    }

    /// Check that each config key that names one file resolves inside the
    /// project scope base of `root` (see [`crate::project_scope_base`]). A
    /// relative path is taken from `root`, so a config of a monorepo package
    /// can name `../../.github/CODEOWNERS`. An absolute path is allowed too.
    /// A key whose path leaves the scope base, by text or through a link,
    /// is refused. A path that does not exist yet passes, so a later read
    /// can report it as missing. When `root` does not resolve, each key that
    /// is set is refused. CLI flags and environment variables that override
    /// these keys are not checked here.
    ///
    /// # Errors
    ///
    /// Returns one message for each refused key. A message names the key and
    /// its value, never a resolved path outside `root`.
    pub fn check_file_path_keys_inside(&self, root: &Path) -> Result<(), Vec<String>> {
        let canonical_root = dunce::canonicalize(root).ok();
        let base = crate::project_scope_base(root);
        let given_base = canonical_root.as_deref().map_or_else(
            || base.clone(),
            |canonical_root| crate::project_read::base_as_given(root, canonical_root, &base),
        );
        let errors: Vec<String> = self
            .file_path_keys()
            .into_iter()
            .filter_map(|(value, field)| value.map(|value| (value, field)))
            .filter(|(value, _)| {
                canonical_root.as_deref().is_none_or(|canonical_root| {
                    crate::project_read::project_path_entry(
                        &base,
                        &given_base,
                        canonical_root,
                        Path::new(value),
                    ) == crate::project_read::ProjectEntry::Outside
                })
            })
            .map(|(value, field)| {
                if self.outside_keys.covers(field) {
                    format!(
                        "{field}: a value from a config file outside the project ({}) resolves \
                         to a location outside the project scope base. The text of a file \
                         outside the project is not shown",
                        self.outside_keys.file_list()
                    )
                } else {
                    format!(
                        "{field}: '{value}' resolves to a location outside the project scope base"
                    )
                }
            })
            .collect();
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
        use super::glob_validation::{
            validate_user_dependency_globs, validate_user_finding_ignore_globs,
            validate_user_globs, validate_user_ignore_pattern_globs, validate_user_specifier_globs,
        };

        validate_user_globs(&self.entry, "entry", errors);
        validate_user_globs(&self.support_entry, "supportEntry", errors);
        if let Some(workspaces) = &self.workspaces {
            for (key, settings) in &workspaces.settings {
                validate_user_globs(
                    std::slice::from_ref(key),
                    "workspaces.settings (key)",
                    errors,
                );
                validate_user_dependency_globs(
                    &settings.ignore_dependencies,
                    "workspaces.settings[].ignoreDependencies",
                    errors,
                );
            }
        }
        validate_user_ignore_pattern_globs(&self.ignore_patterns, "ignorePatterns", errors);
        validate_user_finding_ignore_globs(&self.ignore_findings, "ignoreFindings", errors);
        validate_user_globs(&self.dynamically_loaded, "dynamicallyLoaded", errors);
        super::glob_validation::validate_user_plain_globs(
            self.production.ignore(),
            "production.ignore",
            errors,
        );
        validate_user_dependency_globs(&self.ignore_dependencies, "ignoreDependencies", errors);
        validate_user_dependency_globs(&self.ignore_binaries, "ignoreBinaries", errors);
        validate_user_specifier_globs(
            &self.ignore_unresolved_imports,
            "ignoreUnresolvedImports",
            errors,
        );
        validate_user_globs(&self.duplicates.ignore, "duplicates.ignore", errors);
        validate_user_globs(&self.similar_code.ignore, "similarCode.ignore", errors);
        validate_user_globs(&self.health.ignore, "health.ignore", errors);
        validate_user_globs(
            &self.health.shallow_directories.ignore,
            "health.shallowDirectories.ignore",
            errors,
        );
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
            validate_user_globs(&zone.public_files, "boundaries.zones[].publicFiles", errors);
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

    /// Validate the shape of the `workspaces.changedSince` keys. Whether a
    /// key names a discovered workspace is known only after discovery, so the
    /// engine checks that when a run reads the map.
    fn validate_workspace_baseline_keys(
        &self,
        errors: &mut Vec<super::glob_validation::GlobValidationError>,
    ) {
        let Some(workspaces) = &self.workspaces else {
            return;
        };
        for key in workspaces.changed_since.keys() {
            if let Err(e) =
                super::glob_validation::validate_workspace_root_key(key, "workspaces.changedSince")
            {
                errors.push(e);
            }
        }
    }

    /// Find the config file path without loading it.
    /// Searches the same locations as `find_and_load`.
    ///
    /// A config link that `find_and_load` refuses gives `None`, and the
    /// search stops there, as the load stops there with an error.
    #[must_use]
    pub fn find_config_path(start: &Path) -> Option<PathBuf> {
        let mut dir = start;
        loop {
            for name in CONFIG_NAMES {
                let candidate = dir.join(name);
                if entry_present(&candidate) {
                    return check_discovered_config(dir, &candidate)
                        .is_ok()
                        .then_some(candidate);
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
        Self::find_and_load_with_options(start, ConfigLoadOptions::default())
    }

    /// Find and load config with a host-controlled inheritance trust policy.
    ///
    /// # Errors
    ///
    /// Returns an error if a config file is found but cannot be read, parsed,
    /// or is not permitted by `options`.
    pub fn find_and_load_with_options(
        start: &Path,
        options: ConfigLoadOptions,
    ) -> Result<Option<(Self, PathBuf)>, String> {
        let mut dir = start;
        loop {
            for (idx, name) in CONFIG_NAMES.iter().enumerate() {
                let candidate = dir.join(name);
                if entry_present(&candidate) {
                    check_discovered_config(dir, &candidate)?;
                    warn_on_coexisting_configs(&candidate, &shadowed_config_names(dir, idx));
                    match Self::load_with_options(&candidate, options) {
                        Ok(config) => return Ok(Some((config, candidate))),
                        Err(e) => {
                            // Most load errors already name the config file;
                            // add the path only when the reason lacks it, so
                            // it is mentioned exactly once.
                            let msg = e.to_string();
                            return Err(if msg.contains(&candidate.display().to_string()) {
                                msg
                            } else {
                                format!("Failed to parse {}: {msg}", candidate.display())
                            });
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
    /// exit with code 2. A failure that only emits `tracing::error!` and
    /// continues produces a flood of false-positive boundary violations at
    /// analysis time (#468).
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
        self.validate_snapshot_boundaries(root, super::AnalysisSnapshot::Current)
    }

    /// [`Self::validate_resolved_boundaries`] for the given revision. The base
    /// revision of `audit --base` does not need the module folder of the
    /// `deep-modules` preset, because the change under audit can add it.
    ///
    /// # Errors
    ///
    /// Returns each error as [`Self::validate_resolved_boundaries`] does.
    pub fn validate_snapshot_boundaries(
        &self,
        root: &Path,
        snapshot: super::AnalysisSnapshot,
    ) -> Result<(), Vec<super::boundaries::ZoneValidationError>> {
        use super::boundaries::ZoneValidationError;

        let mut errors = self.boundaries.validate_preset(root);
        if snapshot.is_base() {
            errors
                .retain(|error| !matches!(error, ZoneValidationError::PresetModuleRootMissing(_)));
        }
        let mut boundaries = self.boundaries.clone();
        super::boundaries::expand_boundary_preset(&mut boundaries, root);
        let _logical_groups = boundaries.expand_auto_discover(root);

        errors.extend(
            boundaries
                .validate_zone_references()
                .into_iter()
                .map(ZoneValidationError::UnknownZoneReference),
        );
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
#[path = "parsing_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "parsing_read_tests.rs"]
mod read_tests;
