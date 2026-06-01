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
mod jsonc;
pub mod levenshtein;
mod workspace;

pub use config::*;
pub use config_writer::*;
pub use external_plugin::*;
pub use workspace::*;
