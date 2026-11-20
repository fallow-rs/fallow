//! The CLI reference document of `fallow schema --cli-reference`.
//!
//! A documentation generator reads this one JSON document to write a page per
//! command, the option tables, the exit code and environment variable tables
//! and a page per issue type. Every value comes from the live clap `Command`
//! tree, the root `--help` command groups, the exit code constants, the
//! environment variable registry, the explain registry and the MCP manifest,
//! so the document cannot drift from the binary.

use clap::{Arg, ArgAction, Command, ValueEnum};
use fallow_types::mcp_manifest::{MCP_RESOURCES, MCP_TOOLS, McpResourceInfo, McpToolInfo};
use serde_json::{Value, json};

use super::environment::ENVIRONMENT_VARIABLES;
use super::exit_code_table::{EXIT_CODES, command_exit_codes, exit_code_json};
use super::issue_labels::display_label;
use crate::cli_format::Format;
use crate::explain::{rule_by_token, rule_has_guide};

/// The version of the document shape. A change that removes or renames a
/// field raises it. A new field does not.
pub const CLI_REFERENCE_SCHEMA_VERSION: u32 = 1;

/// The arguments of `fallow schema`.
#[derive(Debug, Clone, Copy, Default, clap::Args)]
pub struct SchemaArgs {
    /// Print the CLI reference instead of the capability manifest: every command and subcommand at each depth with its usage and full help, every option, the exit codes, the environment variables, the output formats and the explanation of each issue type
    #[arg(long)]
    pub cli_reference: bool,
}

/// The `help` subcommand that clap adds to each command with subcommands.
/// `--help` documents the same text, so the reference leaves it out.
const AUTO_HELP_COMMAND: &str = "help";

/// The command groups of the root `--help`, in display order: each heading
/// with the names of its commands. Only a line that ends in `:` without
/// indentation starts a group, so the task cheat sheet above the groups,
/// whose heading ends in `...`, is skipped.
fn help_groups(cmd: &Command) -> Vec<(String, Vec<String>)> {
    let mut groups: Vec<(String, Vec<String>)> = Vec::new();
    for line in crate::cli_args::TOP_LEVEL_AFTER_LONG_HELP.lines() {
        if !line.starts_with(' ') && line.ends_with(':') {
            groups.push((line.trim_end_matches(':').to_string(), Vec::new()));
            continue;
        }
        let Some((_, names)) = groups.last_mut() else {
            continue;
        };
        let Some(name) = line
            .strip_prefix("  ")
            .and_then(|l| l.split_whitespace().next())
        else {
            continue;
        };
        if name != AUTO_HELP_COMMAND && cmd.find_subcommand(name).is_some() {
            names.push(name.to_string());
        }
    }
    groups.retain(|(_, names)| !names.is_empty());
    groups
}

fn is_auto_help(arg: &Arg) -> bool {
    arg.get_id() == "help"
        && matches!(
            arg.get_action(),
            ArgAction::Help | ArgAction::HelpShort | ArgAction::HelpLong
        )
}

fn styled(text: Option<&clap::builder::StyledStr>) -> Value {
    text.map_or(Value::Null, |text| Value::from(text.to_string()))
}

fn action_name(action: &ArgAction) -> &'static str {
    match action {
        ArgAction::Set => "set",
        ArgAction::Append => "append",
        ArgAction::SetTrue => "set-true",
        ArgAction::SetFalse => "set-false",
        ArgAction::Count => "count",
        ArgAction::Help | ArgAction::HelpShort | ArgAction::HelpLong => "help",
        ArgAction::Version => "version",
        _ => "other",
    }
}

/// The display name of an argument: `--long`, `-s`, or the positional id.
fn arg_display_name(arg: &Arg) -> String {
    if let Some(long) = arg.get_long() {
        return format!("--{long}");
    }
    if let Some(short) = arg.get_short() {
        return format!("-{short}");
    }
    arg.get_value_names()
        .and_then(|names| names.first())
        .map_or_else(|| arg.get_id().to_string(), ToString::to_string)
}

