//! Project-local shell helpers that a script command runs.
//!
//! `bash scripts/build.sh` and `./scripts/release.sh` run the commands of a
//! file. Fallow reads such a file without running it and analyzes each of its
//! commands like a command of the caller: the commands credit the packages
//! that they invoke and seed the files that they run as entries.
//!
//! A helper path resolves in the directory where the command runs: the
//! package directory for a package script, or the `working-directory` of a CI
//! step. The helper must stay inside the project root after symlinks resolve,
//! and must not be in `node_modules`. A helper that runs another helper is
//! followed up to [`MAX_SCRIPT_INDIRECTION_DEPTH`] levels, and a helper on the
//! current path is not read again. One analysis reads at most
//! [`MAX_HELPER_FILES`] helpers of at most [`MAX_HELPER_BYTES`] each, and reads
//! each helper once.
//!
//! The commands of a helper run in the directory of the caller. A `cd` to a
//! literal relative directory or to the helper directory
//! (`cd "$(dirname "$0")/.."`) moves that directory. After any other
//! directory change, the remaining commands of the helper credit packages
//! only, because their paths are relative to an unknown directory.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use rustc_hash::FxHashMap;

use super::{
    MAX_SCRIPT_INDIRECTION_DEPTH, ScriptAnalysis, ScriptCommand, ScriptCommandContext,
    accumulate_parsed_commands, could_be_file_path, parse_script_with_context, rebase_path, shell,
};

/// Shells whose first file argument is a script file (`bash scripts/a.sh`).
const SHELLS: &[&str] = &["sh", "bash", "zsh", "dash"];

/// Extensions of a helper that runs as its own command (`./scripts/a.sh`).
const HELPER_EXTENSIONS: &[&str] = &[".sh", ".bash"];

/// Commands that change the working directory of the following commands.
const DIRECTORY_CHANGERS: &[&str] = &["cd", "pushd", "popd"];

/// The forms of the helper directory in a `cd` argument, with quotes removed.
const HELPER_DIR_FORMS: &[&str] = &[
    "$(dirname $0)",
    "$(dirname -- $0)",
    "$(dirname ${BASH_SOURCE[0]})",
    "${0%/*}",
];

/// The most helper files that one analysis reads.
pub(super) const MAX_HELPER_FILES: usize = 256;

/// The most helper analyses of one analysis, cache hits included.
pub(super) const MAX_HELPER_EXPANSIONS: usize = 4096;

/// The largest helper file that Fallow reads.
pub(super) const MAX_HELPER_BYTES: u64 = 1024 * 1024;

/// The bytes that the shebang check of a helper reads.
const SHEBANG_BYTES: u64 = 256;

/// The directory in which the commands of a script catalog run, as the base
/// of the helper paths that they name.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) enum HelperBase {
    /// The directory of the catalog package.
    #[default]
    Package,
    /// A directory relative to the project root (a CI `working-directory`).
    Directory(String),
    /// A directory that Fallow cannot resolve. No helper is followed.
    Unknown,
}

type CommandLines = Arc<[String]>;

/// The command lines of the helper files of one analysis, read once and
/// shared by every command source.
#[derive(Debug, Default)]
pub(super) struct ShellHelperFiles {
    project_root: Option<PathBuf>,
    lines: Mutex<FxHashMap<PathBuf, Option<CommandLines>>>,
    reads: AtomicUsize,
    expansions: AtomicUsize,
}

impl ShellHelperFiles {
    pub(super) fn new(project_root: &Path) -> Self {
        Self {
            project_root: project_root.canonicalize().ok(),
            ..Self::default()
        }
    }

    /// Count one helper analysis. `false` once the analysis budget is spent.
    fn take_expansion(&self) -> bool {
        self.expansions.fetch_add(1, Ordering::Relaxed) < MAX_HELPER_EXPANSIONS
    }

    fn command_lines(&self, path: &Path) -> Option<CommandLines> {
        if let Some(cached) = self.lines.lock().ok()?.get(path) {
            return cached.clone();
        }
        let lines = if self.reads.fetch_add(1, Ordering::Relaxed) < MAX_HELPER_FILES {
            read_command_lines(path)
        } else {
            None
        };
        self.lines
            .lock()
            .ok()?
            .insert(path.to_path_buf(), lines.clone());
        lines
    }
}

