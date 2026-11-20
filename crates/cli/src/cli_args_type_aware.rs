//! The `fallow type-aware` subcommands.

use std::path::PathBuf;

use clap::Subcommand;

#[derive(Clone, Subcommand)]
pub enum TypeAwareCli {
    /// Report the availability of the type-aware companion and its version compatibility without analysis.
    Status,
    /// Install the signed type-aware companion of this version into your user directory.
    ///
    /// For a binary that is not the npm install (Homebrew, `cargo install`, a
    /// release archive). Asks Node for its platform, downloads the bundle from
    /// the GitHub release, checks its signature with the release key, and
    /// unpacks it. Honors HTTPS_PROXY, HTTP_PROXY, NO_PROXY and
    /// FALLOW_CA_BUNDLE. The type-aware companion needs Node.js 20 or later on PATH.
    Setup {
        /// Confirm non-interactively. Required when stdin is not a terminal.
        #[arg(long)]
        yes: bool,
        /// Install a bundle file of this version in place of a download. The
        /// signature must be next to it as `<file>.sig`.
        #[arg(long, value_name = "FILE")]
        from: Option<PathBuf>,
    },
    /// Remove the type-aware companions of other versions from the setup directory.
    ///
    /// Keeps the type-aware companion that Fallow uses now.
    Cleanup {
        /// Confirm non-interactively. Required when stdin is not a terminal.
        #[arg(long)]
        yes: bool,
    },
}
