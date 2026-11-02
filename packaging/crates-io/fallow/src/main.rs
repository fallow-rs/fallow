//! The `fallow` command. This crate only gives the short crate name to
//! `cargo install fallow`. The command itself lives in `fallow-cli`.

use std::process::ExitCode;

// musl malloc is slow when many threads allocate. mimalloc makes the
// parallel analysis several times faster on musl. Only binaries set this.
#[cfg(target_env = "musl")]
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

fn main() -> ExitCode {
    fallow_cli::run()
}
