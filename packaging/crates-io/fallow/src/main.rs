//! The `fallow` command. This crate only gives the short crate name to
//! `cargo install fallow`. The command itself lives in `fallow-cli`.

use std::process::ExitCode;

// The parallel analysis makes many small allocations. mimalloc handles
// them faster than the system allocators. Only binaries set this.
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

fn main() -> ExitCode {
    fallow_cli::run()
}
