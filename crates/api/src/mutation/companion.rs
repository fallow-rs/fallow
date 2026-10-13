//! The embedded companion: its files, its install in the cache, and the
//! SHA-256 check before each spawn (decision 11).
//!
//! The canonical source is `tools/mutation-runner/src/`. A generator copies the
//! runtime files to `src/mutation/companion/`, because `include_str!` cannot
//! read outside the published crate.

use std::fs;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

/// One embedded companion file.
#[derive(Debug, Clone, Copy)]
pub struct EmbeddedFile {
    /// The path below the companion directory, with forward slashes.
    pub path: &'static str,
    /// The file text.
    pub text: &'static str,
}

macro_rules! embedded {
    ($($path:literal),+ $(,)?) => {
        &[$(EmbeddedFile {
            path: $path,
            text: include_str!(concat!("companion/", $path)),
        }),+]
    };
}

/// Each file of the companion. A test checks that this list equals the
/// directory.
pub const COMPANION_FILES: &[EmbeddedFile] = embedded!(
    "adapters/vitest.mjs",
    "companion.mjs",
    "generated-protocol.mjs",
    "protocol.mjs",
    "runtime/global.mjs",
    "runtime/vitest-setup.mjs",
);

/// The entry point that the runtime starts.
pub const COMPANION_ENTRY: &str = "companion.mjs";

/// The directory prefix of the runtime hooks that a run directory copies.
const RUNTIME_PREFIX: &str = "runtime/";

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut hex = String::with_capacity(digest.len() * 2);
    for byte in digest {
        use std::fmt::Write as _;
        let _ = write!(hex, "{byte:02x}");
    }
    hex
}

/// The digest of a whole companion: each path and text, length-prefixed, in
/// list order.
pub fn bundle_digest(files: &[EmbeddedFile]) -> String {
    let mut hasher = Sha256::new();
    for file in files {
        hasher.update((file.path.len() as u64).to_le_bytes());
        hasher.update(file.path.as_bytes());
        hasher.update((file.text.len() as u64).to_le_bytes());
        hasher.update(file.text.as_bytes());
    }
    let mut hex = String::with_capacity(64);
    for byte in hasher.finalize() {
        use std::fmt::Write as _;
        let _ = write!(hex, "{byte:02x}");
    }
    hex
}

/// A companion that is written to the cache.
#[derive(Debug, Clone)]
pub struct InstalledCompanion {
    dir: PathBuf,
    files: Vec<(PathBuf, String)>,
}

impl InstalledCompanion {
    /// The companion directory.
    #[cfg(test)]
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// The entry point.
    pub fn entry(&self) -> PathBuf {
        self.dir.join(COMPANION_ENTRY)
    }

    /// Checks the SHA-256 digest of each file against the embedded text.
    ///
    /// # Errors
    ///
    /// Returns the first file that is missing or has a different digest.
    pub fn verify(&self) -> Result<(), String> {
        for (path, expected) in &self.files {
            let bytes = fs::read(path).map_err(|error| {
                format!(
                    "the mutation companion file {} cannot be read: {error}",
                    path.display()
                )
            })?;
            let actual = sha256_hex(&bytes);
            if &actual != expected {
                return Err(format!(
                    "the mutation companion file {} changed after install (sha256 {actual}, expected {expected}); delete {} and run again",
                    path.display(),
                    self.dir.display()
                ));
            }
        }
        Ok(())
    }

    /// The runtime hook files, as `(path below the companion, absolute path)`.
    pub fn runtime_files(&self) -> impl Iterator<Item = (&str, &Path)> {
        self.files.iter().filter_map(|(path, _)| {
            let relative = path.strip_prefix(&self.dir).ok()?.to_str()?;
            relative
                .replace('\\', "/")
                .starts_with(RUNTIME_PREFIX)
                .then_some((relative, path.as_path()))
        })
    }
}

/// Writes `files` to `<mutation_cache>/companion/<bundle digest>/` on first
/// use, then verifies each file.
///
/// The write goes to a temporary directory that is renamed into place, so a
/// crash never leaves a partial companion. An existing directory is never
/// rewritten: a file that changed after install stops the run.
///
/// # Errors
///
/// Returns an IO error or a digest mismatch.
pub fn install(
    mutation_cache: &Path,
    files: &[EmbeddedFile],
) -> Result<InstalledCompanion, String> {
    let parent = mutation_cache.join("companion");
    let dir = parent.join(bundle_digest(files));
    let installed = InstalledCompanion {
        files: files
            .iter()
            .map(|file| (dir.join(file.path), sha256_hex(file.text.as_bytes())))
            .collect(),
        dir: dir.clone(),
    };
    if !dir.is_dir() {
        write_atomically(&parent, &dir, files)?;
    }
    installed.verify()?;
    Ok(installed)
}

