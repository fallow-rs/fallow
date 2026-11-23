//! Commands that can write each kind of regression baseline.

use std::path::Path;

use fallow_engine::shell_quote::shell_quote;
use fallow_types::semantic::SemanticAnalysisMode;

use crate::cli_args::Cli;

/// Project and analysis arguments to replay from the original working directory.
#[derive(Clone, Copy, Default)]
pub struct RegressionArgs<'a> {
    pub root: Option<&'a Path>,
    pub config: Option<&'a Path>,
    pub type_aware: Option<bool>,
}

impl<'a> From<&'a Cli> for RegressionArgs<'a> {
    fn from(cli: &'a Cli) -> Self {
        Self {
            root: cli.root.as_deref(),
            config: cli.config.as_deref(),
            type_aware: cli.type_aware_override(),
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
                let mut command = format!("fallow {name}");
                if args.type_aware == Some(false) {
                    command.push_str(" --no-type-aware");
                } else if args.type_aware == Some(true) || mode == SemanticAnalysisMode::TypeAware {
                    command.push_str(" --type-aware");
                }
                (command, args)
            }
            Self::Flags(args) => ("fallow flags --retirement".to_owned(), args),
            Self::EntryWeight(args) => ("fallow list --entry-weight".to_owned(), args),
        };
        command.push_str(" --save-regression-baseline");
        if let Some(path) = file {
            command.push(' ');
            command.push_str(&shell_quote(&path.to_string_lossy()));
        }
        if let Some(root) = args.root {
            command.push_str(" --root ");
            command.push_str(&shell_quote(&root.to_string_lossy()));
        }
        if let Some(config) = args.config {
            command.push_str(" --config ");
            command.push_str(&shell_quote(&config.to_string_lossy()));
        }
        command
    }

    pub(super) fn regenerate(self, path: &Path) -> String {
        let save = self.save(Some(path));
        match self {
            Self::Check { .. } => format!("Regenerate it by running: {save}"),
            Self::Flags(_) | Self::EntryWeight(_) => {
                format!("Move the old file to a backup location, then run: {save}")
            }
        }
    }
}
