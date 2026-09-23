//! Base-side file reads, and the check that lets an audit reuse the head run
//! as its base snapshot.

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;

use fallow_engine::changed_files::clear_ambient_git_env;
use rustc_hash::FxHashSet;

/// Whether the head run can stand in for the base snapshot.
///
/// True when no changed file can change a finding: each one is a Fallow cache
/// artifact, a documentation file, or a JS/TS source whose base version has
/// the same token stream (whitespace and comment changes). The audit then
/// reuses the head keys as the base keys, which attributes every finding as
/// inherited, without a base worktree.
#[must_use]
#[expect(
    clippy::implicit_hasher,
    reason = "fallow standardizes on FxHashSet across the workspace"
)]
pub fn can_reuse_current_as_base(
    root: &Path,
    cache_dir: Option<&Path>,
    base_ref: &str,
    changed_files: &FxHashSet<PathBuf>,
) -> bool {
    let Ok(git_root) = fallow_engine::changed_files::resolve_git_toplevel(root) else {
        return false;
    };
    let canonical_cache_dir = cache_dir.and_then(|dir| dunce::canonicalize(dir).ok());
    // Spawn the batched base-file reader lazily: a changeset of only cache
    // artifacts or docs never touches git, so it spawns zero processes.
    let mut reader: Option<BaseFileReader> = None;
    for path in changed_files {
        if cache_dir
            .is_some_and(|dir| is_fallow_cache_artifact(path, dir, canonical_cache_dir.as_deref()))
        {
            continue;
        }
        if !is_analysis_input(path) {
            if is_non_behavioral_doc(path) {
                continue;
            }
            return false;
        }
        let Ok(current) = std::fs::read_to_string(path) else {
            return false;
        };
        let Ok(relative) = path.strip_prefix(&git_root) else {
            return false;
        };
        let reader = match reader.as_mut() {
            Some(reader) => reader,
            None => {
                let Some(spawned) = BaseFileReader::spawn(root) else {
                    return false;
                };
                reader.insert(spawned)
            }
        };
        let base = match reader.read(base_ref, relative) {
            BaseRead::Content(base) => base,
            BaseRead::Missing | BaseRead::Error => return false,
        };
        if current == base {
            continue;
        }
        if !js_ts_tokens_equivalent(path, &current, &base) {
            return false;
        }
    }
    true
}

/// A long-lived `git cat-file --batch` child process that reads the base
/// version of changed files without one `git show` for each file.
///
/// Requests and responses are strictly lockstep (one request line, one
/// response), so the pipe buffers cannot deadlock. A missing object yields
/// [`BaseRead::Missing`], and content is read with lossy UTF-8 conversion.
///
/// The child is a [`fallow_process::ScopedChild`], so an interrupt
/// (SIGINT/SIGTERM) during a large read loop kills the `cat-file` process
/// through the signal registry instead of leaving it orphaned.
pub struct BaseFileReader {
    /// The registered `cat-file --batch` child. `Drop` takes it and calls the
    /// consuming wait after it closes stdin, which reaps the child and
    /// deregisters its PID.
    child: Option<fallow_process::ScopedChild>,
    /// `Drop` takes and drops it before the blocking wait, which closes the
    /// pipe so the wait cannot block.
    stdin: Option<std::process::ChildStdin>,
    stdout: std::io::BufReader<std::process::ChildStdout>,
}

impl BaseFileReader {
    /// Spawn one `git cat-file --batch` process in `root`.
    ///
    /// Returns `None` when the spawn fails or the stdio pipes are not
    /// available. The caller then treats the files as not reusable.
    #[must_use]
    pub fn spawn(root: &Path) -> Option<Self> {
        let mut command = Command::new("git");
        command
            .args(["cat-file", "--batch"])
            .current_dir(root)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null());
        clear_ambient_git_env(&mut command);
        let mut child = fallow_process::ScopedChild::spawn(&mut command).ok()?;
        let stdin = child.take_stdin()?;
        let stdout = child.take_stdout()?;
        Some(Self {
            child: Some(child),
            stdin: Some(stdin),
            stdout: std::io::BufReader::new(stdout),
        })
    }

    /// Read the base version of the repository-relative path `relative` at
    /// `base_ref`.
    ///
    /// Writes one `<base_ref>:<path>` request line (forward slashes) and reads
    /// exactly one response. A ` missing` header yields [`BaseRead::Missing`].
    /// A parse or IO error, or a path with a newline (which would corrupt the
    /// request stream), yields [`BaseRead::Error`].
    pub fn read(&mut self, base_ref: &str, relative: &Path) -> BaseRead {
        use std::io::{BufRead, Read};

        let relative = relative.to_string_lossy().replace('\\', "/");
        if relative.contains('\n') {
            return BaseRead::Error;
        }

        let Some(stdin) = self.stdin.as_mut() else {
            return BaseRead::Error;
        };
        if writeln!(stdin, "{base_ref}:{relative}").is_err() || stdin.flush().is_err() {
            return BaseRead::Error;
        }

        let mut header = String::new();
        if !matches!(self.stdout.read_line(&mut header), Ok(n) if n > 0) {
            return BaseRead::Error;
        }
        // `git cat-file --batch` reports a missing object as `<spec> missing\n`.
        if header.trim_end().ends_with(" missing") {
            return BaseRead::Missing;
        }
        // Otherwise the header is `<oid> <type> <size>\n`.
        let Some(size) = header
            .trim_end()
            .rsplit(' ')
            .next()
            .and_then(|raw| raw.parse::<usize>().ok())
        else {
            return BaseRead::Error;
        };
        let mut buf = vec![0u8; size];
        if self.stdout.read_exact(&mut buf).is_err() {
            return BaseRead::Error;
        }
        // Consume the one newline after the object content. An off-by-one
        // here corrupts every later read in the batch.
        let mut newline = [0u8; 1];
        if self.stdout.read_exact(&mut newline).is_err() {
            return BaseRead::Error;
        }

        BaseRead::Content(String::from_utf8_lossy(&buf).into_owned())
    }
}

