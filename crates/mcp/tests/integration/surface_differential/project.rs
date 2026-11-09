//! A generated project on disk, and the seeded choices that build it.

use std::path::{Path, PathBuf};
use std::process::Command;

/// A small SplitMix64 generator, so a seed always gives the same project.
pub struct Rng(u64);

impl Rng {
    pub const fn new(seed: u64) -> Self {
        Self(seed)
    }

    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// A number in `low..=high`.
    pub fn range(&mut self, low: usize, high: usize) -> usize {
        low + usize::try_from(self.next() % (high - low + 1) as u64).expect("small range")
    }

    pub fn chance(&mut self, percent: u64) -> bool {
        self.next() % 100 < percent
    }
}

/// The files of a generated project, and the names that the oracle asks
/// each surface about.
#[derive(Debug, Default)]
pub struct Project {
    /// The files, as `(path, contents)`.
    pub files: Vec<(String, String)>,
    /// Each declared dependency, as `(package name, manifest path)`.
    pub declared: Vec<(String, String)>,
    /// Exports that an import uses, as `(file, export name)`.
    pub used_exports: Vec<(String, String)>,
}

impl Project {
    pub fn file(&mut self, path: impl Into<String>, contents: impl Into<String>) {
        self.files.push((path.into(), contents.into()));
    }

    pub fn declare(&mut self, name: &str, manifest: &str) {
        self.declared.push((name.to_owned(), manifest.to_owned()));
    }

    /// The declared package names, each name once, in sorted order.
    pub fn dependency_names(&self) -> Vec<String> {
        let mut names: Vec<String> = self.declared.iter().map(|(name, _)| name.clone()).collect();
        names.sort();
        names.dedup();
        names
    }

    /// The manifests that declare `name`, in sorted order.
    pub fn manifests_of(&self, name: &str) -> Vec<String> {
        let mut manifests: Vec<String> = self
            .declared
            .iter()
            .filter(|(declared, _)| declared == name)
            .map(|(_, manifest)| manifest.clone())
            .collect();
        manifests.sort();
        manifests
    }
}

/// A project on disk with two commits. The base commit holds only a README,
/// so a scope of "changed since the base" covers every source file.
pub struct Checkout {
    _dir: tempfile::TempDir,
    pub root: PathBuf,
    pub base: String,
}

pub fn git(root: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args(args)
        .current_dir(root)
        .env("GIT_AUTHOR_NAME", "oracle")
        .env("GIT_AUTHOR_EMAIL", "oracle@example.com")
        .env("GIT_COMMITTER_NAME", "oracle")
        .env("GIT_COMMITTER_EMAIL", "oracle@example.com")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .output()
        .expect("run git");
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .expect("git output is UTF-8")
        .trim()
        .to_owned()
}

pub fn write_files(root: &Path, files: &[(String, String)]) {
    for (path, contents) in files {
        let path = root.join(path);
        std::fs::create_dir_all(path.parent().expect("file has a parent")).expect("create dir");
        std::fs::write(path, contents).expect("write project file");
    }
}

/// Write `project` as two commits in a new temporary directory.
pub fn materialize(project: &Project) -> Checkout {
    let dir = tempfile::tempdir().expect("project dir");
    // The LSP reports `file://` URIs of the canonical path, and macOS
    // links the temporary directory through `/private`.
    let root = std::fs::canonicalize(dir.path()).expect("canonical project dir");
    git(&root, &["init", "-q", "-b", "main"]);
    std::fs::write(root.join("README.md"), "generated project\n").expect("README");
    git(&root, &["add", "-A"]);
    git(&root, &["commit", "-q", "--no-gpg-sign", "-m", "base"]);
    let base = git(&root, &["rev-parse", "HEAD"]);
    write_files(&root, &project.files);
    git(&root, &["add", "-A"]);
    git(&root, &["commit", "-q", "--no-gpg-sign", "-m", "sources"]);
    Checkout {
        _dir: dir,
        root,
        base,
    }
}
