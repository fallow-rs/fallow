//! Config values that name a module request instead of project files.

use std::path::{Path, PathBuf};

use super::config_parser;

/// The module request a config value names, or `None` when the value names a
/// file or a pattern over project files.
///
/// A bundler resolves a value without a leading `./`, `../` or `/` and without a
/// source extension through module resolution, so it names a package. Both
/// Module Federation `exposes` targets and bundler `entry` values are read this
/// way. A value carrying glob syntax is a path in every case: no module
/// resolution accepts a glob, so `src/pages/**` stays an entry pattern. A
/// resource query is not part of the request, so it is dropped before both
/// tests and before the package name is taken.
pub(super) fn module_request(value: &str) -> Option<&str> {
    let request = strip_resource_query(value);
    (config_parser::is_package_specifier(request)
        && !has_glob_syntax(request)
        && !has_source_extension(request))
    .then_some(request)
}

/// Drop a trailing resource query from a config value.
///
/// A bundler hands everything after the first `?` to the loader, so the standard
/// hot-reload entry `webpack-hot-middleware/client?reload=true` names the
/// package's `client` module. A `?` is also the single-character glob wildcard,
/// so what follows it decides: `reload=true` is a query, the `.ts` of
/// `src/pag?.ts` is not.
fn strip_resource_query(value: &str) -> &str {
    match value.split_once('?') {
        Some((request, query)) if is_resource_query(query) => request,
        _ => value,
    }
}

/// Whether a string is an `&`-separated list of `key` or `key=value` pairs whose
/// keys read like identifiers.
fn is_resource_query(query: &str) -> bool {
    !query.is_empty()
        && query.split('&').all(|pair| {
            let key = pair.split_once('=').map_or(pair, |(key, _)| key);
            key.starts_with(|first: char| first.is_ascii_alphanumeric() || first == '_')
                && key
                    .chars()
                    .all(|char| char.is_ascii_alphanumeric() || matches!(char, '_' | '-' | '.'))
        })
}

/// Whether a config value carries glob metacharacters, which makes it a pattern
/// over project files rather than a single path or module request.
pub(super) fn has_glob_syntax(value: &str) -> bool {
    value.contains('*') || value.contains('?') || value.contains('[') || value.contains('{')
}

/// Whether a config value carries an extension discovery analyzes. Discovery's
/// own extension set decides, so a value naming a file type discovery does not
/// analyze stays a module request.
pub(super) fn has_source_extension(value: &str) -> bool {
    Path::new(value)
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| {
            crate::discover::SOURCE_EXTENSIONS.contains(&ext.to_ascii_lowercase().as_str())
        })
}

/// Whether a module specifier from a config value names a local module: a
/// relative or absolute path, such as `./plugin.js`.
pub(super) fn is_local_module_specifier(specifier: &str) -> bool {
    specifier.starts_with('.') || Path::new(specifier).is_absolute()
}

/// Resolve a local module the way Node resolves a request: the file itself,
/// the file with a source extension, then the index file of a directory.
/// Without a match, the joined path is returned.
pub(super) fn resolve_local_module(config_dir: &Path, specifier: &str) -> PathBuf {
    let base = config_parser::lexical_normalize(&config_dir.join(specifier));
    if base.is_file() {
        return base;
    }
    let mut candidates = crate::discover::SOURCE_EXTENSIONS
        .iter()
        .map(|ext| {
            let mut with_ext = base.clone().into_os_string();
            with_ext.push(".");
            with_ext.push(ext);
            PathBuf::from(with_ext)
        })
        .chain(
            crate::discover::SOURCE_EXTENSIONS
                .iter()
                .map(|ext| base.join(format!("index.{ext}"))),
        );
    candidates.find(|path| path.is_file()).unwrap_or(base)
}
