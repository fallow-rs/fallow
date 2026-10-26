//! Project-local shell helpers that a script command runs.
//!
//! `bash scripts/build.sh` and `./scripts/release.sh` run the commands of a
//! file. Fallow reads such a file without running it and analyzes each of its
//! commands like a command of the calling script: the commands credit the
//! packages that they invoke and seed the files that they run as entries.
//!
//! Only a helper inside the calling package directory is read, never one in
//! `node_modules`. A helper that runs another helper is followed up to
//! [`MAX_SCRIPT_INDIRECTION_DEPTH`] levels and [`MAX_SCRIPT_EXPANSIONS`] files,
//! and a helper on the current path is not read again. After a command that
//! changes the working directory (`cd`, `pushd`, `popd`), the paths of the
//! remaining commands are relative to an unknown directory. Those commands
//! still credit packages, but they do not seed entries or follow helpers.

use std::path::{Path, PathBuf};

use rustc_hash::FxHashMap;

use super::{
    MAX_SCRIPT_EXPANSIONS, MAX_SCRIPT_INDIRECTION_DEPTH, ScriptAnalysis, ScriptCommand,
    ScriptCommandContext, accumulate_parsed_commands, could_be_file_path,
    parse_script_with_context, shell,
};

/// Shells whose first file argument is a script file (`bash scripts/a.sh`).
const SHELLS: &[&str] = &["sh", "bash", "zsh", "dash"];

/// Extensions of a helper that runs as its own command (`./scripts/a.sh`).
const HELPER_EXTENSIONS: &[&str] = &[".sh", ".bash"];

/// Commands that change the working directory of the following commands.
const DIRECTORY_CHANGERS: &[&str] = &["cd", "pushd", "popd"];

/// The helpers on the current path and the count of helpers read for one
/// top-level command.
#[derive(Default)]
pub(super) struct HelperWalk {
    stack: Vec<PathBuf>,
    expanded: usize,
}

/// Analyze `command` into `result`, then read and analyze the project-local
/// shell helpers that it runs.
pub(super) fn accumulate_command_and_helpers(
    command: &str,
    root: &Path,
    bin_map: &FxHashMap<String, String>,
    context: &ScriptCommandContext<'_>,
    result: &mut ScriptAnalysis,
    walk: &mut HelperWalk,
) {
    let parsed = parse_script_with_context(command, root, bin_map, context);
    let helpers: Vec<PathBuf> = parsed
        .iter()
        .filter_map(|cmd| helper_file(cmd, root))
        .collect();
    accumulate_parsed_commands(command, root, bin_map, parsed, context.ignored, result);
    for helper in helpers {
        accumulate_helper(&helper, root, bin_map, context, result, walk);
    }
}

fn accumulate_helper(
    helper: &Path,
    root: &Path,
    bin_map: &FxHashMap<String, String>,
    context: &ScriptCommandContext<'_>,
    result: &mut ScriptAnalysis,
    walk: &mut HelperWalk,
) {
    if walk.stack.len() >= MAX_SCRIPT_INDIRECTION_DEPTH
        || walk.expanded >= MAX_SCRIPT_EXPANSIONS
        || walk.stack.iter().any(|open| open == helper)
    {
        return;
    }
    let Ok(content) = std::fs::read_to_string(helper) else {
        return;
    };
    walk.expanded += 1;
    walk.stack.push(helper.to_path_buf());
    let mut moved = false;
    for line in script_file_commands(&content) {
        moved = moved || changes_directory(&line);
        if moved {
            let mut packages_only = ScriptAnalysis::default();
            let parsed = parse_script_with_context(&line, root, bin_map, context);
            accumulate_parsed_commands(
                &line,
                root,
                bin_map,
                parsed,
                context.ignored,
                &mut packages_only,
            );
            result.used_packages.extend(packages_only.used_packages);
        } else {
            accumulate_command_and_helpers(&line, root, bin_map, context, result, walk);
        }
    }
    walk.stack.pop();
}

/// Return the project-local shell helper that `cmd` runs, as a canonical
/// path inside `root`.
fn helper_file(cmd: &ScriptCommand, root: &Path) -> Option<PathBuf> {
    let (candidate, direct) = if SHELLS.contains(&cmd.binary.as_str()) {
        (cmd.file_args.first()?.as_str(), false)
    } else if cmd.binary.contains('/') && could_be_file_path(&cmd.binary) {
        (cmd.binary.as_str(), true)
    } else {
        return None;
    };
    let root = root.canonicalize().ok()?;
    let path = root.join(candidate).canonicalize().ok()?;
    if !path.starts_with(&root)
        || !path.is_file()
        || path
            .strip_prefix(&root)
            .ok()?
            .components()
            .any(|part| part.as_os_str() == "node_modules")
    {
        return None;
    }
    (!direct || is_shell_helper(&path)).then_some(path)
}