fn value_name(arg: &Arg) -> Value {
    if !arg.get_action().takes_values() {
        return Value::Null;
    }
    let name = arg.get_value_names().map_or_else(
        || arg.get_id().as_str().to_uppercase(),
        |names| {
            names
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(" ")
        },
    );
    Value::from(name)
}

/// Each value name of an option that takes values, for example `FROM` and
/// `TO` for an option that takes two values.
fn value_names(arg: &Arg) -> Vec<String> {
    if !arg.get_action().takes_values() {
        return Vec::new();
    }
    arg.get_value_names().map_or_else(
        || vec![arg.get_id().as_str().to_uppercase()],
        |names| names.iter().map(ToString::to_string).collect(),
    )
}

/// Whether the option can be given without its value, as in
/// `--save-snapshot` without a path.
fn value_optional(arg: &Arg) -> bool {
    arg.get_action().takes_values()
        && arg
            .get_num_args()
            .is_some_and(|range| range.min_values() == 0)
}

fn possible_values(arg: &Arg) -> Value {
    if !arg.get_action().takes_values() {
        return json!([]);
    }
    Value::Array(
        arg.get_possible_values()
            .iter()
            .map(|value| {
                json!({
                    "name": value.get_name(),
                    "aliases": value.get_name_and_aliases().skip(1).collect::<Vec<_>>(),
                    "help": styled(value.get_help()),
                    "hidden": value.is_hide_set(),
                })
            })
            .collect(),
    )
}

fn default_value(arg: &Arg) -> Value {
    if !arg.get_action().takes_values() {
        return Value::Null;
    }
    let defaults: Vec<String> = arg
        .get_default_values()
        .iter()
        .map(|value| value.to_string_lossy().into_owned())
        .collect();
    if defaults.is_empty() {
        Value::Null
    } else {
        Value::from(defaults.join(","))
    }
}

fn arg_aliases(arg: &Arg) -> Value {
    let visible_long: Vec<&str> = arg.get_visible_aliases().unwrap_or_default();
    let visible_short: Vec<char> = arg.get_visible_short_aliases().unwrap_or_default();
    let long = arg.get_all_aliases().unwrap_or_default().into_iter().map(
        |alias| json!({ "name": format!("--{alias}"), "visible": visible_long.contains(&alias) }),
    );
    let short = arg
        .get_all_short_aliases()
        .unwrap_or_default()
        .into_iter()
        .map(|alias| {
            json!({ "name": format!("-{alias}"), "visible": visible_short.contains(&alias) })
        });
    Value::Array(long.chain(short).collect())
}

fn is_multiple(arg: &Arg) -> bool {
    matches!(arg.get_action(), ArgAction::Append)
        || arg
            .get_num_args()
            .is_some_and(|range| range.max_values() > 1)
}

/// The `--x` flag that a `--no-x` flag turns off, or the `--no-x` flag that
/// turns `--x` off, when the command has both. Both flags take no value and
/// have the same scope: a command option never pairs with a global option,
/// so `rule-pack init --no-config` does not negate the global `--config <PATH>`.
fn negation_pair(cmd: &Command, arg: &Arg) -> (Value, Value) {
    let Some(long) = arg.get_long().filter(|_| !arg.get_action().takes_values()) else {
        return (Value::Null, Value::Null);
    };
    let has_long = |name: &str| {
        cmd.get_arguments().any(|other| {
            other.get_long() == Some(name)
                && !other.get_action().takes_values()
                && other.is_global_set() == arg.is_global_set()
        })
    };
    let negates = long
        .strip_prefix("no-")
        .filter(|positive| has_long(positive))
        .map_or(Value::Null, |positive| Value::from(format!("--{positive}")));
    let negative = format!("no-{long}");
    let negated_by = if has_long(&negative) {
        Value::from(format!("--{negative}"))
    } else {
        Value::Null
    };
    (negates, negated_by)
}

