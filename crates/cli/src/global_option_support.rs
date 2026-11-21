//! The support of each global option per command.
//!
//! clap propagates a global option to every subcommand, so `fallow init
//! --save-baseline x` parses. [`GLOBAL_OPTION_SUPPORT`] states for each global
//! option what each command does with it: the command uses the
//! option ([`Support::Supported`]), the command uses it only with some output
//! formats ([`Support::SupportedWithFormats`]), the command accepts the option
//! and does not use it ([`Support::Ignored`]), or the command line fails with
//! exit 2 ([`Support::Rejected`]).
//!
//! A row names a command by its path, for example `dupes` or
//! `impact statusline`. A subcommand without an entry of its own has the
//! support of its parent command.
//!
//! The startup check ([`rejected_option`]), the CLI reference
//! (`fallow schema --cli-reference`) and the drift tests read this table.
//! A command can still reject an option for a combination with one of its own
//! options, with its own check. A supported
//! option can also need an option of the command: `list` reads the regression
//! options only with `--entry-weight`, and `flags` only with `--retirement`.
//!
//! The default of a row applies to each command that the row does not name.
//! Most rows default to [`Support::Supported`], so a row names a command as
//! ignored or rejected only where the code is known to ignore or reject the
//! option.

use clap::parser::ValueSource;
use clap::{ArgMatches, ValueEnum};
use fallow_config::OutputFormat;
use serde_json::{Value, json};

use crate::cli_format::Format;

/// The command name that the table uses for a run without a subcommand.
pub const BARE_RUN: &str = "fallow";

