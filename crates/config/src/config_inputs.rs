//! The files and directories that config resolution reads, other than the
//! config file and its `extends` targets.
//!
//! A long-lived process, such as the language server, keeps a resolved config
//! between runs. A snapshot of these inputs tells it when that config is out
//! of date.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use crate::{FallowConfig, external_plugin_source_files};

/// The inputs that [`FallowConfig::resolve`] reads for one project root:
/// the external plugin files, the rule pack files, and the directories that
/// `autoDiscover` lists.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ConfigInputs {
    root: PathBuf,
    plugin_paths: Vec<String>,
    rule_pack_paths: Vec<String>,
    auto_discover_dirs: Vec<PathBuf>,
}

/// The content of [`ConfigInputs`] at one moment. Two snapshots that differ
/// tell that a resolved config can be out of date.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ConfigInputsSnapshot {
    /// Each file with its content, or `None` when the file did not read. The
    /// read is the bounded read of a regular file inside the root.
    files: Vec<(PathBuf, Option<Vec<u8>>)>,
    /// Each directory with the sorted names of its child directories, or
    /// `None` when the directory did not read.
    dirs: Vec<(PathBuf, Option<Vec<OsString>>)>,
}

impl ConfigInputs {
    /// The inputs of `config` for `root`. Take a [`Self::snapshot`] before
    /// [`FallowConfig::resolve`] and one after it: when the two differ, an
    /// input changed while resolution read it.
    #[must_use]
    pub fn new(root: &Path, config: &FallowConfig) -> Self {
        Self {
            root: root.to_path_buf(),
            plugin_paths: config.plugins.clone(),
            rule_pack_paths: config.rule_packs.clone(),
            auto_discover_dirs: config.boundaries.auto_discover_dirs(root),
        }
    }

    /// Read the current content of the inputs.
    #[must_use]
    pub fn snapshot(&self) -> ConfigInputsSnapshot {
        let files = external_plugin_source_files(&self.root, &self.plugin_paths)
            .into_iter()
            .chain(self.rule_pack_paths.iter().map(|path| self.root.join(path)))
            .map(|path| {
                let content = fallow_types::read_guard::read_bytes_inside(
                    &self.root,
                    &path,
                    fallow_types::read_guard::DEFAULT_MAX_READ_BYTES,
                )
                .ok();
                (path, content)
            })
            .collect();
        let dirs = self
            .auto_discover_dirs
            .iter()
            .map(|dir| (dir.clone(), child_dir_names(dir)))
            .collect();
        ConfigInputsSnapshot { files, dirs }
    }
}

fn child_dir_names(dir: &Path) -> Option<Vec<OsString>> {
    let entries = std::fs::read_dir(dir).ok()?;
    let mut names: Vec<OsString> = entries
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_ok_and(|file_type| file_type.is_dir()))
        .map(|entry| entry.file_name())
        .collect();
    names.sort();
    Some(names)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inputs(root: &Path, config: &str) -> ConfigInputs {
        ConfigInputs::new(root, &serde_json::from_str(config).unwrap())
    }

    #[test]
    fn a_plugin_edit_or_a_new_plugin_file_changes_the_snapshot() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join(".fallow/plugins")).unwrap();
        std::fs::write(root.join(".fallow/plugins/a.json"), r#"{"name":"a"}"#).unwrap();
        let inputs = inputs(root, "{}");
        let before = inputs.snapshot();
        assert_eq!(before, inputs.snapshot());

        std::fs::write(root.join(".fallow/plugins/a.json"), r#"{"name":"b"}"#).unwrap();
        let edited = inputs.snapshot();
        assert_ne!(before, edited, "an edit to a plugin file");

        std::fs::write(root.join("fallow-plugin-c.toml"), "name = \"c\"").unwrap();
        assert_ne!(edited, inputs.snapshot(), "a new root plugin file");
    }

    #[test]
    fn configured_plugins_and_rule_packs_are_inputs_also_before_they_exist() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let inputs = inputs(
            root,
            r#"{"plugins":["tools/plugin.json"],"rulePacks":["policy.json"]}"#,
        );
        let before = inputs.snapshot();

        std::fs::create_dir_all(root.join("tools")).unwrap();
        std::fs::write(root.join("tools/plugin.json"), r#"{"name":"a"}"#).unwrap();
        let with_plugin = inputs.snapshot();
        assert_ne!(before, with_plugin, "a configured plugin file appeared");

        std::fs::write(root.join("policy.json"), "{}").unwrap();
        assert_ne!(with_plugin, inputs.snapshot(), "a rule pack appeared");
    }

    #[test]
    fn a_new_child_of_an_auto_discover_dir_changes_the_snapshot() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("src/features/auth")).unwrap();
        let inputs = inputs(
            root,
            r#"{"boundaries":{"zones":[{"name":"features","patterns":[],"autoDiscover":["./src/features/"]}],"rules":[]}}"#,
        );
        let before = inputs.snapshot();

        std::fs::write(root.join("src/features/index.ts"), "").unwrap();
        assert_eq!(before, inputs.snapshot(), "a file is not a zone");
        std::fs::create_dir_all(root.join("src/features/billing")).unwrap();
        assert_ne!(before, inputs.snapshot(), "a new zone directory");
    }

    #[test]
    fn a_preset_auto_discover_dir_is_an_input() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("src/features/auth")).unwrap();
        let inputs = inputs(root, r#"{"boundaries":{"preset":"bulletproof"}}"#);
        let before = inputs.snapshot();

        std::fs::create_dir_all(root.join("src/features/billing")).unwrap();
        assert_ne!(before, inputs.snapshot(), "a new zone directory");
    }

    #[cfg(unix)]
    fn make_fifo(path: &Path) {
        let status = std::process::Command::new("mkfifo")
            .arg(path)
            .status()
            .expect("run mkfifo");
        assert!(status.success(), "mkfifo must create the test pipe");
    }

    #[cfg(unix)]
    #[test]
    #[cfg_attr(miri, ignore = "tempdir is blocked by Miri isolation")]
    fn a_pipe_among_the_inputs_does_not_block_the_snapshot() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        std::fs::create_dir_all(root.join(".fallow/plugins")).unwrap();
        make_fifo(&root.join("pipe"));
        std::os::unix::fs::symlink(root.join("pipe"), root.join(".fallow/plugins/a.json")).unwrap();
        std::os::unix::fs::symlink(root.join("pipe"), root.join("fallow-plugin-b.json")).unwrap();
        let listed = external_plugin_source_files(&root, &[]);
        assert!(listed.is_empty(), "{listed:?}");

        let inputs = inputs(&root, r#"{"plugins":["pipe"],"rulePacks":["pipe"]}"#);
        let (send, receive) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = send.send(inputs.snapshot());
        });
        let snapshot = receive
            .recv_timeout(std::time::Duration::from_secs(10))
            .expect("the snapshot must not wait on a pipe");
        assert!(snapshot.files.iter().all(|(_, content)| content.is_none()));
    }
}