fn read_command_lines(path: &Path) -> Option<CommandLines> {
    if std::fs::metadata(path).ok()?.len() > MAX_HELPER_BYTES {
        return None;
    }
    let bytes = std::fs::read(path).ok()?;
    Some(script_file_commands(&String::from_utf8_lossy(&bytes)).into())
}

/// Analyze `command` into `result`, then read and analyze the project-local
/// shell helpers that it runs.
pub(super) fn accumulate_command_and_helpers(
    command: &str,
    root: &Path,
    bin_map: &FxHashMap<String, String>,
    context: &ScriptCommandContext<'_>,
    result: &mut ScriptAnalysis,
) {
    let parsed = parse_script_with_context(command, root, bin_map, context);
    let local_files = ShellHelperFiles::default();
    let files = context
        .scripts
        .workspaces
        .as_deref()
        .map_or(&local_files, |workspaces| workspaces.shell_helpers());
    let walk = HelperWalk::new(root, bin_map, context, files);
    let helpers: Vec<PathBuf> = match &walk {
        Some(walk) if !changes_directory(command) => parsed
            .iter()
            .filter_map(|cmd| walk.helper_file(cmd, &walk.base))
            .collect(),
        _ => Vec::new(),
    };
    accumulate_parsed_commands(command, root, bin_map, parsed, context.ignored, result);
    if let Some(mut walk) = walk {
        let base = walk.base.clone();
        for helper in helpers {
            walk.accumulate_helper(&helper, &base, result);
        }
    }
}

/// The state of the helper walk of one top-level command.
struct HelperWalk<'a, 'c> {
    root: &'a Path,
    bin_map: &'a FxHashMap<String, String>,
    context: &'a ScriptCommandContext<'c>,
    files: &'a ShellHelperFiles,
    /// The canonical project root. No helper outside it is read.
    project_root: PathBuf,
    /// The canonical directory of the top-level command. Result paths are
    /// relative to it.
    base: PathBuf,
    /// The helpers on the current path.
    stack: Vec<PathBuf>,
    /// The smallest depth at which this walk analyzed each helper and
    /// directory. A helper that several commands run is analyzed once.
    visited: FxHashMap<(PathBuf, PathBuf), usize>,
}

impl<'a, 'c> HelperWalk<'a, 'c> {
    fn new(
        root: &'a Path,
        bin_map: &'a FxHashMap<String, String>,
        context: &'a ScriptCommandContext<'c>,
        files: &'a ShellHelperFiles,
    ) -> Option<Self> {
        let project_root = files
            .project_root
            .clone()
            .or_else(|| root.canonicalize().ok())?;
        let dir = match &context.scripts.helper_base {
            HelperBase::Package => context.scripts.package_dir.as_str(),
            HelperBase::Directory(dir) => dir.as_str(),
            HelperBase::Unknown => return None,
        };
        let base = project_root.join(dir).canonicalize().ok()?;
        Some(Self {
            root,
            bin_map,
            context,
            files,
            project_root,
            base,
            stack: Vec::new(),
            visited: FxHashMap::default(),
        })
    }

    /// Analyze the commands of `helper`, which runs in `cwd`.
    fn accumulate_helper(&mut self, helper: &Path, cwd: &Path, result: &mut ScriptAnalysis) {
        let depth = self.stack.len();
        if depth >= MAX_SCRIPT_INDIRECTION_DEPTH || self.stack.iter().any(|open| open == helper) {
            return;
        }
        let key = (helper.to_path_buf(), cwd.to_path_buf());
        if self.visited.get(&key).is_some_and(|&seen| seen <= depth) {
            return;
        }
        self.visited.insert(key, depth);
        if !self.files.take_expansion() {
            return;
        }
        let Some(lines) = self.files.command_lines(helper) else {
            return;
        };
        self.stack.push(helper.to_path_buf());
        let mut cwd = Some(cwd.to_path_buf());
        for line in lines.iter() {
            for segment in shell::split_shell_operators(line) {
                let segment = segment.trim();
                if segment.is_empty() {
                    continue;
                }
                if let Some(change) = self.directory_change(cwd.as_deref(), helper, segment) {
                    cwd = change;
                    continue;
                }
                self.accumulate_segment(segment, cwd.as_deref(), result);
            }
        }
        self.stack.pop();
    }

