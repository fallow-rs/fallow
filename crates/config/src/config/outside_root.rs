//! Error text for a config whose `extends` chain has a file outside the
//! project.
//!
//! An `extends` entry can name a file outside the project, for example a
//! shared config above the git top level. The load still reads that file,
//! but its error text names the file and a position only. A field name, a
//! value, or the message of a fallow deserializer from that file is not
//! shown. The files inside the project keep the full error text.
//!
//! A discovered config file that is a link out of the project is allowed
//! only when git does not track it, see [`git_tracks`].

use std::path::{Path, PathBuf};

use super::FallowConfig;

/// One local file of an `extends` walk with its own value, before the merge.
pub(super) struct LocalValue {
    /// The canonical path of the file.
    pub(super) path: PathBuf,
    /// Whether the file is inside the project.
    pub(super) inside: bool,
    /// The value of the file without its `extends` key.
    pub(super) value: serde_json::Value,
}

impl LocalValue {
    pub(super) fn new(path: &Path, inside: bool, value: &serde_json::Value) -> Self {
        Self {
            path: path.to_path_buf(),
            inside,
            value: value.clone(),
        }
    }
}

/// Whether the walk read a local file outside the project.
pub(super) fn has_outside(values: &[LocalValue]) -> bool {
    values.iter().any(|value| !value.inside)
}

/// The error of a merged config that does not deserialize, when the walk
/// read a file outside the project.
///
/// Each file inside the project deserializes alone first, and the first one
/// that fails gives the full error text. When all of them pass, the error
/// names the files outside the project only.
pub(super) fn merged_error(path: &Path, values: &[LocalValue]) -> miette::Report {
    for value in values.iter().filter(|value| value.inside) {
        if let Err(error) = serde_json::from_value::<FallowConfig>(value.value.clone()) {
            return miette::miette!(
                "{} in {}",
                crate::parse_error::describe_json(&error),
                value.path.display()
            );
        }
    }
    let outside: Vec<String> = values
        .iter()
        .filter(|value| !value.inside)
        .map(|value| value.path.display().to_string())
        .collect();
    miette::miette!(
        "invalid config in {}: a file of its extends chain outside the project does not match the config schema ({}). The text of a file outside the project is not shown",
        path.display(),
        outside.join(", ")
    )
}

/// A function that tells whether git tracks the entry at a path:
/// `Some(true)` for a tracked entry, `Some(false)` for an entry of a git
/// checkout that git does not track, and `None` when no git checkout holds
/// the entry or git cannot tell.
pub type TrackedLinkProbe = fn(&Path) -> Option<bool>;

static TRACKED_LINK_PROBE: std::sync::OnceLock<TrackedLinkProbe> = std::sync::OnceLock::new();

/// Set the probe that config discovery asks about a config file link out of
/// the project. The config crate starts no process, so the host that owns
/// the git process wrapper sets it once. The first call wins. Without a
/// probe, every such link is refused.
pub fn set_tracked_link_probe(probe: TrackedLinkProbe) {
    let _ = TRACKED_LINK_PROBE.set(probe);
}

/// What the probe of [`set_tracked_link_probe`] tells about `path`, or
/// `None` when no probe is set.
pub(super) fn git_tracks(path: &Path) -> Option<bool> {
    TRACKED_LINK_PROBE.get().and_then(|probe| probe(path))
}
