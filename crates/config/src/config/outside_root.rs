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

use rustc_hash::FxHashSet;

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

/// The config keys that a local file outside the project sets, kept after
/// the merge so that the checks of the merged config can leave out values
/// from that file.
///
/// A key counts when any file outside the project sets it, also when a file
/// inside the project sets it again later in the merge. A check then names
/// the key and the outside files, never the value. That can hide a value
/// that came from the project, but never shows one from outside it.
#[derive(Debug, Clone, Default)]
pub struct OutsideKeys {
    /// Each dotted key path that an outside file sets, also each object key
    /// on the way to a value.
    paths: FxHashSet<String>,
    /// The dotted key paths whose value in an outside file is not an object.
    leaves: FxHashSet<String>,
    /// The outside files, in read order.
    files: Vec<PathBuf>,
}

impl OutsideKeys {
    /// The keys of the files of `values` that are outside the project.
    pub(super) fn from_values(values: &[LocalValue]) -> Self {
        let mut keys = Self::default();
        for value in values.iter().filter(|value| !value.inside) {
            keys.files.push(value.path.clone());
            keys.collect(String::new(), &value.value);
        }
        keys
    }

    fn collect(&mut self, prefix: String, value: &serde_json::Value) {
        let Some(map) = value.as_object() else {
            if !prefix.is_empty() {
                self.leaves.insert(prefix);
            }
            return;
        };
        for (key, child) in map {
            let path = if prefix.is_empty() {
                key.clone()
            } else {
                format!("{prefix}.{key}")
            };
            self.paths.insert(path.clone());
            self.collect(path, child);
        }
    }

    /// Whether a value of the config field `field` can come from a file
    /// outside the project. `field` is a dotted key path, and the text from
    /// the first `[` or space on is ignored, so `health.thresholdOverrides[0]`
    /// and `workspaces.settings (key)` work.
    pub(super) fn covers(&self, field: &str) -> bool {
        let key = field
            .split(['[', ' ', ':'])
            .next()
            .unwrap_or(field)
            .trim_end_matches('.');
        if key.is_empty() || self.files.is_empty() {
            return false;
        }
        if self.paths.contains(key) {
            return true;
        }
        let mut end = key.len();
        while let Some(dot) = key[..end].rfind('.') {
            if self.leaves.contains(&key[..dot]) {
                return true;
            }
            end = dot;
        }
        false
    }

    /// The outside files as one display text.
    pub(super) fn file_list(&self) -> String {
        self.files
            .iter()
            .map(|path| path.display().to_string())
            .collect::<Vec<_>>()
            .join(", ")
    }

    /// `message` of a check of the merged config, or a text that names its
    /// field and the outside files only when the value can come from a file
    /// outside the project. The field is the text of `message` before the
    /// first space or `:`.
    pub(super) fn redact(&self, message: String) -> String {
        let field = message
            .split([' ', ':'])
            .next()
            .unwrap_or_default()
            .to_owned();
        if self.covers(&field) {
            self.field_message(&field)
        } else {
            message
        }
    }

    /// The text for a value of `field` that is not valid and can come from a
    /// file outside the project.
    pub(super) fn field_message(&self, field: &str) -> String {
        format!(
            "{field}: a value from a config file outside the project ({}) is not valid. \
             The text of a file outside the project is not shown",
            self.file_list()
        )
    }
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