    /// Analyze one command segment of a helper that runs in `cwd`, or in an
    /// unknown directory for `None`.
    fn accumulate_segment(
        &mut self,
        segment: &str,
        cwd: Option<&Path>,
        result: &mut ScriptAnalysis,
    ) {
        let parsed = parse_script_with_context(segment, self.root, self.bin_map, self.context);
        let known = cwd.and_then(|cwd| {
            let prefix = cwd.strip_prefix(&self.base).ok()?;
            Some((cwd, prefix.to_string_lossy().replace('\\', "/")))
        });
        let Some((cwd, prefix)) = known else {
            let mut packages_only = ScriptAnalysis::default();
            accumulate_parsed_commands(
                segment,
                self.root,
                self.bin_map,
                parsed,
                self.context.ignored,
                &mut packages_only,
            );
            result.used_packages.extend(packages_only.used_packages);
            return;
        };
        let helpers: Vec<PathBuf> = parsed
            .iter()
            .filter_map(|cmd| self.helper_file(cmd, cwd))
            .collect();
        let parsed = parsed
            .into_iter()
            .map(|mut cmd| {
                if !prefix.is_empty() {
                    cmd.map_paths(|paths| {
                        paths
                            .iter()
                            .map(|path| rebase_path(&prefix, path))
                            .collect()
                    });
                }
                cmd
            })
            .collect();
        accumulate_parsed_commands(
            segment,
            self.root,
            self.bin_map,
            parsed,
            self.context.ignored,
            result,
        );
        for helper in helpers {
            self.accumulate_helper(&helper, cwd, result);
        }
    }

    /// The working directory after `segment`: `None` when `segment` does not
    /// change it, `Some(None)` when the new directory is unknown.
    #[expect(
        clippy::option_option,
        reason = "not a directory change, an unknown directory, or a known one"
    )]
    fn directory_change(
        &self,
        cwd: Option<&Path>,
        helper: &Path,
        segment: &str,
    ) -> Option<Option<PathBuf>> {
        let first = segment.split_whitespace().next()?;
        if first != "cd" {
            return DIRECTORY_CHANGERS
                .contains(&first.trim_start_matches('('))
                .then_some(None);
        }
        let target: String = segment["cd".len()..]
            .trim()
            .chars()
            .filter(|ch| !matches!(ch, '"' | '\''))
            .collect();
        Some(cwd.and_then(|cwd| self.cd_target(cwd, helper, &target)))
    }

    /// The canonical directory that `cd <target>` enters from `cwd`, when it
    /// is a literal relative directory or the helper directory.
    fn cd_target(&self, cwd: &Path, helper: &Path, target: &str) -> Option<PathBuf> {
        let (start, rest) = match HELPER_DIR_FORMS
            .iter()
            .find_map(|form| target.strip_prefix(form))
        {
            Some(rest) => (helper.parent()?, rest.trim_start_matches('/')),
            None => (cwd, target),
        };
        if target.is_empty()
            || rest.starts_with(['-', '~', '/'])
            || rest.contains(['$', '`', '*', '?', '[', ' '])
        {
            return None;
        }
        let dir = start.join(rest).canonicalize().ok()?;
        (dir.is_dir() && dir.starts_with(&self.project_root)).then_some(dir)
    }

    /// Return the project-local shell helper that `cmd` runs from `cwd`, as
    /// a canonical path.
    fn helper_file(&self, cmd: &ScriptCommand, cwd: &Path) -> Option<PathBuf> {
        let (candidate, direct) = if SHELLS.contains(&cmd.binary.as_str()) {
            (cmd.file_args.first()?.as_str(), false)
        } else if cmd.binary.contains('/') && could_be_file_path(&cmd.binary) {
            (cmd.binary.as_str(), true)
        } else {
            return None;
        };
        let path = cwd.join(candidate).canonicalize().ok()?;
        let in_dependency = path
            .strip_prefix(&self.project_root)
            .ok()?
            .components()
            .any(|part| part.as_os_str() == "node_modules");
        if in_dependency || !path.is_file() {
            return None;
        }
        (!direct || is_shell_helper(&path)).then_some(path)
    }
}

