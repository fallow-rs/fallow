#![cfg_attr(
    test,
    allow(
        clippy::unwrap_used,
        clippy::expect_used,
        reason = "tests use unwrap and expect to keep fixture setup concise"
    )
)]

mod config;
mod config_writer;
mod external_plugin;
mod fixability;
pub mod jsonc;
pub mod levenshtein;
mod rule_pack;
mod workspace;

pub use config::*;
pub use config_writer::*;
pub use external_plugin::*;
pub use fixability::*;
pub use rule_pack::*;
pub use workspace::*;

use std::path::{Path, PathBuf};

/// Basename of the local walkthrough viewed-state ledger inside the cache dir.
const WALKTHROUGH_STATE_FILE: &str = "walkthrough-state.json";

/// Path to the local `fallow review --walkthrough` viewed-state ledger inside a
/// resolved cache directory (default `<root>/.fallow`, already gitignored).
///
/// Pure path join, mirroring the `cache.bin` / `graph-cache.bin` / `churn.bin`
/// conventions; the file IO and serde live in the CLI crate to keep this crate
/// free of side effects.
#[must_use]
pub fn walkthrough_state_path(cache_dir: &Path) -> PathBuf {
    cache_dir.join(WALKTHROUGH_STATE_FILE)
}

#[cfg(test)]
mod walkthrough_state_path_tests {
    use super::walkthrough_state_path;
    use std::path::Path;

    #[test]
    fn joins_state_file_under_cache_dir() {
        let path = walkthrough_state_path(Path::new("/project/.fallow"));
        assert_eq!(path, Path::new("/project/.fallow/walkthrough-state.json"));
    }
}
