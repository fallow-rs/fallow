//! The config sources that the loader merges for one side of a change.
//!
//! The chain starts at the Fallow config file that the loader finds from the
//! analysis root and follows each relative `extends` entry, the same way the
//! loader does. The
//! loader merges objects key by key, and an array or a scalar of a later
//! source replaces the value of an earlier source. Thus the value of a key
//! comes from the last source in merge order that sets it.
//!
//! The detector cannot read every source at the base: an `npm:` package, an
//! `https://` URL, a file outside the repository, or a file that git does not
//! track. Such a source is opaque. A key that no source after the last opaque
//! source sets has an unknown value, and the detector does not compare it.

use std::path::{Component, Path, PathBuf};

use rustc_hash::{FxHashMap, FxHashSet};
use serde_json::{Map, Value};

/// The loader stops at the same depth.
const MAX_EXTENDS_DEPTH: usize = 10;

/// The result of one file read on one side of the change.
pub enum ConfigRead {
    /// The text of the file.
    Content(String),
    /// The side has no file at the path.
    Missing,
    /// The side cannot read the file, for example a base path outside the
    /// repository.
    Unreadable,
}

/// The config sources of one side, in merge order.
///
/// The merge order keeps only the last copy of a source. A later copy of the
/// same file answers every lookup that an earlier copy can answer, and a later
/// opaque source hides every key that an earlier opaque source hides. Thus a
/// repeated or diamond `extends` graph stays small.
pub struct ConfigChain {
    /// Sources in merge order, as indices into `values`. `None` is an opaque
    /// source.
    order: Vec<Option<usize>>,
    /// The path and the value, without `extends`, of each local source.
    values: Vec<(PathBuf, Map<String, Value>)>,
    index: FxHashMap<PathBuf, usize>,
    /// The merge order that each resolved source adds, for a reuse.
    resolved: FxHashMap<usize, Vec<Option<usize>>>,
    /// The sources that resolve now, to stop a cycle.
    active: FxHashSet<usize>,
    /// The text of each local file that the chain read, by absolute path.
    texts: FxHashMap<PathBuf, String>,
    /// The root config file, `None` when the side has no config file.
    root: Option<PathBuf>,
}

/// The value of one key across the chain.
pub enum Lookup<'a> {
    /// An opaque source can set the key.
    Unknown,
    /// No source sets the key, so the default applies.
    Unset,
    /// The last source that sets the key.
    Set {
        /// The value of the key.
        value: &'a Value,
        /// The file of the source.
        abs: &'a Path,
    },
}

impl ConfigChain {
    fn empty() -> Self {
        Self {
            order: Vec::new(),
            values: Vec::new(),
            index: FxHashMap::default(),
            resolved: FxHashMap::default(),
            active: FxHashSet::default(),
            texts: FxHashMap::default(),
            root: None,
        }
    }

    /// Resolve the chain of the config file that the loader finds: the first
    /// name in priority order, in `root` and then in each parent directory up
    /// to `stop`. Returns `None` when that file does not parse or cannot be
    /// read, so that no comparison uses a wrong value.
    pub fn resolve(
        root: &Path,
        stop: &Path,
        read: &mut dyn FnMut(&Path) -> ConfigRead,
    ) -> Option<Self> {
        let mut chain = Self::empty();
        let mut dir = Some(root);
        while let Some(current) = dir {
            for name in fallow_config::CONFIG_FILE_NAMES {
                let abs = current.join(name);
                match read(&abs) {
                    ConfigRead::Content(text) => {
                        let value = parse_object(&text)?;
                        chain.texts.insert(abs.clone(), text);
                        chain.root = Some(abs.clone());
                        chain.order = chain.file_order(abs, value, read, 0)?;
                        return Some(chain);
                    }
                    ConfigRead::Missing => {}
                    ConfigRead::Unreadable => return None,
                }
            }
            dir = (current != stop && current.starts_with(stop))
                .then(|| current.parent())
                .flatten();
        }
        Some(chain)
    }

