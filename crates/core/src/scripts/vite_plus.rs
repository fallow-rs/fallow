//! Vite+ commands: the `vp`, `vpr` and `vpx` binaries, and the commands in
//! the `staged` and `run.tasks` blocks of `vite.config.*`.
//!
//! `vp <subcommand>` runs a bundled tool. The `vite-plus` package is the
//! dependency that the command uses, and the bundled tool decides how the
//! arguments are read. `vp lint <file>` does not run the file, and
//! `vp test -c <config>` reads a Vitest config.

use std::path::Path;

use fallow_extract::JsText;
use oxc_ast::ast::{Expression, ObjectExpression};

use super::{PackageSelector, RunLocation};
use crate::plugins::config_parser;
use crate::plugins::vite_plus::VITE_CONFIG_FILES;

/// The binary that runs a Vite+ subcommand.
const VP: &str = "vp";

/// `vpr <task>` is `vp run <task>`.
pub(super) const VPR: &str = "vpr";

/// `vpx <binary>` runs a package binary, like `npx`.
const VPX: &str = "vpx";

/// The tool that reads the arguments of each `vp` subcommand. A subcommand
/// that is not in this table takes no file or config arguments.
const SUBCOMMAND_TOOLS: &[(&str, &str)] = &[
    ("build", "vite"),
    ("check", "oxlint"),
    ("dev", "vite"),
    ("fmt", "oxfmt"),
    ("format", "oxfmt"),
    ("lint", "oxlint"),
    ("pack", "tsdown"),
    ("preview", "vite"),
    ("test", "vitest"),
];

/// Flags of `vp run` that take no value and select no package.
const RUN_BOOLEAN_FLAGS: &[&str] = &[
    "-t",
    "--transitive",
    "--parallel",
    "--cache",
    "--no-cache",
    "-v",
    "--verbose",
    "--last-details",
];

/// Flags of `vp run` that take a value and select no package.
const RUN_VALUE_FLAGS: &[&str] = &["--concurrency-limit"];

/// Flags of `vp exec` and `vpx` that take a value and select no package.
const EXEC_VALUE_FLAGS: &[&str] = &["-p", "--package"];

/// Flags of `vp exec` that run the binary in other workspace packages. The
/// file arguments of such a command do not resolve in this package.
const EXEC_SELECTION_FLAGS: &[&str] = &["-r", "--recursive", "-F", "--filter"];

/// The tool that reads the arguments of the `vp` command at `idx`, and the
/// index of its subcommand. `None` when the binary at `idx` is not `vp`.
/// The tool is `None` for a subcommand that takes no file arguments.
pub(super) fn subcommand_tool(
    tokens: &[&str],
    idx: usize,
) -> Option<(Option<&'static str>, usize)> {
    if super::tool_name(tokens.get(idx)?) != VP {
        return None;
    }
    let sub_idx = idx + 1;
    let tool = tokens.get(sub_idx).and_then(|sub| {
        SUBCOMMAND_TOOLS
            .iter()
            .find_map(|(name, tool)| (name == sub).then_some(*tool))
    });
    Some((tool, sub_idx))
}

/// The command whose file-argument rules apply to the command at `idx`, and
/// its index, for the file reference scan of entry discovery. `vp lint` gives
/// `oxlint` at the subcommand. `None` for a `vp` subcommand that takes no file
/// arguments, such as `vp run`. `vp exec` and any other command stay as they
/// are, because the scan reads the child command of `vp exec` too.
#[must_use]
pub fn scan_command<'a>(tokens: &[&'a str], idx: usize) -> Option<(&'a str, usize)> {
    let unchanged = Some((*tokens.get(idx)?, idx));
    if matches!(tokens.get(idx + 1), Some(&("exec" | "dlx"))) {
        return unchanged;
    }
    match subcommand_tool(tokens, idx) {
        Some((Some(tool), sub_idx)) => Some((tool, sub_idx)),
        Some((None, _)) => None,
        None => unchanged,
    }
}

/// The index of the first token after the `vp run` flags from `idx`. The
/// selection flags record the packages in `location`, as for pnpm.
pub(super) fn skip_run_flags(tokens: &[&str], mut idx: usize, location: &mut RunLocation) -> usize {
    while let Some(&token) = tokens.get(idx) {
        match token {
            "-r" | "--recursive" => location.select_all_packages(),
            "-w" | "--workspace-root" => location.select_package(PackageSelector::root()),
            "-F" | "--filter" => {
                select_filter(location, tokens.get(idx + 1).copied());
                idx += 1;
            }
            _ if RUN_VALUE_FLAGS.contains(&token) => idx += 1,
            _ if RUN_BOOLEAN_FLAGS.contains(&token) => {}
            _ => {
                if let Some(("--filter", value)) = token.split_once('=') {
                    select_filter(location, Some(value));
                } else if !token.starts_with("--concurrency-limit=") {
                    break;
                }
            }
        }
        idx += 1;
    }
    idx.min(tokens.len())
}