/// Outcome of one batched base-file read. "The object does not exist at base"
/// and "the pipe or parse failed" stay apart, so a transient `git cat-file`
/// failure never reads as an empty base file.
#[derive(Debug, PartialEq, Eq)]
pub enum BaseRead {
    /// The object exists at base; lossy UTF-8 content.
    Content(String),
    /// `git cat-file` reported the object as ` missing`: the file is new
    /// relative to base.
    Missing,
    /// A pipe write or read, or a header parse, failed (or the path cannot be
    /// requested). Later reads from this reader are not reliable.
    Error,
}

impl Drop for BaseFileReader {
    fn drop(&mut self) {
        // Close stdin so the child sees EOF and exits, then reap it through the
        // blocking wait of the scoped child, which also deregisters the PID.
        self.stdin.take();
        if let Some(child) = self.child.take() {
            let _ = child.wait();
        }
    }
}

fn is_fallow_cache_artifact(
    path: &Path,
    cache_dir: &Path,
    canonical_cache_dir: Option<&Path>,
) -> bool {
    path.starts_with(cache_dir)
        || canonical_cache_dir.is_some_and(|canonical| path.starts_with(canonical))
}

pub(super) fn is_analysis_input(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|ext| ext.to_str()),
        Some(
            "js" | "jsx"
                | "ts"
                | "tsx"
                | "mjs"
                | "mts"
                | "cjs"
                | "cts"
                | "vue"
                | "svelte"
                | "astro"
                | "mdx"
                | "css"
                | "scss"
        )
    )
}

pub(super) fn is_non_behavioral_doc(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|ext| ext.to_str()),
        Some("md" | "markdown" | "txt" | "rst" | "adoc")
    )
}

/// Text that fallow reads outside the token stream. The token comparison
/// skips comments, so a change to a suppression comment, a JSDoc visibility
/// tag or a JSDoc `import()` type can change the findings with no token
/// change. `import(` also covers a dynamic import whose template literal
/// content the tokenizer does not keep.
const REUSE_BLOCKING_MARKERS: &[&str] = &[
    "fallow-ignore",
    "@expected-unused",
    "@public",
    "@internal",
    "@beta",
    "@alpha",
    "@api",
    "import(",
];

fn has_reuse_blocking_marker(source: &str) -> bool {
    REUSE_BLOCKING_MARKERS
        .iter()
        .any(|marker| source.contains(marker))
}

pub(super) fn js_ts_tokens_equivalent(path: &Path, current: &str, base: &str) -> bool {
    if has_reuse_blocking_marker(current) || has_reuse_blocking_marker(base) {
        return false;
    }
    if !matches!(
        path.extension().and_then(|ext| ext.to_str()),
        Some("js" | "jsx" | "ts" | "tsx" | "mjs" | "mts" | "cjs" | "cts")
    ) {
        return false;
    }
    fallow_engine::duplicates::source_token_kinds_equivalent(path, current, base, false)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A severed request pipe is an error, never a missing object or empty
    /// content, so a caller stops its scan instead of reading every later
    /// file as empty at base.
    #[test]
    fn a_severed_request_pipe_is_an_error() {
        let tmp = tempfile::TempDir::new().expect("temp dir should be created");
        let mut reader = BaseFileReader::spawn(tmp.path()).expect("reader should spawn");

        reader.stdin.take();

        assert_eq!(reader.read("HEAD", Path::new("README.md")), BaseRead::Error);
    }
}
