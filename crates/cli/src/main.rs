use std::process::ExitCode;

/// Thin delegator to the CLI library entry. The full clap tree and command
/// dispatch live in `fallow_cli::run` so the multicall `fallow-multicall`
/// binary can reuse the exact same command surface.
fn main() -> ExitCode {
    fallow_cli::run()
}