fn option_json(cmd: &Command, arg: &Arg) -> Value {
    let (negates, negated_by) = negation_pair(cmd, arg);
    let conflicts: Vec<String> = cmd
        .get_arg_conflicts_with(arg)
        .into_iter()
        .map(arg_display_name)
        .collect();
    json!({
        "id": arg.get_id().as_str(),
        "long": arg.get_long().map(|long| format!("--{long}")),
        "short": arg.get_short().map(|short| format!("-{short}")),
        "aliases": arg_aliases(arg),
        "value_name": value_name(arg),
        "value_names": value_names(arg),
        "value_optional": value_optional(arg),
        "action": action_name(arg.get_action()),
        "takes_value": arg.get_action().takes_values(),
        "multiple": is_multiple(arg),
        "possible_values": possible_values(arg),
        "default": default_value(arg),
        "env": Value::Null,
        "help": styled(arg.get_help()),
        "long_help": styled(arg.get_long_help()),
        "hidden": arg.is_hide_set(),
        "hidden_in_short_help": arg.is_hide_short_help_set(),
        "global": arg.is_global_set(),
        "help_heading": arg.get_help_heading(),
        "required": arg.is_required_set(),
        "conflicts_with": conflicts,
        "negates": negates,
        "negated_by": negated_by,
    })
}

fn positional_json(arg: &Arg) -> Value {
    json!({
        "id": arg.get_id().as_str(),
        "index": arg.get_index(),
        "value_name": value_name(arg),
        "multiple": is_multiple(arg),
        "possible_values": possible_values(arg),
        "default": default_value(arg),
        "help": styled(arg.get_help()),
        "long_help": styled(arg.get_long_help()),
        "hidden": arg.is_hide_set(),
        "required": arg.is_required_set(),
    })
}

fn usage_line(cmd: &Command) -> String {
    let usage = cmd.clone().render_usage().to_string();
    usage
        .strip_prefix("Usage: ")
        .unwrap_or(&usage)
        .trim()
        .to_string()
}

/// Whether the command writes JSON for every `--format`. `None` when the
/// command needs arguments, so a bare parse of its path cannot tell.
fn always_json(path: &str) -> Option<bool> {
    use clap::Parser;
    let argv = std::iter::once("fallow").chain(path.split(' '));
    let cli = crate::Cli::try_parse_from(argv).ok()?;
    Some(crate::cli_format::command_payload_is_json(
        &cli,
        fallow_config::OutputFormat::Human,
    ))
}

/// The options of `cmd` without the propagated global options and the
/// automatic `--help` flag.
fn own_options(cmd: &Command) -> Vec<Value> {
    cmd.get_arguments()
        .filter(|arg| !arg.is_positional() && !arg.is_global_set() && !is_auto_help(arg))
        .map(|arg| option_json(cmd, arg))
        .collect()
}

fn positionals(cmd: &Command) -> Vec<Value> {
    cmd.get_positionals().map(positional_json).collect()
}

fn command_json(cmd: &Command, path: &str, help_group: Option<&str>) -> Value {
    let subcommands: Vec<Value> = cmd
        .get_subcommands()
        .filter(|sub| sub.get_name() != AUTO_HELP_COMMAND)
        .map(|sub| command_json(sub, &format!("{path} {}", sub.get_name()), None))
        .collect();
    let visible_aliases: Vec<&str> = cmd.get_visible_aliases().collect();
    let aliases: Vec<Value> = cmd
        .get_all_aliases()
        .map(|alias| json!({ "name": alias, "visible": visible_aliases.contains(&alias) }))
        .collect();
    json!({
        "name": cmd.get_name(),
        "path": path,
        "aliases": aliases,
        "help_group": help_group,
        "about": styled(cmd.get_about()),
        "long_about": styled(cmd.get_long_about()),
        "hidden": cmd.is_hide_set(),
        "usage": usage_line(cmd),
        "always_json": always_json(path),
        "exit_codes": command_exit_codes(path),
        "positionals": positionals(cmd),
        "options": own_options(cmd),
        "subcommands": subcommands,
    })
}

