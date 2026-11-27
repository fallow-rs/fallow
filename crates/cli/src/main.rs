use std::process::ExitCode;

// The parallel analysis makes many small allocations. mimalloc handles
// them faster than the system allocators. Only binaries set this.
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

/// Thin delegator to the CLI library entry. The full clap tree and command
/// dispatch live in `fallow_cli::run` so the multicall `fallow-multicall`
/// binary can reuse the exact same command surface.
fn main() -> ExitCode {
    fallow_cli::run()
}
