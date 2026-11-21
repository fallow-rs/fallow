//! The support of each global option per command.
//!
//! clap propagates a global option to every subcommand, so `fallow init
//! --save-baseline x` parses. [`GLOBAL_OPTION_SUPPORT`] states for each global
//! option what each top-level command does with it: the command uses the
//! option ([`Support::Supported`]), the command accepts the option and does
//! not use it ([`Support::Ignored`]), or the command line fails with exit 2
//! ([`Support::Rejected`]).
//!
//! The startup check ([`rejected_option`]), the CLI reference
//! (`fallow schema --cli-reference`) and the drift tests read this table.
//! A command can still reject an option for one of its subcommands, or for a
//! combination with one of its own options, with its own check. A supported
//! option can also need an option of the command: `list` reads the regression
//! options only with `--entry-weight`, and `flags` only with `--retirement`.
//!
//! The default of a row applies to each command that the row does not name.
//! Most rows default to [`Support::Supported`], so a row names a command as
//! ignored or rejected only where the code is known to ignore or reject the
//! option.

use clap::ArgMatches;
use clap::parser::ValueSource;
use serde_json::{Value, json};

/// The command name that the table uses for a run without a subcommand.
pub const BARE_RUN: &str = "fallow";

/// What one command does with one global option.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Support {
    /// The command uses the option.
    Supported,
    /// The command accepts the option and does not use it.
    Ignored {
        /// Why the option has no effect on this command.
        reason: &'static str,
    },
    /// The command line fails with exit 2.
    Rejected {
        /// The error message. `<option>` stands for the long option, for
        /// example `--save-baseline`, and `<command>` for the command name.
        message: &'static str,
    },
}

/// The support of one global option for one command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CommandSupport {
    /// The top-level command name, or [`BARE_RUN`].
    pub command: &'static str,
    /// What the command does with the option.
    pub support: Support,
}

/// One row of [`GLOBAL_OPTION_SUPPORT`].
#[derive(Debug, Clone, Copy)]
pub struct GlobalOptionSupport {
    /// The long option, for example `--save-baseline`.
    pub option: &'static str,
    /// The support of each command that `commands` does not name.
    pub default: Support,
    /// The commands with a support other than `default`. One command occurs
    /// at most once over all groups.
    pub commands: &'static [&'static [CommandSupport]],
}

impl GlobalOptionSupport {
    /// The support of `command`, which is a top-level command name or
    /// [`BARE_RUN`].
    #[must_use]
    pub fn support_for(&self, command: &str) -> Support {
        self.commands
            .iter()
            .flat_map(|group| group.iter())
            .find(|entry| entry.command == command)
            .map_or(self.default, |entry| entry.support)
    }

    /// The `command_support` value of the CLI reference.
    #[must_use]
    pub fn to_json(self) -> Value {
        let commands: Vec<Value> = self
            .commands
            .iter()
            .flat_map(|group| group.iter())
            .map(|entry| {
                let mut value = support_json(entry.support);
                value["command"] = Value::from(entry.command);
                value
            })
            .collect();
        json!({
            "default": support_json(self.default),
            "commands": commands,
        })
    }
}

fn support_json(support: Support) -> Value {
    match support {
        Support::Supported => json!({ "status": "supported" }),
        Support::Ignored { reason } => json!({ "status": "ignored", "reason": reason }),
        Support::Rejected { message } => json!({ "status": "rejected", "message": message }),
    }
}

const fn supported(command: &'static str) -> CommandSupport {
    CommandSupport {
        command,
        support: Support::Supported,
    }
}

const fn ignored(command: &'static str, reason: &'static str) -> CommandSupport {
    CommandSupport {
        command,
        support: Support::Ignored { reason },
    }
}

const fn rejected(command: &'static str, message: &'static str) -> CommandSupport {
    CommandSupport {
        command,
        support: Support::Rejected { message },
    }
}

const SUPPORTED: Support = Support::Supported;

// Messages. The text of each message is part of the CLI contract.