/// Top-level commands in the order and the groups of the root `--help`, then
/// the commands that no group lists (hidden or deprecated commands).
fn top_level_commands(cmd: &Command) -> Vec<Value> {
    let groups = help_groups(cmd);
    let mut listed: Vec<&str> = Vec::new();
    let mut commands = Vec::new();
    for (heading, names) in &groups {
        for name in names {
            let Some(sub) = cmd.find_subcommand(name) else {
                continue;
            };
            listed.push(sub.get_name());
            commands.push(command_json(sub, sub.get_name(), Some(heading)));
        }
    }
    for sub in cmd.get_subcommands() {
        if sub.get_name() == AUTO_HELP_COMMAND || listed.contains(&sub.get_name()) {
            continue;
        }
        commands.push(command_json(sub, sub.get_name(), None));
    }
    commands
}

fn output_formats() -> Vec<Value> {
    Format::value_variants()
        .iter()
        .filter_map(ValueEnum::to_possible_value)
        .map(|value| {
            json!({
                "name": value.get_name(),
                "aliases": value.get_name_and_aliases().skip(1).collect::<Vec<_>>(),
                "help": styled(value.get_help()),
            })
        })
        .collect()
}

fn environment_variables() -> Vec<Value> {
    ENVIRONMENT_VARIABLES
        .iter()
        .map(|variable| {
            json!({
                "name": variable.name,
                "group": variable.group,
                "default": variable.default,
                "description": variable.description,
            })
        })
        .collect()
}

/// The registry rows of `fallow schema`, each with the text that
/// `fallow explain <id> --format json` prints and a sentence-case
/// `display_label`. `example` and `how_to_fix` are null when the issue type
/// has no guide of its own.
fn issue_types() -> Vec<Value> {
    let Value::Array(rows) = super::issue_types_schema() else {
        return Vec::new();
    };
    rows.into_iter()
        .map(|mut row| {
            row["explanation"] = row["rule_id"]
                .as_str()
                .map_or(Value::Null, explanation_json);
            row["display_label"] = display_label(&row);
            row
        })
        .collect()
}

fn explanation_json(rule_id: &str) -> Value {
    let (Some(rule), Ok(output)) = (
        rule_by_token(rule_id),
        fallow_api::explain_issue_type(rule_id),
    ) else {
        return Value::Null;
    };
    let has_guide = rule_has_guide(rule);
    json!({
        "id": output.id,
        "name": output.name,
        "summary": output.summary,
        "rationale": output.rationale,
        "example": has_guide.then_some(output.example),
        "how_to_fix": has_guide.then_some(output.how_to_fix),
        "docs": output.docs,
    })
}

/// Build the CLI reference document from the clap command tree.
pub fn build_cli_reference(cmd: &Command) -> Value {
    let mut root = cmd.clone();
    root.build();
    let global_options: Vec<Value> = root
        .get_arguments()
        .filter(|arg| arg.is_global_set())
        .map(|arg| option_json(&root, arg))
        .collect();
    json!({
        "schema_version": CLI_REFERENCE_SCHEMA_VERSION,
        "name": root.get_name(),
        "version": env!("CARGO_PKG_VERSION"),
        "about": styled(root.get_about()),
        "usage": usage_line(&root),
        "root_command": {
            "positionals": positionals(&root),
            "options": own_options(&root),
            "exit_codes": [],
        },
        "global_options": global_options,
        "commands": top_level_commands(&root),
        "exit_codes": EXIT_CODES.iter().map(exit_code_json).collect::<Vec<_>>(),
        "environment_variables": environment_variables(),
        "output_formats": output_formats(),
        "issue_types": issue_types(),
        "mcp_tools": MCP_TOOLS.iter().map(McpToolInfo::to_json).collect::<Vec<_>>(),
        "mcp_resources": MCP_RESOURCES.iter().map(McpResourceInfo::to_json).collect::<Vec<_>>(),
    })
}

#[cfg(test)]
#[path = "cli_reference_tests.rs"]
mod tests;
