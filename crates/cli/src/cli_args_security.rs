//! Arguments of the `fallow security` subcommands.

use super::*;

#[derive(Subcommand)]
pub enum SecuritySubcommand {
    /// Render verifier-retained survivor candidates from fallow output plus verifier verdicts.
    Survivors {
        /// Raw `fallow security --format json` candidate output.
        #[arg(long, value_name = "PATH")]
        candidates: PathBuf,
        /// Verifier verdict JSON file.
        #[arg(long, value_name = "PATH")]
        verdicts: PathBuf,
        /// Fail when any candidate has no matching verdict.
        #[arg(long)]
        require_verdict_for_each_candidate: bool,
    },
    /// Group unresolved security callees into actionable blind-spot output.
    #[command(name = "blind-spots")]
    BlindSpots {
        /// Scope diagnostics to selected files.
        #[arg(long, value_name = "PATH")]
        file: Vec<PathBuf>,
    },
}
