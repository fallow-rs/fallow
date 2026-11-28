//! The target of one `extends` entry of a local config file, before the
//! load reads it.

use std::path::{Path, PathBuf};

use fallow_types::path_util::is_absolute_path_any_platform;

use super::{
    HTTP_PREFIX, HTTPS_PREFIX, NPM_PREFIX, remote_config_display, resolve_npm_package,
    starts_with_url_prefix,
};

/// One `extends` entry of the local config file `path`.
#[derive(Clone, Copy)]
pub(super) struct LocalExtendsEntry<'a> {
    pub(super) path: &'a Path,
    pub(super) config_dir: &'a Path,
    pub(super) entry: &'a str,
    pub(super) sealed: bool,
    pub(super) sealed_dir_canonical: Option<&'a Path>,
}

/// The source that one `extends` entry of a local file names.
pub(super) enum ExtendsTarget<'a> {
    Remote(&'a str),
    Local(PathBuf),
}

/// The source that the `extends` entry of `input` names, before a read.
pub(super) fn extends_target(
    input: LocalExtendsEntry<'_>,
) -> Result<ExtendsTarget<'_>, miette::Report> {
    let LocalExtendsEntry {
        path,
        config_dir,
        entry,
        sealed,
        sealed_dir_canonical,
    } = input;
    if starts_with_url_prefix(entry, HTTPS_PREFIX) {
        reject_sealed_remote_extends(path, entry, sealed, "URL")?;
        return Ok(ExtendsTarget::Remote(entry));
    }
    if starts_with_url_prefix(entry, HTTP_PREFIX) {
        let entry_display = remote_config_display(entry);
        return Err(miette::miette!(
            "URL extends must use https://, got http:// URL '{}' (in {}). \
             Change the URL to use https:// instead",
            entry_display,
            path.display()
        ));
    }
    if let Some(npm_specifier) = entry.strip_prefix(NPM_PREFIX) {
        reject_sealed_remote_extends(path, entry, sealed, "npm")?;
        return resolve_npm_package(config_dir, npm_specifier, path).map(ExtendsTarget::Local);
    }
    if is_absolute_path_any_platform(Path::new(entry)) {
        return Err(miette::miette!(
            "extends paths must be relative, got absolute path: {} (in {})",
            entry,
            path.display()
        ));
    }
    let resolved_path = config_dir.join(entry);
    if !resolved_path.exists() {
        return Err(miette::miette!(
            "Extended config file not found: {} (referenced from {})",
            resolved_path.display(),
            path.display()
        ));
    }
    validate_sealed_relative_extends(path, entry, &resolved_path, sealed_dir_canonical)?;
    Ok(ExtendsTarget::Local(resolved_path))
}

/// The error of an `extends` entry of the local file `path` outside the
/// project that does not resolve or is refused. It names the file only.
pub(super) fn outside_extends_error(path: &Path) -> miette::Report {
    miette::miette!(
        "an extends entry of {}, a config file outside the project, does not resolve or is \
         not allowed. The text of a file outside the project is not shown",
        path.display()
    )
}

fn reject_sealed_remote_extends(
    path: &Path,
    entry: &str,
    sealed: bool,
    kind: &str,
) -> Result<(), miette::Report> {
    if sealed {
        let entry_display = remote_config_display(entry);
        Err(miette::miette!(
            "'sealed: true' config at {} rejects {} extends '{}'. \
             Sealed configs only allow file-relative extends within \
             the config's directory",
            path.display(),
            kind,
            entry_display
        ))
    } else {
        Ok(())
    }
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

/// The most distinct `extends` entries that one config file can list. Real
/// configs list a few, and each distinct entry is one more full merge.
pub(super) const MAX_EXTENDS_ENTRIES: usize = 64;

/// `entries` with each repeated key at its last position only, in order.
///
/// The load merges the entries in order and a later entry overrides an
/// earlier one. A repeated entry merges its full value again, so only its
/// last position changes the result, and the earlier merges are wasted work.
pub(super) fn keep_last_entries<T, K: Eq + std::hash::Hash>(
    entries: Vec<T>,
    key: impl Fn(&T) -> K,
) -> Vec<T> {
    let mut seen = rustc_hash::FxHashSet::default();
    let mut kept: Vec<T> = entries
        .into_iter()
        .rev()
        .filter(|entry| seen.insert(key(entry)))
        .collect();
    kept.reverse();
    kept
}

/// Refuse a config that lists more than [`MAX_EXTENDS_ENTRIES`] distinct
/// `extends` entries.
pub(super) fn check_extends_count(count: usize, source: &str) -> Result<(), miette::Report> {
    if count > MAX_EXTENDS_ENTRIES {
        return Err(miette::miette!(
            "{source} lists {count} distinct extends entries; the limit is {MAX_EXTENDS_ENTRIES}"
        ));
    }
    Ok(())
}

/// The key that identifies the source of an `extends` target: the URL
/// without the parts that do not change the fetched file, or the real path
/// of a local file.
pub(super) fn extends_target_key(target: &ExtendsTarget<'_>) -> super::ConfigResourceId {
    match target {
        ExtendsTarget::Remote(url) => {
            super::ConfigResourceId::Remote(super::normalize_url_for_dedup(url))
        }
        ExtendsTarget::Local(path) => super::ConfigResourceId::Local(
            dunce::canonicalize(path).unwrap_or_else(|_| path.clone()),
        ),
    }
}

/// Extract the `extends` array from a parsed JSON config value.
///
/// Fails loud on malformed values: the key is removed before
/// deserialization, so `FallowConfig`'s `deny_unknown_fields` never sees a
/// bad `extends` and the base config would otherwise go unmerged silently.
pub(super) fn extract_extends(
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
