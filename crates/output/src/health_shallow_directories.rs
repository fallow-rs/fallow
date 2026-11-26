//! Shallow directories section of the health report.

/// Directories whose interface is as large as what they hide.
///
/// A directory is a module: files outside it use its interface items, and it
/// keeps the rest inside. A deep directory hides much code behind a small
/// interface. A shallow directory shows almost all that it has, so each
/// caller must learn many names and gets little in return.
///
/// The section is advisory: it has no health score weight and never gates.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ShallowDirectoriesReport {
    /// Minimum number of shown interface items for a shallow directory
    /// (`health.shallowDirectories.minInterface`).
    pub min_interface: u32,
    /// Shallow directories in scope before `--top` cuts `directories`.
    pub total: usize,
    /// Shallow directories, sorted by `shown_count` (descending), then by
    /// `path`.
    pub directories: Vec<ShallowDirectoryFinding>,
}

/// One shallow directory.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ShallowDirectoryFinding {
    /// The directory, relative to the project root.
    #[serde(serialize_with = "fallow_types::serde_path::serialize")]
    pub path: std::path::PathBuf,
    /// The file that represents the directory in file-based formats (SARIF,
    /// CodeClimate): the `index` barrel of the directory when it has one,
    /// else the first source file by path. Relative to the project root.
    #[serde(serialize_with = "fallow_types::serde_path::serialize")]
    pub anchor_file: std::path::PathBuf,
    /// Interface items of the directory that at least one file outside the
    /// directory uses. An interface item is a module-scope function or a
    /// class. A type-only export, a constant and an enum are not items.
    pub shown_count: u32,
    /// Files of the directory that declare at least one shown item.
    pub shown_files: u32,
    /// Function items and classes of the directory that no file outside the
    /// directory uses.
    pub hidden_count: u32,
    /// The shown items with the most outside users, at most 7 (the cap of the
    /// engine). Sorted by
    /// `outside_files` (descending), then by `name`, then by `path`.
    pub items: Vec<ShallowDirectoryItem>,
    /// Shown items that `items` leaves out.
    pub more_items: u32,
    /// Machine-actionable hints. The list always starts with
    /// `narrow-interface`.
    pub actions: Vec<ShallowDirectoryAction>,
}

/// One shown interface item of a shallow directory.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ShallowDirectoryItem {
    /// The exported name. For a default export: the local binding name, else
    /// the file stem.
    pub name: String,
    /// True for a default export.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub is_default: bool,
    /// The file that declares the item, relative to the project root.
    #[serde(serialize_with = "fallow_types::serde_path::serialize")]
    pub path: std::path::PathBuf,
    /// 1-based line of the export.
    pub line: u32,
    /// Whether the item is a function or a class.
    pub kind: ShallowDirectoryItemKind,
    /// Files outside the directory that use the item.
    pub outside_files: u32,
    /// Public members of a class item. Absent for a function.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub members: Option<u32>,
}

/// The kind of a shown interface item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum ShallowDirectoryItemKind {
    /// A module-scope function: a declaration, or a binding that holds a
    /// function expression or an arrow function.
    Function,
    /// A class.
    Class,
}

/// A hint for one shallow directory.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ShallowDirectoryAction {
    /// The action kind.
    #[serde(rename = "type")]
    pub kind: ShallowDirectoryActionType,
    /// Always `false`: fallow does not change the code of a directory.
    pub auto_fixable: bool,
    /// What the action does.
    pub description: String,
    /// For `ignore-directory`: the config key that takes the pattern,
    /// `health.shallowDirectories.ignore`. Absent for other actions.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub config_key: Option<String>,
    /// For `ignore-directory`: the glob pattern to add to `config_key`. The
    /// pattern escapes the glob characters of the directory path. Absent for
    /// other actions.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
}

/// The kind of a shallow directory action.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum ShallowDirectoryActionType {
    /// Show fewer items: move the logic behind a smaller interface, or merge
    /// the directory into the code that calls it.
    NarrowInterface,
    /// Add the directory to `health.shallowDirectories.ignore`.
    IgnoreDirectory,
}

/// The config key of the `ignore-directory` action.
const IGNORE_CONFIG_KEY: &str = "health.shallowDirectories.ignore";

impl ShallowDirectoryFinding {
    /// Build the `actions` list. `ignore_pattern` is the glob pattern that
    /// matches the directory, relative to the root.
    #[must_use]
    pub fn actions_for(ignore_pattern: &str) -> Vec<ShallowDirectoryAction> {
        vec![
            ShallowDirectoryAction {
                kind: ShallowDirectoryActionType::NarrowInterface,
                auto_fixable: false,
                description: "Move the logic behind fewer interface items, or merge the \
                              directory into the code that calls it. To keep the interface \
                              narrow, see https://fallow.tools/docs/guides/keep-a-module-deep/"
                    .to_string(),
                config_key: None,
                value: None,
            },
            ShallowDirectoryAction {
                kind: ShallowDirectoryActionType::IgnoreDirectory,
                auto_fixable: false,
                description: "Exclude this directory when its wide interface is on purpose"
                    .to_string(),
                config_key: Some(IGNORE_CONFIG_KEY.to_string()),
                value: Some(ignore_pattern.to_string()),
            },
        ]
    }
}
