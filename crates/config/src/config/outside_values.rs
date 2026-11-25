//! The config file paths whose value comes from a config file outside the
//! project, kept by project root for the error text of later reads.
//!
//! A read of the `codeowners` file, of the `health.coverage` data or of the
//! `cache.dir` setting happens long after the config load, in code that has
//! only the path. When the value comes from a config file outside the
//! project, the error text of that read names the key and the outside
//! files, not the value. See [`super::outside_root::OutsideKeys`].

use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use rustc_hash::FxHashMap;

use super::FallowConfig;

/// One config file path from an outside file.
struct OutsideValue {
    field: &'static str,
    value: PathBuf,
    files: String,
}

static OUTSIDE_VALUES: OnceLock<Mutex<FxHashMap<PathBuf, Vec<OutsideValue>>>> = OnceLock::new();

fn root_key(root: &Path) -> PathBuf {
    dunce::canonicalize(root).unwrap_or_else(|_| root.to_path_buf())
}

impl FallowConfig {
    /// Keep the config file paths of this config that come from a config
    /// file outside the project, for the project at `root`. Each call
    /// replaces what an earlier load of the same root kept.
    pub fn record_outside_values(&self, root: &Path) {
        let files = self.outside_keys.file_list();
        let mut values: Vec<OutsideValue> = self
            .file_path_keys()
            .into_iter()
            .filter_map(|(value, field)| value.map(|value| (PathBuf::from(value), field)))
            .chain(self.cache.dir.clone().map(|dir| (dir, "cache.dir")))
            .filter(|(_, field)| self.outside_keys.covers(field))
            .map(|(value, field)| OutsideValue {
                field,
                value,
                files: files.clone(),
            })
            .collect();
        let registry = OUTSIDE_VALUES.get_or_init(Mutex::default);
        if let Ok(mut map) = registry.lock() {
            if values.is_empty() {
                map.remove(&root_key(root));
            } else {
                values.shrink_to_fit();
                map.insert(root_key(root), values);
            }
        }
    }
}

/// The outside config files that set the config key `field` of the project
/// at `root` to `path`, as one display text. A caller whose read of `path`
/// failed then names the key and these files, not the value. `None` when
/// the value comes from the project, or when `path` is not the config
/// value, for example a CLI flag value.
#[must_use]
pub fn outside_value_files(root: &Path, field: &str, path: &Path) -> Option<String> {
    let normalized = |path: &Path| {
        let joined = root.join(path);
        let absolute = std::path::absolute(&joined).unwrap_or(joined);
        fallow_types::path_guard::lexically_normalized(&absolute)
    };
    let map = OUTSIDE_VALUES.get()?.lock().ok()?;
    map.get(&root_key(root))?
        .iter()
        .find(|entry| entry.field == field && normalized(&entry.value) == normalized(path))
        .map(|entry| entry.files.clone())
}

/// The error text for a read of the file that the config key `field` names,
/// when [`outside_value_files`] finds that the value comes from outside.
#[must_use]
pub fn outside_value_error(root: &Path, field: &str, path: &Path) -> Option<String> {
    outside_value_files(root, field, path).map(|files| {
        format!(
            "{field}: the file that a config file outside the project ({files}) names does not \
             load. The text of a file outside the project is not shown"
        )
    })
}