fn select_filter(location: &mut RunLocation, value: Option<&str>) {
    location.select_package(PackageSelector::pnpm_filter(value.unwrap_or_default()));
}

/// The first token of the command that `vp exec`, `vp dlx` or `vpx` at
/// `idx` runs. `None` for any other command, and for a command that runs in
/// other workspace packages.
pub(super) fn exec_child_index(tokens: &[&str], idx: usize) -> Option<usize> {
    let mut next = match (*tokens.get(idx)?, tokens.get(idx + 1).copied()) {
        (VPX, _) => idx + 1,
        (VP, Some("exec" | "dlx")) => idx + 2,
        _ => return None,
    };
    while let Some(&token) = tokens.get(next) {
        if token == "--" {
            next += 1;
            break;
        }
        if !token.starts_with('-') {
            break;
        }
        let flag = token.split_once('=').map_or(token, |(flag, _)| flag);
        if EXEC_SELECTION_FLAGS.contains(&flag) {
            return None;
        }
        next += if EXEC_VALUE_FLAGS.contains(&token) {
            2
        } else {
            1
        };
    }
    (next < tokens.len()).then_some(next)
}

/// The root `vite.config.*` of `dir` that Vite+ loads, with its source.
fn read_vite_config(dir: &Path) -> Option<(std::path::PathBuf, String)> {
    VITE_CONFIG_FILES.iter().find_map(|name| {
        let path = dir.join(name);
        let source = std::fs::read_to_string(&path).ok()?;
        Some((path, source))
    })
}

/// The commands of the `staged` block of the Vite+ config in `dir`. A value
/// is a command or an array of commands. A function value is skipped.
#[must_use]
pub fn staged_commands(dir: &Path) -> Vec<String> {
    let Some((path, source)) = read_vite_config(dir) else {
        return Vec::new();
    };
    config_parser::extract_from_source(&source, &path, |program| {
        let config = config_parser::find_config_object(program)?;
        let staged = config_parser::property_object(config, "staged")?;
        Some(
            staged
                .properties
                .iter()
                .filter_map(|property| property.as_property())
                .flat_map(|property| command_values(&property.value))
                .collect(),
        )
    })
    .unwrap_or_default()
}

/// The commands of the `run.tasks` block of the Vite+ config in `dir`, keyed
/// by task name. A task is a command string, or an object with a `command`
/// string or array. An array runs its commands one after the other. A task
/// with a `cwd` runs in another directory, so a path in it would resolve
/// against the wrong directory. Such a task gives no command.
#[must_use]
pub fn task_commands(dir: &Path) -> Vec<(String, String)> {
    let Some((path, source)) = read_vite_config(dir) else {
        return Vec::new();
    };
    config_parser::extract_from_source(&source, &path, |program| {
        let config = config_parser::find_config_object(program)?;
        let tasks = config_parser::property_object(config, "run")
            .and_then(|run| config_parser::property_object(run, "tasks"))?;
        Some(
            tasks
                .properties
                .iter()
                .filter_map(|property| property.as_property())
                .filter_map(|property| {
                    let name = property.key.static_name()?.text().into_owned();
                    let command = task_command(&property.value)?;
                    Some((name, command))
                })
                .collect(),
        )
    })
    .unwrap_or_default()
}

fn task_command(value: &Expression<'_>) -> Option<String> {
    let command = match config_parser::object_expression(value) {
        Some(task) => task_object_command(task)?,
        None => command_values(value),
    };
    (!command.is_empty()).then(|| command.join(" && "))
}

fn task_object_command(task: &ObjectExpression<'_>) -> Option<Vec<String>> {
    if config_parser::find_property(task, "cwd").is_some() {
        return None;
    }
    config_parser::property_expr(task, "command").map(command_values)
}

/// The command strings of a string or an array of strings.
fn command_values(value: &Expression<'_>) -> Vec<String> {
    match config_parser::array_expression(value) {
        Some(array) => array
            .elements
            .iter()
            .filter_map(|element| element.as_expression())
            .filter_map(config_parser::expression_to_string)
            .collect(),
        None => config_parser::expression_to_string(value)
            .into_iter()
            .collect(),
    }
}

#[cfg(test)]
#[path = "vite_plus_tests.rs"]
mod tests;