    /// A chain of one config text at `abs`. Every `extends` entry is opaque.
    #[cfg(test)]
    pub fn from_text(abs: &Path, text: &str) -> Option<Self> {
        let mut value = parse_object(text)?;
        let extends = take_extends(&mut value)?;
        let mut chain = Self::empty();
        if !extends.is_empty() {
            chain.order.push(None);
        }
        chain.order.push(Some(0));
        chain.values.push((abs.to_path_buf(), value));
        chain.texts.insert(abs.to_path_buf(), text.to_owned());
        chain.root = Some(abs.to_path_buf());
        Some(chain)
    }

    /// The merge order of one file: its `extends` targets, then the file.
    /// Returns `None` when the `extends` entry has a type that the loader
    /// rejects.
    fn file_order(
        &mut self,
        abs: PathBuf,
        mut value: Map<String, Value>,
        read: &mut dyn FnMut(&Path) -> ConfigRead,
        depth: usize,
    ) -> Option<Vec<Option<usize>>> {
        let extends = take_extends(&mut value)?;
        let dir = abs.parent().map(Path::to_path_buf).unwrap_or_default();
        let id = self.values.len();
        self.values.push((abs.clone(), value));
        self.index.insert(abs, id);
        self.active.insert(id);
        let mut order = Vec::new();
        for entry in extends {
            for source in self.entry_order(&dir, &entry, read, depth + 1) {
                push_last(&mut order, source);
            }
        }
        push_last(&mut order, Some(id));
        self.active.remove(&id);
        self.resolved.insert(id, order.clone());
        Some(order)
    }

    /// The merge order of one `extends` target. A target that the side cannot
    /// read or parse is opaque, never an empty config. The loader rejects a
    /// cycle, so a cycle is opaque too.
    fn entry_order(
        &mut self,
        dir: &Path,
        entry: &str,
        read: &mut dyn FnMut(&Path) -> ConfigRead,
        depth: usize,
    ) -> Vec<Option<usize>> {
        let opaque = vec![None];
        let local = is_relative_entry(entry)
            .then(|| normalize(&dir.join(entry)))
            .filter(|_| depth < MAX_EXTENDS_DEPTH);
        let Some(abs) = local else {
            return opaque;
        };
        if let Some(id) = self.index.get(&abs).copied() {
            if self.active.contains(&id) {
                return opaque;
            }
            return self.resolved.get(&id).cloned().unwrap_or(opaque);
        }
        let text = match read(&abs) {
            ConfigRead::Content(text) => text,
            ConfigRead::Missing | ConfigRead::Unreadable => return opaque,
        };
        let Some(value) = parse_object(&text) else {
            return opaque;
        };
        match self.file_order(abs.clone(), value, read, depth) {
            Some(order) => {
                self.texts.insert(abs, text);
                order
            }
            None => opaque,
        }
    }

    /// The value of the key that `get` reads from one source object.
    pub fn lookup<'a>(
        &'a self,
        get: impl Fn(&'a Map<String, Value>) -> Option<&'a Value>,
    ) -> Lookup<'a> {
        for source in self.order.iter().rev() {
            let Some((abs, value)) = source.and_then(|id| self.values.get(id)) else {
                return Lookup::Unknown;
            };
            if let Some(found) = get(value) {
                return Lookup::Set { value: found, abs };
            }
        }
        Lookup::Unset
    }

    /// Every local file of the chain, as absolute paths.
    pub fn files(&self) -> impl Iterator<Item = &Path> {
        self.texts.keys().map(PathBuf::as_path)
    }

    /// The text of a local file of the chain.
    pub fn text(&self, abs: &Path) -> Option<&str> {
        self.texts.get(abs).map(String::as_str)
    }

    /// The root config file of the side.
    pub fn root(&self) -> Option<&Path> {
        self.root.as_deref()
    }
}

/// Append `source` and drop its earlier copy.
fn push_last(order: &mut Vec<Option<usize>>, source: Option<usize>) {
    order.retain(|existing| *existing != source);
    order.push(source);
}

/// A path entry that the loader resolves from the directory of the file.
fn is_relative_entry(entry: &str) -> bool {
    let lower = entry.to_ascii_lowercase();
    !entry.is_empty()
        && !lower.starts_with("npm:")
        && !lower.starts_with("https://")
        && !lower.starts_with("http://")
        && !fallow_types::path_util::is_absolute_path_any_platform(Path::new(entry))
}

