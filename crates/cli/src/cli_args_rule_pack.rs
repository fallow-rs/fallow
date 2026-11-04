//! Arguments of `fallow rule-pack`.

use super::*;

#[derive(Subcommand)]
pub enum RulePackCli {
    /// Scaffold a new rule pack file and wire it into the config
    Init {
        /// Pack name (default: the template name, or "team-policy")
        name: Option<String>,

        /// Template: starter, ai-safe-repo, side-effect-free-domain, clean-architecture, next-app-router
        #[arg(long, default_value = "starter")]
        template: String,

        /// Directory for the pack file, relative to the project root
        #[arg(long, default_value = "rule-packs")]
        dir: String,

        /// Only write the pack file; do not modify the config
        #[arg(long)]
        no_config: bool,
    },

    /// List configured rule packs and their rules
    List,

    /// Evaluate a pack (or all configured packs) against this project and print matches
    Test {
        /// Path to a pack file to test in isolation (default: all configured packs)
        pack: Option<PathBuf>,
    },

    /// Print the JSON Schema for rule pack files
    Schema,
}
