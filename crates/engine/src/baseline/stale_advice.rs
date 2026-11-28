//! Commands for stale baseline entries.

use std::path::Path;

use fallow_output::BaselineMode;

use super::{BaselineKind, BaselineStaleness, BaselineStalenessWarning};
use crate::shell_quote::shell_quote;

impl BaselineKind {
    /// The command that removes only the stale entries of the baseline at
    /// `path`, in the mode that the file records. A prune in another mode
    /// refuses the file.
    #[must_use]
    pub fn prune_command(self, path: &Path, mode: BaselineMode) -> String {
        // The prune flags resolve a relative path against the project root,
        // while `--baseline` resolves it against the working directory. An
        // absolute path names the same file in both.
        let absolute = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
        let quoted = shell_quote(&absolute.display().to_string());
        format!(
            "fallow baselines prune{} --{}-baseline {quoted}",
            mode.flags(),
            self.as_str()
        )
    }

    /// The prune command with the analyzed root when it differs from the working directory.
    #[must_use]
    pub fn prune_command_for_root(self, path: &Path, mode: BaselineMode, root: &Path) -> String {
        let command = self.prune_command(path, mode);
        if std::env::current_dir().is_ok_and(|cwd| cwd == root) {
            return command;
        }
        format!("{command} --root {}", shell_quote(&root.to_string_lossy()))
    }

    /// One finding that an entry of this kind can match.
    #[must_use]
    pub const fn entry_noun(self) -> &'static str {
        match self {
            Self::DeadCode => "issue",
            Self::Dupes => "clone group",
            Self::Health => "finding",
        }
    }

    /// Print the staleness warning for a loaded baseline.
    #[expect(
        clippy::print_stderr,
        reason = "baseline warnings accompany the report on stderr"
    )]
    pub fn warn_on_staleness(
        self,
        staleness: BaselineStaleness,
        path: &Path,
        saved_mode: BaselineMode,
        root: &Path,
    ) {
        let label = match self {
            Self::DeadCode => "baseline",
            Self::Dupes => "duplication baseline",
            Self::Health => "health baseline",
        };
        let entries = staleness.entries;
        let noun = self.entry_noun();
        match staleness.warning() {
            BaselineStalenessWarning::None => {}
            BaselineStalenessWarning::ZeroOverlap => eprintln!(
                "Warning: {label} has {entries} entries but matched 0 current {noun}s. \
                 Your paths may have changed, or the baseline was saved on a different machine. \
                 Re-save with: --save-baseline {}",
                fallow_output::display_path(path),
            ),
            BaselineStalenessWarning::Partial => eprintln!(
                "Warning: {label} is partially stale: {} of {entries} entries matched no current {noun}, \
                 so the gate protects less than what was saved. Remove only the stale entries with: {}, \
                 or re-save with: --save-baseline {}",
                staleness.stale_entries(),
                fallow_output::display_text(&self.prune_command_for_root(path, saved_mode, root)),
                fallow_output::display_path(path),
            ),
        }
    }
}