/// What one command does with one global option.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Support {
    /// The command uses the option.
    Supported,
    /// The command uses the option with the output formats in `formats`. With
    /// another format, the command line fails with exit 2.
    SupportedWithFormats {
        /// The output formats with which the command uses the option.
        formats: &'static [Format],
        /// The error message for another format. `<formats>` stands for the
        /// list of `formats`, and `<option>` and `<command>` as for
        /// [`Support::Rejected`].
        message: &'static str,
    },
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
    /// The command path, such as `dupes` or `impact statusline`, or
    /// [`BARE_RUN`].
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
    /// The support of `command`, which is a command path such as
    /// `impact statusline`, or [`BARE_RUN`].
    #[must_use]
    pub fn support_for(&self, command: &str) -> Support {
        self.lookup(command).0
    }

    /// The support of `command`, with the command path of the entry that
    /// decides it. The longest path with an entry decides. Without an entry,
    /// the default decides for the top-level command.
    fn lookup<'a>(&self, command: &'a str) -> (Support, &'a str) {
        let mut path = command;
        loop {
            if let Some(entry) = self
                .commands
                .iter()
                .flat_map(|group| group.iter())
                .find(|entry| entry.command == path)
            {
                return (entry.support, path);
            }
            match path.rsplit_once(' ') {
                Some((parent, _)) => path = parent,
                None => return (self.default, path),
            }
        }
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
        Support::SupportedWithFormats { formats, message } => json!({
            "status": "supported_with_formats",
            "formats": formats.iter().map(|format| format_name(*format)).collect::<Vec<_>>(),
            "message": message,
        }),
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

const fn supported_with_formats(
    command: &'static str,
    formats: &'static [Format],
    message: &'static str,
) -> CommandSupport {
    CommandSupport {
        command,
        support: Support::SupportedWithFormats { formats, message },
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
const SUBCOMMAND_MESSAGE: &str = "<option> is not valid with `fallow <command>`.";
const FORMATS_OUTPUT_FILE_MESSAGE: &str = "`fallow <command>` writes the file of `--output-file` only with <formats>. With another format it prints its output to stdout, so `--output-file` has no effect.";
const NOT_READ_MESSAGE: &str =
    "`fallow <command>` does not read `<option>`, so the option has no effect.";
const STDOUT_REPORT_MESSAGE: &str = "`fallow <command>` prints its output to stdout, so `--output-file` has no effect. Redirect stdout to a file instead.";
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

const NO_PROJECT_ROOT_REASON: &str = "The command reads no project. It accepts `--root` because a caller can pass it to each command.";
const AUDIT_CACHE_CONFIG_REASON: &str = "`fallow audit-cache` removes cache folders without the config, so a config that does not load cannot block the cleanup.";
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

/// The commands that read no project and so reject `--config` and
/// `--diff-file`.
const NO_PROJECT_COMMANDS: &[CommandSupport] = &[
    rejected("config-schema", NOT_READ_MESSAGE),
    rejected("plugin-schema", NOT_READ_MESSAGE),
    rejected("rule-pack-schema", NOT_READ_MESSAGE),
    rejected("schema", NOT_READ_MESSAGE),
    rejected("telemetry", NOT_READ_MESSAGE),
];

/// The commands that read no project. A caller can pass `--root` to each
/// command, so these commands accept it.
const NO_PROJECT_ROOT: &[CommandSupport] = &[
    ignored("config-schema", NO_PROJECT_ROOT_REASON),
    ignored("plugin-schema", NO_PROJECT_ROOT_REASON),
    ignored("rule-pack-schema", NO_PROJECT_ROOT_REASON),
    ignored("schema", NO_PROJECT_ROOT_REASON),
    ignored("telemetry", NO_PROJECT_ROOT_REASON),
];

/// The commands that read a project but load no config file.
const NO_CONFIG_COMMANDS: &[CommandSupport] = &[
    rejected("agent", NOT_READ_MESSAGE),
    ignored("audit-cache", AUDIT_CACHE_CONFIG_REASON),
    rejected("ci", NOT_READ_MESSAGE),
    rejected("ci-template", NOT_READ_MESSAGE),
    rejected("cloud", NOT_READ_MESSAGE),
    rejected("explain", NOT_READ_MESSAGE),
    rejected("hooks", NOT_READ_MESSAGE),
    rejected("impact", NOT_READ_MESSAGE),
    rejected("init", NOT_READ_MESSAGE),
    rejected("license", NOT_READ_MESSAGE),
    rejected("migrate", NOT_READ_MESSAGE),
    rejected("recommend", NOT_READ_MESSAGE),
    rejected("rule-pack schema", NOT_READ_MESSAGE),
    rejected("security survivors", NOT_READ_MESSAGE),
    rejected("setup-hooks", NOT_READ_MESSAGE),
    rejected("type-aware", NOT_READ_MESSAGE),
];

/// The commands that read `--churn-file`.
const CHURN_FILE_COMMANDS: &[CommandSupport] = &[supported(BARE_RUN), supported("health")];

/// The commands that pass `--type-aware-project` to the type-aware run.
const TYPE_AWARE_PROJECT_COMMANDS: &[CommandSupport] = &[
    supported(BARE_RUN),
    supported("dead-code"),
    supported("architecture"),
    supported("health"),
    supported("audit"),
    supported("fix"),
    supported("inspect"),
    supported("watch"),
];

const fn not_read_row(
    option: &'static str,
    commands: &'static [&'static [CommandSupport]],
) -> GlobalOptionSupport {
    GlobalOptionSupport {
        option,
        default: Support::Rejected {
            message: NOT_READ_MESSAGE,
        },
        commands,
    }
}

/// The commands that print their output to stdout and do not write the file
/// of `--output-file`, or write it only with some output formats.
const STDOUT_REPORT_COMMANDS: &[CommandSupport] = &[
    supported_with_formats("agent", &[Format::Json], FORMATS_OUTPUT_FILE_MESSAGE),
    supported_with_formats("fix", &[Format::GithubSummary], FORMATS_OUTPUT_FILE_MESSAGE),
    supported_with_formats(
        "rule-pack list",
        &[Format::Json],
        FORMATS_OUTPUT_FILE_MESSAGE,
    ),
    rejected("impact", STDOUT_REPORT_MESSAGE),
    rejected("recommend", STDOUT_REPORT_MESSAGE),
    rejected("rule-pack", STDOUT_REPORT_MESSAGE),
    rejected("suppressions", STDOUT_REPORT_MESSAGE),
    rejected("viz", STDOUT_REPORT_MESSAGE),
    rejected("workspaces", STDOUT_REPORT_MESSAGE),
];

/// The Impact statusline prints one line before any report or gate runs.
const IMPACT_STATUSLINE: &[CommandSupport] = &[rejected("impact statusline", OUTPUT_GATE_MESSAGE)];

/// The security subcommands read saved candidates or list blind spots, so
/// they have no gate, no SARIF file and no summary.
const SECURITY_SUBCOMMANDS: &[CommandSupport] = &[
    rejected("security survivors", SUBCOMMAND_MESSAGE),
    rejected("security blind-spots", SUBCOMMAND_MESSAGE),
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

/// A row of a global duplication override. Only the bare run reads the
/// global overrides. `fallow dupes` takes the same setting as its own option
/// `$local`, and the other commands read the setting from the config key
/// `$key`.
macro_rules! dupes_row {
    ($option:literal, $key:literal, $local:literal) => {
        GlobalOptionSupport {
            option: $option,
            default: Support::Rejected {
                message: concat!(
                    "`fallow <command>` does not read `",
                    $option,
                    "`. Set `",
                    $key,
                    "` in the config, or pass `",
                    $option,
                    "` to bare `fallow`."
                ),
            },
            commands: &[
                &[
                    supported(BARE_RUN),
                    rejected(
                        "dupes",
                        concat!(
                            "`fallow dupes` does not read `",
                            $option,
                            "`. Use `fallow dupes ",
                            $local,
                            "`, or set `",
                            $key,
                            "` in the config."
                        ),
                    ),
                ],
                SECURITY,
                DOCTOR,
                SIMILAR_CODE,
            ],
        }
    };
}

/// The support of each global option. The startup check reads the rows in
/// this order and reports the first rejected option. The order is the clap
/// definition order, except that `--tolerance` follows the other regression
/// options, so `--fail-on-regression`, `--regression-baseline` and
/// `--save-regression-baseline` come first in an error.
pub const GLOBAL_OPTION_SUPPORT: &[GlobalOptionSupport] = &[
    row("--root", &[NO_PROJECT_ROOT]),
    row("--config", &[NO_PROJECT_COMMANDS, NO_CONFIG_COMMANDS]),
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
    row(
        "--diff-file",
        &[
            DOCTOR,
            NO_PROJECT_COMMANDS,
            &[
                rejected("impact statusline", NOT_READ_MESSAGE),
                rejected("ci-template", NOT_READ_MESSAGE),
                rejected("migrate", NOT_READ_MESSAGE),
            ],
        ],
    ),
    row("--diff-stdin", &[DOCTOR]),
    not_read_row("--churn-file", &[CHURN_FILE_COMMANDS, DOCTOR, SIMILAR_CODE]),
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
    row("--explain", &[DOCTOR, SECURITY_SUBCOMMANDS]),
    row("--explain-skipped", &[SECURITY, DOCTOR, SIMILAR_CODE]),
    row("--summary", &[DOCTOR, SIMILAR_CODE, SECURITY_SUBCOMMANDS]),
    row(
        "--ci",
        &[
            OUTPUT_GATE_REJECTED,
            DOCTOR,
            SIMILAR_CODE,
            IMPACT_STATUSLINE,
            SECURITY_SUBCOMMANDS,
        ],
    ),
    row(
        "--fail-on-issues",
        &[
            OUTPUT_GATE_REJECTED,
            DOCTOR,
            SIMILAR_CODE,
            IMPACT_STATUSLINE,
            SECURITY_SUBCOMMANDS,
        ],
    ),
    GlobalOptionSupport {
        option: "--sarif-file",
        default: Support::Rejected {
            message: SARIF_OTHER_MESSAGE,
        },
        commands: &[
            SARIF_COMMANDS,
            OUTPUT_GATE_REJECTED,
            DOCTOR,
            SIMILAR_CODE,
            SECURITY_SUBCOMMANDS,
        ],
    },
    row(
        "--output-file",
        &[
            OUTPUT_GATE_REJECTED,
            STDOUT_REPORT_COMMANDS,
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
        "--regression-baseline",
        &[REGRESSION_COMMANDS, SECURITY, DOCTOR, SIMILAR_CODE],
    ),
    regression_row(
        "--save-regression-baseline",
        &[REGRESSION_COMMANDS, SECURITY, DOCTOR, SIMILAR_CODE],
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
    dupes_row!("--dupes-mode", "duplicates.mode", "--mode"),
    dupes_row!("--dupes-near", "duplicates.near", "--near"),
    dupes_row!("--dupes-threshold", "duplicates.threshold", "--threshold"),
    dupes_row!("--dupes-min-tokens", "duplicates.minTokens", "--min-tokens"),
    dupes_row!("--dupes-min-lines", "duplicates.minLines", "--min-lines"),
    dupes_row!(
        "--dupes-min-occurrences",
        "duplicates.minOccurrences",
        "--min-occurrences"
    ),
    dupes_row!("--dupes-skip-local", "duplicates.skipLocal", "--skip-local"),
    dupes_row!(
        "--dupes-cross-language",
        "duplicates.crossLanguage",
        "--cross-language"
    ),
    dupes_row!(
        "--dupes-ignore-imports",
        "duplicates.ignoreImports",
        "--ignore-imports"
    ),
    dupes_row!(
        "--dupes-no-ignore-imports",
        "duplicates.ignoreImports",
        "--no-ignore-imports"
    ),
    dupes_row!(
        "--dupes-ignore-symlinks",
        "duplicates.ignoreSymlinks",
        "--ignore-symlinks"
    ),
    dupes_row!(
        "--dupes-no-ignore-symlinks",
        "duplicates.ignoreSymlinks",
        "--no-ignore-symlinks"
    ),
    row("--include-entry-exports", &[SECURITY, DOCTOR, SIMILAR_CODE]),
    row("--show-cascade", &[SECURITY, DOCTOR, SIMILAR_CODE]),
    row("--type-aware", &[DOCTOR, SIMILAR_CODE]),
    row("--no-type-aware", &[DOCTOR, SIMILAR_CODE]),
    not_read_row(
        "--type-aware-project",
        &[TYPE_AWARE_PROJECT_COMMANDS, DOCTOR, SIMILAR_CODE],
    ),
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
/// the command rejects with the resolved output format `output`, or `None`
/// when the command accepts each one.
///
/// Call it with the matches of the root command, before `FromArgMatches`
/// takes the values out of them.
#[must_use]
pub fn rejected_option(
    root: &clap::Command,
    matches: &ArgMatches,
    output: OutputFormat,
) -> Option<String> {
    let (path, leaf) = command_path(matches);
    let command = if path.is_empty() {
        BARE_RUN
    } else {
        path.as_str()
    };
    GLOBAL_OPTION_SUPPORT.iter().find_map(|row| {
        let (support, decided_by) = row.lookup(command);
        let message = match support {
            Support::Rejected { message } => message,
            Support::SupportedWithFormats { formats, message }
                if !formats.iter().any(|format| same_format(*format, output)) =>
            {
                message
            }
            _ => return None,
        };
        let id = global_arg_id(root, row.option)?;
        (leaf.value_source(id) == Some(ValueSource::CommandLine)).then(|| {
            render(message, row.option, decided_by).replace("<formats>", &formats_text(support))
        })
    })
}

/// Whether the `--format` value `format` selects the output format `output`.
fn same_format(format: Format, output: OutputFormat) -> bool {
    std::mem::discriminant(&OutputFormat::from(format)) == std::mem::discriminant(&output)
}

/// The `--format` value of `format`, for example `json`.
fn format_name(format: Format) -> String {
    format
        .to_possible_value()
        .map_or_else(String::new, |value| value.get_name().to_owned())
}

/// The formats of a [`Support::SupportedWithFormats`] as message text, for
/// example `` `--format json` ``.
fn formats_text(support: Support) -> String {
    let Support::SupportedWithFormats { formats, .. } = support else {
        return String::new();
    };
    formats
        .iter()
        .map(|format| format!("`--format {}`", format_name(*format)))
        .collect::<Vec<_>>()
        .join(" or ")
}

/// The subcommand path of `matches`, such as `impact statusline`, and the
/// matches of the deepest subcommand. clap copies the value of a global
/// option to the matches of each subcommand level.
fn command_path(matches: &ArgMatches) -> (String, &ArgMatches) {
    let mut path = String::new();
    let mut current = matches;
    while let Some((name, sub)) = current.subcommand() {
        if !path.is_empty() {
            path.push(' ');
        }
        path.push_str(name);
        current = sub;
    }
    (path, current)
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
