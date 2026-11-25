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