/// Remove `.` and `..` components without a file system access, because the
/// base file does not exist on disk.
fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    out
}

/// The `extends` entries of a source, removed from it. Returns `None` for a
/// type that the loader rejects.
fn take_extends(value: &mut Map<String, Value>) -> Option<Vec<String>> {
    match value.remove("extends") {
        None => Some(Vec::new()),
        Some(Value::String(entry)) => Some(vec![entry]),
        Some(Value::Array(entries)) => entries
            .into_iter()
            .map(|entry| match entry {
                Value::String(entry) => Some(entry),
                _ => None,
            })
            .collect(),
        Some(_) => None,
    }
}

/// Parse a config text as JSONC, else as TOML. The text decides the format,
/// not the file name, so a rename from `.fallowrc.json` to `fallow.toml`
/// still parses the base text. An empty or comment-only text is an empty
/// config.
pub fn parse_object(text: &str) -> Option<Map<String, Value>> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let value = match fallow_config::jsonc::parse_to_value::<Option<Value>>(text) {
        Ok(Some(value)) => value,
        Ok(None) => return Some(Map::new()),
        Err(_) => fallow_config::toml_input::from_str(text).ok()?,
    };
    match value {
        Value::Object(map) => Some(map),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reader(files: &[(&str, &str)]) -> impl FnMut(&Path) -> ConfigRead + use<> {
        let files: FxHashMap<PathBuf, String> = files
            .iter()
            .map(|(path, text)| (PathBuf::from(path), (*text).to_owned()))
            .collect();
        move |abs: &Path| match files.get(abs) {
            Some(text) => ConfigRead::Content(text.clone()),
            None => ConfigRead::Missing,
        }
    }

    fn rule<'a>(chain: &'a ConfigChain, key: &str) -> Option<&'a str> {
        match chain.lookup(|map| map.get("rules")?.get(key)) {
            Lookup::Set { value, .. } => value.as_str(),
            Lookup::Unset => Some("unset"),
            Lookup::Unknown => None,
        }
    }

    #[test]
    fn a_later_source_wins_and_a_relative_entry_resolves_from_its_file() {
        let mut read = reader(&[
            (
                "/r/.fallowrc.json",
                r#"{"extends": ["./cfg/a.json", "./cfg/b.json"], "rules": {"x": "error"}}"#,
            ),
            (
                "/r/cfg/a.json",
                r#"{"extends": "../shared.json", "rules": {"y": "warn"}}"#,
            ),
            ("/r/cfg/b.json", r#"{"rules": {"y": "off"}}"#),
            ("/r/shared.json", r#"{"rules": {"z": "error"}}"#),
        ]);
        let chain =
            ConfigChain::resolve(Path::new("/r"), Path::new("/r"), &mut read).expect("chain");
        assert_eq!(rule(&chain, "x"), Some("error"));
        assert_eq!(rule(&chain, "y"), Some("off"));
        assert_eq!(rule(&chain, "z"), Some("error"));
        assert_eq!(rule(&chain, "w"), Some("unset"));
        assert_eq!(chain.files().count(), 4);
    }

    #[test]
    fn a_package_url_or_missing_target_hides_only_what_it_can_set() {
        for entry in [
            "npm:@acme/config",
            "https://example.com/c.json",
            "./missing.json",
            "/abs.json",
        ] {
            let root = format!(r#"{{"extends": "{entry}", "rules": {{"x": "error"}}}}"#);
            let mut read = reader(&[("/r/.fallowrc.json", &root)]);
            let chain =
                ConfigChain::resolve(Path::new("/r"), Path::new("/r"), &mut read).expect("chain");
            assert_eq!(rule(&chain, "x"), Some("error"), "{entry}");
            assert_eq!(rule(&chain, "y"), None, "{entry}");
        }
    }

    #[test]
    fn a_repeated_extends_graph_stays_small() {
        let mut files = vec![(
            "/r/.fallowrc.json".to_owned(),
            r#"{"extends": ["./c0.json", "./c0.json", "./c0.json"]}"#.to_owned(),
        )];
        for level in 0..8 {
            let next = format!("./c{}.json", level + 1);
            files.push((
                format!("/r/c{level}.json"),
                format!(r#"{{"extends": ["{next}", "{next}", "{next}"], "rules": {{"r{level}": "warn"}}}}"#),
            ));
        }
        files.push((
            "/r/c8.json".to_owned(),
            r#"{"rules": {"x": "error"}}"#.to_owned(),
        ));
        let borrowed: Vec<(&str, &str)> = files
            .iter()
            .map(|(p, t)| (p.as_str(), t.as_str()))
            .collect();
        let mut read = reader(&borrowed);
        let chain =
            ConfigChain::resolve(Path::new("/r"), Path::new("/r"), &mut read).expect("chain");
        assert_eq!(chain.order.len(), 10);
        assert_eq!(rule(&chain, "x"), Some("error"));
        assert_eq!(rule(&chain, "r0"), Some("warn"));
    }

    #[test]
    fn a_reused_source_applies_again_at_its_later_place() {
        let mut read = reader(&[
            (
                "/r/.fallowrc.json",
                r#"{"extends": ["./a.json", "./b.json", "./a.json"]}"#,
            ),
            ("/r/a.json", r#"{"rules": {"x": "error"}}"#),
            ("/r/b.json", r#"{"rules": {"x": "off"}}"#),
        ]);
        let chain =
            ConfigChain::resolve(Path::new("/r"), Path::new("/r"), &mut read).expect("chain");
        assert_eq!(rule(&chain, "x"), Some("error"));
    }

    #[test]
    fn the_config_of_a_parent_directory_up_to_the_repository_root_counts() {
        let mut read = reader(&[("/repo/.fallowrc.json", r#"{"rules": {"x": "error"}}"#)]);
        let chain = ConfigChain::resolve(
            Path::new("/repo/packages/app"),
            Path::new("/repo"),
            &mut read,
        )
        .expect("chain");
        assert_eq!(rule(&chain, "x"), Some("error"));
        let mut read = reader(&[("/.fallowrc.json", r#"{"rules": {"x": "error"}}"#)]);
        let chain = ConfigChain::resolve(Path::new("/repo/app"), Path::new("/repo"), &mut read)
            .expect("chain");
        assert_eq!(rule(&chain, "x"), Some("unset"));
    }

    #[test]
    fn a_cycle_is_opaque() {
        let mut read = reader(&[
            ("/r/.fallowrc.json", r#"{"extends": "./a.json"}"#),
            ("/r/a.json", r#"{"extends": "./.fallowrc.json"}"#),
        ]);
        let chain =
            ConfigChain::resolve(Path::new("/r"), Path::new("/r"), &mut read).expect("chain");
        assert_eq!(rule(&chain, "x"), None);
    }

    #[test]
    fn the_first_config_name_wins_and_no_config_is_empty() {
        let mut read = reader(&[
            ("/r/fallow.toml", "[rules]\nx = \"off\"\n"),
            ("/r/.fallowrc.json", r#"{"rules": {"x": "error"}}"#),
        ]);
        let chain =
            ConfigChain::resolve(Path::new("/r"), Path::new("/r"), &mut read).expect("chain");
        assert_eq!(rule(&chain, "x"), Some("error"));
        let mut empty = reader(&[]);
        let chain =
            ConfigChain::resolve(Path::new("/r"), Path::new("/r"), &mut empty).expect("chain");
        assert_eq!(rule(&chain, "x"), Some("unset"));
        assert!(chain.root().is_none());
    }

    #[test]
    fn a_root_that_does_not_parse_gives_no_chain() {
        let mut read = reader(&[("/r/.fallowrc.json", "{ not json")]);
        assert!(ConfigChain::resolve(Path::new("/r"), Path::new("/r"), &mut read).is_none());
        let mut read = reader(&[("/r/.fallowrc.json", r#"{"extends": 3}"#)]);
        assert!(ConfigChain::resolve(Path::new("/r"), Path::new("/r"), &mut read).is_none());
    }
}
