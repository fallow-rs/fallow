use std::process::ExitCode;

// musl malloc is slow when many threads allocate. mimalloc makes the
// parallel analysis several times faster on musl. Only binaries set this.
#[cfg(target_env = "musl")]
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

/// Thin delegator to the CLI library entry. The full clap tree and command
/// dispatch live in `fallow_cli::run` so the multicall `fallow-multicall`
/// binary can reuse the exact same command surface.
fn main() -> ExitCode {
    fallow_cli::run()
}