fn write_atomically(parent: &Path, dir: &Path, files: &[EmbeddedFile]) -> Result<(), String> {
    let io = |error: std::io::Error| {
        format!(
            "cannot write the mutation companion to {}: {error}",
            dir.display()
        )
    };
    fs::create_dir_all(parent).map_err(io)?;
    let staging = parent.join(format!(
        ".staging-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.as_nanos())
    ));
    let result = (|| {
        for file in files {
            let path = staging.join(file.path);
            if let Some(directory) = path.parent() {
                fs::create_dir_all(directory)?;
            }
            fs::write(&path, file.text)?;
        }
        fs::rename(&staging, dir)
    })();
    if let Err(error) = result {
        let _ = fs::remove_dir_all(&staging);
        // Another process can win the rename. Its directory has the same
        // content, and `verify` checks it.
        if dir.is_dir() {
            return Ok(());
        }
        return Err(io(error));
    }
    Ok(())
}

#[expect(clippy::unwrap_used, reason = "tests unwrap to keep assertions short")]
#[cfg(test)]
mod tests {
    use super::*;

    const FAKE: &[EmbeddedFile] = &[
        EmbeddedFile {
            path: "companion.mjs",
            text: "console.log('entry');\n",
        },
        EmbeddedFile {
            path: "runtime/global.mjs",
            text: "export const g = 1;\n",
        },
    ];

    fn listed_directory(dir: &Path, prefix: &str, out: &mut Vec<String>) {
        for entry in fs::read_dir(dir).expect("readable directory") {
            let entry = entry.expect("entry");
            let name = entry.file_name().to_string_lossy().into_owned();
            let path = format!("{prefix}{name}");
            if entry.file_type().expect("type").is_dir() {
                listed_directory(&entry.path(), &format!("{path}/"), out);
            } else {
                out.push(path);
            }
        }
    }

    #[test]
    fn the_embedded_list_is_every_file_of_the_companion_directory() {
        let dir = Path::new(
            &std::env::var_os("CARGO_MANIFEST_DIR").expect("cargo sets CARGO_MANIFEST_DIR"),
        )
        .join("src/mutation/companion");
        let mut on_disk = Vec::new();
        listed_directory(&dir, "", &mut on_disk);
        on_disk.sort();
        let embedded: Vec<String> = COMPANION_FILES
            .iter()
            .map(|file| file.path.to_owned())
            .collect();
        assert_eq!(embedded, on_disk);
        assert!(embedded.iter().any(|path| path == COMPANION_ENTRY));
    }

    #[test]
    fn install_writes_each_file_under_the_bundle_digest_and_verifies_it() {
        let temp = tempfile::tempdir().expect("temp dir");
        let installed = install(temp.path(), FAKE).expect("install");
        assert_eq!(
            installed.dir(),
            temp.path().join("companion").join(bundle_digest(FAKE))
        );
        assert_eq!(
            fs::read_to_string(installed.entry()).expect("entry"),
            "console.log('entry');\n"
        );
        let runtime: Vec<&str> = installed.runtime_files().map(|(path, _)| path).collect();
        assert_eq!(runtime, vec!["runtime/global.mjs"]);
        assert!(installed.verify().is_ok());
    }

    #[test]
    fn a_changed_file_fails_the_check_and_a_new_install_does_not_repair_it() {
        let temp = tempfile::tempdir().expect("temp dir");
        let installed = install(temp.path(), FAKE).expect("install");
        fs::write(
            installed.dir().join("runtime/global.mjs"),
            "export const g = 2;\n",
        )
        .expect("tamper");
        let error = installed.verify().unwrap_err();
        assert!(error.contains("runtime/global.mjs"), "{error}");
        assert!(error.contains("changed after install"), "{error}");
        assert!(install(temp.path(), FAKE).is_err());
    }

    #[test]
    fn a_missing_file_fails_the_check() {
        let temp = tempfile::tempdir().expect("temp dir");
        let installed = install(temp.path(), FAKE).expect("install");
        fs::remove_file(installed.entry()).expect("remove");
        assert!(installed.verify().unwrap_err().contains("cannot be read"));
    }

    #[test]
    fn a_different_bundle_gets_a_different_directory() {
        let changed = [
            FAKE[0],
            EmbeddedFile {
                path: "runtime/global.mjs",
                text: "export const g = 3;\n",
            },
        ];
        assert_ne!(bundle_digest(FAKE), bundle_digest(&changed));
        let moved = [
            EmbeddedFile {
                path: "companion.mjs",
                text: "console.log('entry');\nexport const g = 1;\n",
            },
            EmbeddedFile {
                path: "runtime/global.mjs",
                text: "",
            },
        ];
        assert_ne!(
            bundle_digest(FAKE),
            bundle_digest(&moved),
            "text moved between files changes the digest"
        );
    }
}
