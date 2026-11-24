//! The config file names that discovery reads, and the near-miss names that
//! a run reports because discovery does not read them.

/// Config file names the loader accepts, in discovery priority order.
///
/// Public because surfaces outside config loading have to react to the same
/// set: the language server registers a file watcher per name, so a name added
/// here reaches the editor without a second list to keep in step.
pub const CONFIG_FILE_NAMES: &[&str] = &[
    ".fallowrc.json",
    ".fallowrc.jsonc",
    "fallow.toml",
    ".fallow.toml",
];

/// File names that look like a fallow config but that the loader does not
/// read. Discovery skips them, so a run reports a notice when the project
/// root has one of them and no name of [`CONFIG_FILE_NAMES`]. The language
/// server watches them too, so the notice stays current in the editor.
///
/// An explicit config path can still name `fallow.json`, `fallow.jsonc`,
/// `fallow.config.json`, `fallow.config.jsonc`, `.fallow.json` or
/// `.fallow.jsonc`: the loader picks the format from the extension.
pub const UNRECOGNIZED_CONFIG_FILE_NAMES: &[&str] = &[
    "fallow.json",
    "fallow.jsonc",
    "fallow.yaml",
    "fallow.yml",
    "fallow.config.json",
    "fallow.config.jsonc",
    "fallow.config.toml",
    "fallow.config.js",
    "fallow.config.mjs",
    "fallow.config.ts",
    ".fallowrc",
    ".fallowrc.yaml",
    ".fallowrc.yml",
    ".fallowrc.toml",
    ".fallow.json",
    ".fallow.jsonc",
];

/// Whether `name` is a config file name of either list. A surface that
/// reacts to a config change, such as the language server, uses this one
/// check, so a created near-miss file also refreshes its notice.
#[must_use]
pub fn config_file_name(name: &str) -> bool {
    CONFIG_FILE_NAMES.contains(&name) || UNRECOGNIZED_CONFIG_FILE_NAMES.contains(&name)
}
