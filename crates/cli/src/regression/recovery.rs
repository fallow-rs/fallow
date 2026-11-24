//! Commands that can write each kind of regression baseline.

use std::fmt::Write as _;
use std::path::Path;

use clap::ValueEnum as _;
use fallow_engine::shell_quote::shell_quote;
use fallow_types::semantic::SemanticAnalysisMode;

use crate::cli_args::Cli;
use crate::flags::{FlagAgeArg, RetirementArgs};

/// Project and analysis arguments to replay from the original working directory.
#[derive(Clone, Copy, Default)]
pub struct RegressionArgs<'a> {
    cli: Option<&'a Cli>,
    pub retirement: Option<&'a RetirementArgs>,
    pub production: Option<bool>,
    pub selectors: &'a [&'static str],
    pub syntactic_fallback: bool,
    pub combined_health: bool,
}

impl<'a> From<&'a Cli> for RegressionArgs<'a> {
    fn from(cli: &'a Cli) -> Self {
        Self {
            cli: Some(cli),
            retirement: None,
            production: if cli.production {
                Some(true)
            } else if cli.no_production {
                Some(false)
            } else {
                None
            },
            selectors: &[],
            syntactic_fallback: false,
            combined_health: false,
        }
    }
}

#[derive(Clone, Copy)]
pub(super) enum BaselineCommand<'a> {
    Check {
        name: &'a str,
        mode: SemanticAnalysisMode,
        args: RegressionArgs<'a>,
    },
    Flags(RegressionArgs<'a>),
    EntryWeight(RegressionArgs<'a>),
}

impl BaselineCommand<'_> {
    pub(super) fn save(self, file: Option<&Path>) -> String {
        let (mut command, args) = match self {
            Self::Check { name, mode, args } => {
                // Health adds type coupling to the semantic identity of a combined run.
                let mut command = if args.combined_health && mode == SemanticAnalysisMode::TypeAware
                {
                    "fallow --only dead-code,health".to_owned()
                } else {
                    format!("fallow {name}")
                };
                let type_aware = args.cli.and_then(Cli::type_aware_override);
                if type_aware == Some(false) {
                    command.push_str(" --no-type-aware");
                } else if type_aware == Some(true) || mode == SemanticAnalysisMode::TypeAware {
                    command.push_str(" --type-aware");
                }
                (command, args)
            }
            Self::Flags(args) => (flags_command(args.retirement), args),
            Self::EntryWeight(args) => ("fallow list --entry-weight".to_owned(), args),
        };
        if let Some(path) = file {
            append_path(&mut command, "--save-regression-baseline", path);
        } else {
            command.push_str(match self {
                Self::Check { .. } => " --save-regression-baseline",
                Self::Flags(_) | Self::EntryWeight(_) => " --save-regression-baseline <PATH>",
            });
        }
        if let Some(cli) = args.cli {
            append_scope(&mut command, cli, self, args);
        }
        command
    }

    pub(super) fn save_hint(self, instruction: &str, path: Option<&Path>) -> String {
        let instruction = match self {
            Self::Check { args, .. } if args.syntactic_fallback => {
                "The comparison ran in fallback syntactic mode. When the type-aware sidecar is available, run"
            }
            _ => instruction,
        };
        format!("{instruction}: {}", self.save(path))
    }

    pub(super) fn regenerate(self, path: &Path) -> String {
        let instruction = match self {
            Self::Check { .. } => "Regenerate it by running",
            Self::Flags(_) | Self::EntryWeight(_) => {
                "Move the old file to a backup location, then run"
            }
        };
        self.save_hint(instruction, Some(path))
    }
}

fn flags_command(retirement: Option<&RetirementArgs>) -> String {
    let mut command = "fallow flags --retirement".to_owned();
    if let Some(args) = retirement {
        if let Some(path) = &args.flag_state {
            append_path(&mut command, "--flag-state", path);
        }
        match args.flag_age {
            FlagAgeArg::Blame => {}
            FlagAgeArg::Off => command.push_str(" --flag-age off"),
            FlagAgeArg::Pickaxe => command.push_str(" --flag-age pickaxe"),
        }
    }
    command
}

fn append_path(command: &mut String, flag: &str, path: &Path) {
    let path = path.to_string_lossy();
    command.push(' ');
    command.push_str(flag);
    command.push(if path.starts_with('-') { '=' } else { ' ' });
    command.push_str(&shell_quote(&path));
}

fn append_scope(
    command: &mut String,
    cli: &Cli,
    baseline: BaselineCommand<'_>,
    args: RegressionArgs<'_>,
) {
    if let Some(root) = &cli.root {
        append_path(command, "--root", root);
    }
    if let Some(config) = &cli.config {
        append_path(command, "--config", config);
    }
    if cli.allow_remote_extends {
        command.push_str(" --allow-remote-extends");
    }
    if let Some(production) = args.production {
        command.push_str(if production {
            " --production"
        } else {
            " --no-production"
        });
    }
    if let Some(size) = cli.max_file_size {
        let _ = write!(command, " --max-file-size {size}");
    }
    if cli.no_package_baselines {
        command.push_str(" --no-package-baselines");
    }
    if !matches!(baseline, BaselineCommand::Check { .. }) {
        return;
    }
    for flag in args.selectors {
        command.push(' ');
        command.push_str(flag);
    }
    if cli.include_entry_exports {
        command.push_str(" --include-entry-exports");
    }
    if cli.show_cascade {
        command.push_str(" --show-cascade");
    }
    if let Some(path) = &cli.diff_file {
        append_path(command, "--diff-file", path);
    }
    if cli.diff_stdin {
        command.push_str(" --diff-stdin");
    }
    if let Some(path) = &cli.baseline {
        append_path(command, "--baseline", path);
    }
    if matches!(
        baseline,
        BaselineCommand::Check {
            mode: SemanticAnalysisMode::TypeAware,
            ..
        }
    ) {
        if let Some(require) = cli
            .type_aware_require
            .and_then(|value| value.to_possible_value())
        {
            let _ = write!(command, " --type-aware-require {}", require.get_name());
        }
        for path in &cli.type_aware_project {
            append_path(command, "--type-aware-project", path);
        }
    }
}