const NO_BASELINE_MESSAGE: &str = "`fallow <command>` does not load or save a baseline, so `<option>` has no effect. Use `<option>` with bare `fallow`, `fallow dead-code`, `fallow architecture`, `fallow dupes` or `fallow health`, or use `fallow audit --dead-code-baseline`, `--health-baseline` or `--dupes-baseline`.";
const AUDIT_BASELINE_MESSAGE: &str = "audit uses per-analysis baselines. Use --dead-code-baseline, --health-baseline, or --dupes-baseline (or save them with `fallow dead-code|health|dupes --save-baseline <file>`)";
const OUTPUT_GATE_MESSAGE: &str = "--ci, --fail-on-issues, --sarif-file, and --output-file are only valid with dead-code, architecture, dupes, health, security, or bare invocation";
const BASELINES_MESSAGE: &str = "`fallow baselines prune` does not accept `<option>`. Name the files with --dead-code-baseline, --health-baseline and --dupes-baseline, or with `audit.*Baseline` in the config.";
const SECURITY_MESSAGE: &str = "<option> is not valid with `fallow security`.";
const DOCTOR_MESSAGE: &str = "<option> is not valid with `fallow doctor`.";
const DOCTOR_OUTPUT_FILE_MESSAGE: &str = "--output-file is not valid with `fallow doctor`; doctor is read-only and writes its report to stdout";
const SIMILAR_CODE_MESSAGE: &str = "<option> is not valid with this `fallow similar-code` command.";
const PARSE_ERROR_MESSAGE: &str = "--fail-on-parse-error is not valid with `fallow <command>`. Use it with dead-code, architecture, health, audit or the bare run.";
const LEAN_MESSAGE: &str =
    "--lean applies to `fallow dead-code`, `fallow health` and bare `fallow`. Remove --lean.";
const NO_REGRESSION_MESSAGE: &str = "This command has no regression baseline, so `<option>` has no effect. The regression flags work with bare `fallow`, `fallow dead-code`, `fallow architecture`, `fallow flags --retirement` and `fallow list --entry-weight`.";
const DUPES_REGRESSION_MESSAGE: &str = "`fallow <command>` has no regression baseline, so `<option>` has no effect. To gate on new clone groups, use `fallow dupes --baseline <file>`. The regression flags work with bare `fallow`, `fallow dead-code`, `fallow architecture`, `fallow flags --retirement` and `fallow list --entry-weight`.";
const HEALTH_REGRESSION_MESSAGE: &str = "`fallow <command>` has no regression baseline, so `<option>` has no effect. To gate on new health findings, use `fallow health --baseline <file>`. The regression flags work with bare `fallow`, `fallow dead-code`, `fallow architecture`, `fallow flags --retirement` and `fallow list --entry-weight`.";
const AUDIT_REGRESSION_MESSAGE: &str = "`fallow <command>` has no regression baseline, so `<option>` has no effect. The regression flags work with bare `fallow`, `fallow dead-code`, `fallow architecture`, `fallow flags --retirement` and `fallow list --entry-weight`.";
const GROUP_MESSAGE: &str = "--group is valid with `fallow health --group-by` only.";
const SARIF_COMMAND_MESSAGE: &str = "`fallow <command>` does not write a SARIF file, so `--sarif-file` has no effect. Use `--format sarif` with `--output-file`, or use `--sarif-file` with bare `fallow`, `fallow dead-code`, `fallow architecture` or `fallow security`.";
const SARIF_OTHER_MESSAGE: &str = "This command does not write a SARIF file, so `--sarif-file` has no effect. Use `--format sarif` with `--output-file`, or use `--sarif-file` with bare `fallow`, `fallow dead-code`, `fallow architecture` or `fallow security`.";

// Reasons.

const FULL_TABLES_REASON: &str = "`--full-tables` changes only the health tables of `--lean`.";
const SECURITY_TOLERANCE_REASON: &str =
    "`fallow security` has no regression gate, and `--tolerance` sets only the gate.";

// Command groups.

const SECURITY: &[CommandSupport] = &[rejected("security", SECURITY_MESSAGE)];
const DOCTOR: &[CommandSupport] = &[rejected("doctor", DOCTOR_MESSAGE)];
const SIMILAR_CODE: &[CommandSupport] = &[rejected("similar-code", SIMILAR_CODE_MESSAGE)];

