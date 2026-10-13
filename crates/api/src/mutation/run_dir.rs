//! The run directory (`protocol.md` section 6): the instrumented files, the
//! path map, and the runtime hooks. Project files never change.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Component, Path, PathBuf};

use sha2::{Digest, Sha256};

use super::companion::InstalledCompanion;

/// One instrumented file to write.
#[derive(Debug, Clone, Copy)]
pub struct RunFile<'a> {
    /// The root-relative path, with forward slashes.
    pub path: &'a str,
    /// The instrumented source text.
    pub code: &'a str,
}

/// A written run directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunDir {
    pub dir: PathBuf,
    /// Root-relative source path to run-dir-relative instrumented path.
    pub path_map: BTreeMap<String, String>,
}

/// True when `path` is a plain relative path with no parent or root part.
fn is_contained(path: &str) -> bool {
    !path.is_empty()
        && Path::new(path)
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
}

fn content_hash(files: &[RunFile<'_>]) -> String {
    let mut hasher = Sha256::new();
    for file in files {
        hasher.update((file.path.len() as u64).to_le_bytes());
        hasher.update(file.path.as_bytes());
        hasher.update((file.code.len() as u64).to_le_bytes());
        hasher.update(file.code.as_bytes());
    }
    let mut hex = String::with_capacity(64);
    for byte in hasher.finalize() {
        use std::fmt::Write as _;
        let _ = write!(hex, "{byte:02x}");
    }
    hex
}

/// Writes `<mutation_cache>/runs/<content hash>/`.
///
/// The directory is written again on each run, so a stale directory with the
/// same hash cannot serve other text.
///
/// # Errors
///
/// Returns an error for a path that leaves the run directory, or an IO error.
pub fn write_run_dir(
    mutation_cache: &Path,
    files: &[RunFile<'_>],
    companion: Option<&InstalledCompanion>,
) -> Result<RunDir, String> {
    let mut files = files.to_vec();
    files.sort_by(|left, right| left.path.cmp(right.path));
    if let Some(bad) = files.iter().find(|file| !is_contained(file.path)) {
        return Err(format!(
            "the plan path `{}` is not a plain relative path",
            bad.path
        ));
    }
    let dir = mutation_cache.join("runs").join(content_hash(&files));
    let io = |error: std::io::Error| {
        format!(
            "cannot write the mutation run directory {}: {error}",
            dir.display()
        )
    };
    let mut path_map = BTreeMap::new();
    for file in &files {
        let target = format!("files/{}", file.path);
        let absolute = dir.join(&target);
        if let Some(parent) = absolute.parent() {
            fs::create_dir_all(parent).map_err(io)?;
        }
        fs::write(&absolute, file.code).map_err(io)?;
        path_map.insert(file.path.to_owned(), target);
    }
    fs::create_dir_all(&dir).map_err(io)?;
    let paths = serde_json::to_string_pretty(&path_map)
        .map_err(|error| format!("cannot serialize paths.json: {error}"))?;
    fs::write(dir.join("paths.json"), paths).map_err(io)?;
    if let Some(companion) = companion {
        for (relative, source) in companion.runtime_files() {
            let target = dir.join(relative);
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent).map_err(io)?;
            }
            fs::copy(source, &target).map_err(io)?;
        }
    }
    Ok(RunDir { dir, path_map })
}

#[expect(clippy::unwrap_used, reason = "tests unwrap to keep assertions short")]
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_run_dir_holds_each_file_and_the_path_map_under_a_content_hash() {
        let cache = tempfile::tempdir().expect("dir");
        let files = [
            RunFile {
                path: "src/b.ts",
                code: "// @ts-nocheck\nb",
            },
            RunFile {
                path: "src/a.ts",
                code: "// @ts-nocheck\na",
            },
        ];
        let run = write_run_dir(cache.path(), &files, None).expect("run dir");
        assert_eq!(
            run.path_map,
            BTreeMap::from([
                ("src/a.ts".to_owned(), "files/src/a.ts".to_owned()),
                ("src/b.ts".to_owned(), "files/src/b.ts".to_owned()),
            ])
        );
        assert_eq!(
            fs::read_to_string(run.dir.join("files/src/b.ts")).expect("file"),
            "// @ts-nocheck\nb"
        );
        let paths: BTreeMap<String, String> = serde_json::from_str(
            &fs::read_to_string(run.dir.join("paths.json")).expect("paths.json"),
        )
        .expect("json");
        assert_eq!(paths, run.path_map);

        let changed = [
            files[0],
            RunFile {
                path: "src/a.ts",
                code: "// @ts-nocheck\nA",
            },
        ];
        let other = write_run_dir(cache.path(), &changed, None).expect("run dir");
        assert_ne!(other.dir, run.dir, "other text gets another directory");
        let reordered = [files[1], files[0]];
        assert_eq!(
            write_run_dir(cache.path(), &reordered, None)
                .expect("run dir")
                .dir,
            run.dir,
            "the file order does not change the hash"
        );
    }

    #[test]
    fn a_path_that_leaves_the_run_dir_is_refused() {
        let cache = tempfile::tempdir().expect("dir");
        for path in ["../escape.ts", "/abs.ts", "src/../../x.ts", ""] {
            let error =
                write_run_dir(cache.path(), &[RunFile { path, code: "x" }], None).unwrap_err();
            assert!(
                error.contains("not a plain relative path"),
                "{path}: {error}"
            );
        }
        assert!(!cache.path().join("escape.ts").exists());
    }
}