/// Whether a file that runs as its own command is a shell script: it has a
/// shell extension or a shell shebang (`#!/usr/bin/env bash`).
fn is_shell_helper(path: &Path) -> bool {
    let name = path.to_string_lossy();
    if HELPER_EXTENSIONS.iter().any(|ext| name.ends_with(ext)) {
        return true;
    }
    let mut head = Vec::new();
    let read = std::fs::File::open(path)
        .and_then(|file| file.take(SHEBANG_BYTES).read_to_end(&mut head))
        .is_ok();
    let head = String::from_utf8_lossy(&head);
    read && head.lines().next().is_some_and(|first| {
        first.starts_with("#!")
            && first
                .split_whitespace()
                .last()
                .and_then(|interpreter| interpreter.rsplit('/').next())
                .is_some_and(|interpreter| SHELLS.contains(&interpreter))
    })
}

/// Whether a command segment of `command` changes the working directory.
fn changes_directory(command: &str) -> bool {
    shell::split_shell_operators(command)
        .into_iter()
        .any(|segment| {
            segment
                .split_whitespace()
                .next()
                .map(|word| word.trim_start_matches('('))
                .is_some_and(|word| DIRECTORY_CHANGERS.contains(&word))
        })
}

/// Split a shell script into command lines. Comments, the shebang, blank
/// lines, `.` sourcing lines and here-document bodies are skipped, and a line
/// that ends with `\` continues on the next one.
pub(super) fn script_file_commands(content: &str) -> Vec<String> {
    let mut commands = Vec::new();
    let mut pending = String::new();
    let mut heredoc_end: Option<String> = None;
    for line in content.lines() {
        let trimmed = line.trim();
        if let Some(end) = &heredoc_end {
            if trimmed == end {
                heredoc_end = None;
            }
            continue;
        }
        if pending.is_empty()
            && (trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with(". "))
        {
            continue;
        }
        if let Some(continued) = trimmed.strip_suffix('\\') {
            pending.push_str(continued.trim_end());
            pending.push(' ');
            continue;
        }
        pending.push_str(trimmed);
        heredoc_end = heredoc_delimiter(&pending);
        commands.push(std::mem::take(&mut pending));
    }
    if !pending.trim().is_empty() {
        commands.push(pending);
    }
    commands
}

/// The end word of a here-document that `line` opens (`cat <<'EOF'`). A
/// `<<` in quotes or in arithmetic (`$((1 << 3))`) does not open one.
fn heredoc_delimiter(line: &str) -> Option<String> {
    let bytes = line.as_bytes();
    let (mut single, mut double, mut arithmetic) = (false, false, 0_usize);
    let mut i = 0;
    while i < bytes.len() {
        let rest = &bytes[i..];
        match bytes[i] {
            b'\\' if !single => i += 2,
            b'\'' if !double => {
                single = !single;
                i += 1;
            }
            b'"' if !single => {
                double = !double;
                i += 1;
            }
            _ if single || double => i += 1,
            _ if rest.starts_with(b"$((") || rest.starts_with(b"((") => {
                arithmetic += 1;
                i += if rest.starts_with(b"$") { 3 } else { 2 };
            }
            _ if arithmetic > 0 && rest.starts_with(b"))") => {
                arithmetic -= 1;
                i += 2;
            }
            _ if arithmetic > 0 => i += 1,
            // A here-string (`<<< "$value"`) has no body.
            _ if rest.starts_with(b"<<<") => i += 3,
            _ if rest.starts_with(b"<<") => return delimiter_word(&line[i + 2..]),
            _ => i += 1,
        }
    }
    None
}

/// The word after `<<`: `EOF`, `-EOF`, `'EOF'`, `"EOF"` or `\EOF`.
fn delimiter_word(rest: &str) -> Option<String> {
    let rest = rest.strip_prefix('-').unwrap_or(rest).trim_start();
    let word: String = rest
        .trim_start_matches(['\\', '\'', '"'])
        .chars()
        .take_while(|ch| ch.is_ascii_alphanumeric() || *ch == '_')
        .collect();
    (!word.is_empty()).then_some(word)
}

#[cfg(test)]
#[path = "shell_helpers_tests.rs"]
mod tests;