/// The commands that load and save a baseline with the global options.
const BASELINE_COMMANDS: &[CommandSupport] = &[
    supported(BARE_RUN),
    supported("dead-code"),
    supported("architecture"),
    supported("dupes"),
    supported("health"),
    rejected("baselines", BASELINES_MESSAGE),
];

/// The commands that print no report and so reject the output and gate
/// options.
const OUTPUT_GATE_REJECTED: &[CommandSupport] = &[
    rejected("init", OUTPUT_GATE_MESSAGE),
    rejected("config-schema", OUTPUT_GATE_MESSAGE),
    rejected("plugin-schema", OUTPUT_GATE_MESSAGE),
    rejected("plugin-check", OUTPUT_GATE_MESSAGE),
    rejected("rule-pack-schema", OUTPUT_GATE_MESSAGE),
    rejected("schema", OUTPUT_GATE_MESSAGE),
    rejected("explain", OUTPUT_GATE_MESSAGE),
    rejected("ci-template", OUTPUT_GATE_MESSAGE),
    rejected("config", OUTPUT_GATE_MESSAGE),
    rejected("ci", OUTPUT_GATE_MESSAGE),
    rejected("list", OUTPUT_GATE_MESSAGE),
    rejected("inspect", OUTPUT_GATE_MESSAGE),
    rejected("trace", OUTPUT_GATE_MESSAGE),
    rejected("flags", OUTPUT_GATE_MESSAGE),
    rejected("migrate", OUTPUT_GATE_MESSAGE),
    rejected("license", OUTPUT_GATE_MESSAGE),
    rejected("cloud", OUTPUT_GATE_MESSAGE),
    rejected("coverage", OUTPUT_GATE_MESSAGE),
    rejected("hooks", OUTPUT_GATE_MESSAGE),
    rejected("setup-hooks", OUTPUT_GATE_MESSAGE),
    rejected("audit-cache", OUTPUT_GATE_MESSAGE),
];

/// The commands that write the file of `--sarif-file`.
const SARIF_COMMANDS: &[CommandSupport] = &[
    supported(BARE_RUN),
    supported("dead-code"),
    supported("architecture"),
    supported("security"),
    rejected("dupes", SARIF_COMMAND_MESSAGE),
    rejected("health", SARIF_COMMAND_MESSAGE),
    rejected("audit", SARIF_COMMAND_MESSAGE),
];

/// The commands that read the regression options, and the analysis commands
/// that reject them with a hint to their own baseline.
const REGRESSION_COMMANDS: &[CommandSupport] = &[
    supported(BARE_RUN),
    supported("dead-code"),
    supported("architecture"),
    supported("flags"),
    supported("list"),
    rejected("dupes", DUPES_REGRESSION_MESSAGE),
    rejected("health", HEALTH_REGRESSION_MESSAGE),
    rejected("audit", AUDIT_REGRESSION_MESSAGE),
];

const fn row(
    option: &'static str,
    commands: &'static [&'static [CommandSupport]],
) -> GlobalOptionSupport {
    GlobalOptionSupport {
        option,
        default: SUPPORTED,
        commands,
    }
}

const fn baseline_row(
    option: &'static str,
    commands: &'static [&'static [CommandSupport]],
) -> GlobalOptionSupport {
    GlobalOptionSupport {
        option,
        default: Support::Rejected {
            message: NO_BASELINE_MESSAGE,
        },
        commands,
    }
}

const fn regression_row(
    option: &'static str,
    commands: &'static [&'static [CommandSupport]],
) -> GlobalOptionSupport {
    GlobalOptionSupport {
        option,
        default: Support::Rejected {
            message: NO_REGRESSION_MESSAGE,
        },
        commands,
    }
}