/// Whether a file that runs as its own command is a shell script: it has a
/// shell extension or a shell shebang (`#!/usr/bin/env bash`).
fn is_shell_helper(path: &Path) -> bool {
    let name = path.to_string_lossy();
    if HELPER_EXTENSIONS.iter().any(|ext| name.ends_with(ext)) {
        return true;
    }
    std::fs::read_to_string(path).is_ok_and(|content| {
        content.lines().next().is_some_and(|first| {
            first.starts_with("#!")
                && first
                    .split_whitespace()
                    .last()
                    .and_then(|interpreter| interpreter.rsplit('/').next())
                    .is_some_and(|interpreter| SHELLS.contains(&interpreter))
        })
    })
}

/// Whether a command segment of `line` changes the working directory.
fn changes_directory(line: &str) -> bool {
    shell::split_shell_operators(line)
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

/// The end word of a here-document that `line` opens (`cat <<'EOF'`).
fn heredoc_delimiter(line: &str) -> Option<String> {
    let start = line.find("<<")?;
    let rest = &line[start + 2..];
    if rest.starts_with('<') {
        return None;
    }
    let word: String = rest
        .trim_start_matches('-')
        .trim_start()
        .trim_start_matches(['\'', '"'])
        .chars()
        .take_while(|ch| ch.is_ascii_alphanumeric() || *ch == '_')
        .collect();
    (!word.is_empty()).then_some(word)
}

#[cfg(test)]
mod tests {
    use rustc_hash::FxHashSet;

    use super::super::{IgnoredCommandEntries, ScriptCatalog, analyze_commands_with_context};
    use super::*;

    fn write(root: &Path, path: &str, content: &str) {
        let path = root.join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, content).unwrap();
    }

    fn analyze(root: &Path, command: &str) -> ScriptAnalysis {
        analyze_commands_with_context(
            &[command.to_string()],
            root,
            &FxHashMap::default(),
            &FxHashSet::default(),
            &ScriptCatalog::default(),
            IgnoredCommandEntries::NONE,
        )
    }

    #[test]
    fn shell_runs_a_helper_and_its_commands_count() {
        let dir = tempfile::tempdir().unwrap();
        write(
            dir.path(),
            "scripts/build.sh",
            "#!/bin/sh\nreport-cli --format json\ntsx src/generate.ts\n",
        );
        let result = analyze(dir.path(), "bash scripts/build.sh");
        assert!(result.used_packages.contains("report-cli"));
        assert!(result.entry_files.contains(&"src/generate.ts".to_string()));
    }

    #[test]
    fn executable_helper_needs_a_shell_extension_or_shebang() {
        let dir = tempfile::tempdir().unwrap();
        write(
            dir.path(),
            "scripts/release",
            "#!/usr/bin/env bash\nexec-cli\n",
        );
        write(dir.path(), "scripts/data", "data-cli\n");
        assert!(
            analyze(dir.path(), "./scripts/release")
                .used_packages
                .contains("exec-cli")
        );
        assert!(
            !analyze(dir.path(), "./scripts/data")
                .used_packages
                .contains("data-cli")
        );
    }

    #[test]
    fn nested_helpers_are_followed_and_cycles_stop() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "a.sh", "a-cli\nbash b.sh\n");
        write(dir.path(), "b.sh", "b-cli\nsh a.sh\n");
        let result = analyze(dir.path(), "bash a.sh");
        assert!(result.used_packages.contains("a-cli"));
        assert!(result.used_packages.contains("b-cli"));
    }

    #[test]
    fn helper_outside_the_package_is_not_read() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "outside.sh", "outside-cli\n");
        write(dir.path(), "pkg/node_modules/dep/run.sh", "dep-cli\n");
        let pkg = dir.path().join("pkg");
        assert!(
            !analyze(&pkg, "bash ../outside.sh")
                .used_packages
                .contains("outside-cli")
        );
        assert!(
            !analyze(&pkg, "bash node_modules/dep/run.sh")
                .used_packages
                .contains("dep-cli")
        );
    }

    #[test]
    fn commands_after_a_directory_change_credit_packages_only() {
        let dir = tempfile::tempdir().unwrap();
        write(
            dir.path(),
            "run.sh",
            "tsx src/before.ts\ncd app && tsx src/inline.ts\nlate-cli src/after.ts\n",
        );
        let result = analyze(dir.path(), "bash run.sh");
        assert!(result.entry_files.contains(&"src/before.ts".to_string()));
        assert!(!result.entry_files.contains(&"src/inline.ts".to_string()));
        assert!(!result.entry_files.contains(&"src/after.ts".to_string()));
        assert!(result.used_packages.contains("late-cli"));
    }

    #[test]
    fn script_file_commands_skip_comments_and_heredocs() {
        let commands = script_file_commands(
            "#!/bin/sh\n# note-cli\ncat <<-'EOF'\nheredoc-cli\nEOF\nfirst \\\n  --flag\n. ./env.sh\n",
        );
        assert_eq!(commands, vec!["cat <<-'EOF'", "first --flag"]);
    }
}
