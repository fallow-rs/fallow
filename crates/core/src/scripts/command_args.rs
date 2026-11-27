//! The path arguments that a command segment records: the file and config
//! arguments of a binary, of a Vite+ subcommand, and of the command that a
//! command wrapper runs.

use super::{
    NODE_RUNNERS, PackageManagerTarget, RunLocation, ScriptCatalog, argument_entries,
    extract_args_for_binary, invoked_command, is_task_runner, node_test, vite_plus,
    wrapped_command_target,
};

/// Return the command that receives the path arguments of `binary` at `idx`
/// in `tokens` (`vitest` for `vp test`), its file and config arguments, and
/// whether it runs the file arguments as modules.
pub(super) fn command_path_args(
    binary: &str,
    tokens: &[&str],
    idx: usize,
) -> (String, (Vec<String>, Vec<String>), bool) {
    let (command, args_idx, args) = match vite_plus::subcommand_tool(tokens, idx) {
        Some((Some(tool), sub_idx)) => (tool, sub_idx, binary_path_args(tool, tokens, sub_idx)),
        Some((None, _)) => (binary, idx, (Vec::new(), Vec::new())),
        None => (binary, idx, binary_path_args(binary, tokens, idx)),
    };
    let runs_modules = argument_entries::runs_modules(command, &tokens[args_idx + 1..]);
    (command.to_string(), args, runs_modules)
}

/// Return the file and config arguments of `binary` at `idx` in `tokens`.
fn binary_path_args(binary: &str, tokens: &[&str], idx: usize) -> (Vec<String>, Vec<String>) {
    let args = &tokens[idx + 1..];
    let (mut file_args, mut config_args) =
        extract_args_for_binary(tokens, idx + 1, NODE_RUNNERS.contains(&binary));
    file_args = argument_entries::entry_file_args(binary, args, file_args);
    if is_task_runner(binary) {
        file_args.clear();
        config_args.clear();
    }
    let file_args = node_test::with_default_test_patterns(binary, args, file_args);
    (file_args, config_args)
}

/// The file and config arguments that a command wrapper records for the
/// command it runs.
pub(super) struct WrappedCommandArgs<'a> {
    pub(super) file_args: Vec<String>,
    pub(super) config_args: Vec<String>,
    /// The name of the command that the wrapper runs.
    pub(super) command: &'a str,
    /// The subset of `file_args` that the command runs as tests.
    pub(super) test_file_args: Vec<String>,
    /// Whether the command runs `file_args` as modules.
    pub(super) runs_modules: bool,
}

impl<'a> WrappedCommandArgs<'a> {
    const fn without_args(command: &'a str) -> Self {
        Self {
            file_args: Vec::new(),
            config_args: Vec::new(),
            command,
            test_file_args: Vec::new(),
            runs_modules: false,
        }
    }
}

/// Return the file and config arguments that a command wrapper records for
/// the command it runs from `command_start`, plus the name of that command.
///
/// The arguments stay with the wrapper only when the command runs in the
/// same package as a binary. Then an executable path or a package-manager
/// form that does not resolve to a dependency keeps its entry references.
/// A declared script call and a command in another location record their
/// arguments when the segment parses the command itself.
pub(super) fn wrapped_command_args<'a>(
    tokens: &[&'a str],
    command_start: usize,
    advance_package_manager: &impl Fn(&[&str], usize, &ScriptCatalog) -> Option<PackageManagerTarget>,
    catalog: &ScriptCatalog,
) -> WrappedCommandArgs<'a> {
    let (child_idx, runs_here) =
        match wrapped_command_target(tokens, command_start, advance_package_manager, catalog) {
            Some(PackageManagerTarget::Binary(child_idx, location)) => {
                (child_idx, location == RunLocation::Here)
            }
            Some(PackageManagerTarget::Script { .. }) => {
                return WrappedCommandArgs::without_args(tokens[command_start]);
            }
            None => invoked_command(tokens, command_start, &ScriptCatalog::default())
                .map_or((command_start, true), |invoked| {
                    (invoked.index, invoked.base_dirs.is_none())
                }),
        };
    let child = tokens[child_idx];
    if !runs_here {
        return WrappedCommandArgs::without_args(child);
    }
    let (mut file_args, mut config_args) = extract_args_for_binary(tokens, command_start, false);
    file_args = argument_entries::entry_file_args(child, &tokens[child_idx + 1..], file_args);
    if is_task_runner(child) {
        file_args.clear();
        config_args.clear();
    }
    let file_args =
        node_test::with_default_test_patterns(child, &tokens[child_idx + 1..], file_args);
    WrappedCommandArgs {
        test_file_args: node_test::test_file_args(child, &tokens[child_idx + 1..], &file_args),
        runs_modules: argument_entries::runs_modules(child, &tokens[child_idx + 1..]),
        file_args,
        config_args,
        command: child,
    }
}