/// The support of each global option, in the order of the clap definition.
pub const GLOBAL_OPTION_SUPPORT: &[GlobalOptionSupport] = &[
    row("--root", &[]),
    row("--config", &[]),
    row("--allow-remote-extends", &[DOCTOR]),
    row("--format", &[]),
    row("--pretty", &[]),
    GlobalOptionSupport {
        option: "--lean",
        default: Support::Rejected {
            message: LEAN_MESSAGE,
        },
        commands: &[&[
            supported(BARE_RUN),
            supported("dead-code"),
            supported("health"),
        ]],
    },
    GlobalOptionSupport {
        option: "--full-tables",
        default: Support::Ignored {
            reason: FULL_TABLES_REASON,
        },
        commands: &[&[supported(BARE_RUN), supported("health")]],
    },
    row("--quiet", &[]),
    row("--changed-since", &[DOCTOR]),
    row("--no-package-baselines", &[SECURITY, DOCTOR, SIMILAR_CODE]),
    row("--diff-file", &[DOCTOR]),
    row("--diff-stdin", &[DOCTOR]),
    row("--churn-file", &[DOCTOR, SIMILAR_CODE]),
    row("--max-file-size", &[DOCTOR]),
    baseline_row(
        "--baseline",
        &[
            BASELINE_COMMANDS,
            &[rejected("audit", AUDIT_BASELINE_MESSAGE)],
            SECURITY,
            DOCTOR,
            SIMILAR_CODE,
        ],
    ),
    row("--baseline-mode", &[DOCTOR, SIMILAR_CODE]),
    row("--parent-run", &[DOCTOR, SIMILAR_CODE]),
    baseline_row(
        "--save-baseline",
        &[
            BASELINE_COMMANDS,
            &[rejected("audit", AUDIT_BASELINE_MESSAGE)],
            SECURITY,
            DOCTOR,
            SIMILAR_CODE,
        ],
    ),
    row("--production", &[SECURITY, DOCTOR, SIMILAR_CODE]),
    row("--no-production", &[SECURITY, DOCTOR, SIMILAR_CODE]),
    row("--workspace", &[DOCTOR]),
    row("--changed-workspaces", &[DOCTOR]),
    row("--group-by", &[SECURITY, DOCTOR, SIMILAR_CODE]),
    GlobalOptionSupport {
        option: "--group",
        default: Support::Rejected {
            message: GROUP_MESSAGE,
        },
        commands: &[&[supported("health")]],
    },
    row("--explain", &[DOCTOR]),
    row("--explain-skipped", &[SECURITY, DOCTOR, SIMILAR_CODE]),
    row("--summary", &[DOCTOR, SIMILAR_CODE]),
    row("--ci", &[OUTPUT_GATE_REJECTED, DOCTOR, SIMILAR_CODE]),
    row(
        "--fail-on-issues",
        &[OUTPUT_GATE_REJECTED, DOCTOR, SIMILAR_CODE],
    ),
    GlobalOptionSupport {
        option: "--sarif-file",
        default: Support::Rejected {
            message: SARIF_OTHER_MESSAGE,
        },
        commands: &[SARIF_COMMANDS, OUTPUT_GATE_REJECTED, DOCTOR, SIMILAR_CODE],
    },
    row(
        "--output-file",
        &[
            OUTPUT_GATE_REJECTED,
            &[
                rejected("doctor", DOCTOR_OUTPUT_FILE_MESSAGE),
                rejected("telemetry", OUTPUT_GATE_MESSAGE),
            ],
        ],
    ),
    row("--report-path-prefix", &[DOCTOR, SIMILAR_CODE]),
    regression_row(
        "--fail-on-regression",
        &[REGRESSION_COMMANDS, SECURITY, DOCTOR, SIMILAR_CODE],
    ),
    row("--fail-on-stale-baseline", &[SECURITY, DOCTOR]),
    baseline_row(
        "--fail-on-baseline-growth",
        &[
            BASELINE_COMMANDS,
            &[supported("audit")],
            SECURITY,
            DOCTOR,
            SIMILAR_CODE,
        ],
    ),
    baseline_row(
        "--baseline-base",
        &[
            BASELINE_COMMANDS,
            &[supported("audit")],
            SECURITY,
            DOCTOR,
            SIMILAR_CODE,
        ],
    ),
    row(
        "--fail-on-parse-error",
        &[
            &[
                rejected("dupes", PARSE_ERROR_MESSAGE),
                rejected("fix", PARSE_ERROR_MESSAGE),
            ],
            SECURITY,
            DOCTOR,
        ],
    ),
    regression_row(
        "--tolerance",
        &[
            REGRESSION_COMMANDS,
            &[ignored("security", SECURITY_TOLERANCE_REASON)],
            DOCTOR,
            SIMILAR_CODE,
        ],
    ),
    regression_row(
        "--regression-baseline",
        &[REGRESSION_COMMANDS, SECURITY, DOCTOR, SIMILAR_CODE],
    ),
    regression_row(
        "--save-regression-baseline",
        &[REGRESSION_COMMANDS, SECURITY, DOCTOR, SIMILAR_CODE],
    ),
    row("--dupes-mode", &[SECURITY, DOCTOR, SIMILAR_CODE]),
    row("--dupes-near", &[SECURITY, DOCTOR, SIMILAR_CODE]),
    row("--dupes-threshold", &[SECURITY, DOCTOR, SIMILAR_CODE]),
    row("--dupes-min-tokens", &[SECURITY, DOCTOR, SIMILAR_CODE]),
    row("--dupes-min-lines", &[SECURITY, DOCTOR, SIMILAR_CODE]),
    row("--dupes-min-occurrences", &[SECURITY, DOCTOR, SIMILAR_CODE]),
    row("--dupes-skip-local", &[SECURITY, DOCTOR, SIMILAR_CODE]),
    row("--dupes-cross-language", &[SECURITY, DOCTOR, SIMILAR_CODE]),
    row("--dupes-ignore-imports", &[SECURITY, DOCTOR, SIMILAR_CODE]),
    row(
        "--dupes-no-ignore-imports",
        &[SECURITY, DOCTOR, SIMILAR_CODE],
    ),
    row("--dupes-ignore-symlinks", &[SECURITY, DOCTOR, SIMILAR_CODE]),
    row(
        "--dupes-no-ignore-symlinks",
        &[SECURITY, DOCTOR, SIMILAR_CODE],
    ),
    row("--include-entry-exports", &[SECURITY, DOCTOR, SIMILAR_CODE]),
    row("--show-cascade", &[SECURITY, DOCTOR, SIMILAR_CODE]),
    row("--type-aware", &[DOCTOR, SIMILAR_CODE]),
    row("--no-type-aware", &[DOCTOR, SIMILAR_CODE]),
    row("--type-aware-project", &[DOCTOR, SIMILAR_CODE]),
    row("--type-aware-require", &[DOCTOR, SIMILAR_CODE]),
    row("--no-cache", &[DOCTOR]),
    row("--threads", &[DOCTOR]),
    row("--performance", &[SECURITY, DOCTOR, SIMILAR_CODE]),
];

