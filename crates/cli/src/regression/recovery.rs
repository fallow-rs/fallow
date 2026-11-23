//! Commands that can write each kind of regression baseline.

use std::path::Path;

use fallow_engine::shell_quote::shell_quote;
use fallow_types::semantic::SemanticAnalysisMode;

#[derive(Clone, Copy)]
pub(super) enum BaselineCommand<'a> {
    Check {
        name: &'a str,
        mode: SemanticAnalysisMode,
        config: Option<&'a Path>,
    },
    Flags,
    EntryWeight,
}

impl BaselineCommand<'_> {
    pub(super) fn save(self, file: Option<&Path>) -> String {
        let mut command = match self {
            Self::Check { name, mode, config } => {
                let mut command = format!("fallow {name}");
                if mode == SemanticAnalysisMode::TypeAware {
                    command.push_str(" --type-aware");
                }
                if let Some(config) = config {
                    command.push_str(" --config ");
                    command.push_str(&shell_quote(&config.to_string_lossy()));
                }
                command
            }
            Self::Flags => "fallow flags --retirement".to_owned(),
            Self::EntryWeight => "fallow list --entry-weight".to_owned(),
        };
        command.push_str(" --save-regression-baseline");
        if let Some(path) = file {
            command.push(' ');
            command.push_str(&shell_quote(&path.to_string_lossy()));
        }
        command
    }

    pub(super) fn regenerate(self, path: &Path) -> String {
        let save = self.save(Some(path));
        match self {
            Self::Check { .. } => format!("Regenerate it by running: {save}"),
            Self::Flags | Self::EntryWeight => {
                format!("Move the old file to a backup location, then run: {save}")
            }
        }
    }
}