/// The row of `option`, a long option such as `--save-baseline`.
#[must_use]
pub fn option_support(option: &str) -> Option<&'static GlobalOptionSupport> {
    GLOBAL_OPTION_SUPPORT
        .iter()
        .find(|row| row.option == option)
}

/// The error message for the first global option on the command line that
/// the command rejects, or `None` when the command accepts each one.
///
/// Call it with the matches of the root command, before `FromArgMatches`
/// takes the values out of them.
#[must_use]
pub fn rejected_option(root: &clap::Command, matches: &ArgMatches) -> Option<String> {
    let command = matches.subcommand_name().unwrap_or(BARE_RUN);
    let leaf = leaf_matches(matches);
    GLOBAL_OPTION_SUPPORT.iter().find_map(|row| {
        let Support::Rejected { message } = row.support_for(command) else {
            return None;
        };
        let id = global_arg_id(root, row.option)?;
        (leaf.value_source(id) == Some(ValueSource::CommandLine))
            .then(|| render(message, row.option, command))
    })
}

/// The matches of the deepest subcommand. clap copies the value of a global
/// option to the matches of each subcommand level.
fn leaf_matches(matches: &ArgMatches) -> &ArgMatches {
    let mut current = matches;
    while let Some((_, sub)) = current.subcommand() {
        current = sub;
    }
    current
}

fn global_arg_id<'a>(root: &'a clap::Command, option: &str) -> Option<&'a str> {
    let long = option.strip_prefix("--")?;
    root.get_arguments()
        .find(|arg| arg.is_global_set() && arg.get_long() == Some(long))
        .map(|arg| arg.get_id().as_str())
}

fn render(message: &str, option: &str, command: &str) -> String {
    message
        .replace("<option>", option)
        .replace("<command>", command)
}

#[cfg(test)]
#[path = "global_option_support_tests.rs"]
mod tests;
