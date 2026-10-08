use std::process::ExitCode;

// musl malloc is slow when many threads allocate. mimalloc makes the
// parallel analysis several times faster on musl. Only binaries set this.
#[cfg(target_env = "musl")]
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

/// Thin delegator to the LSP server library entry. The full server lives in
/// `fallow_lsp::run_stdio_server` so the multicall `fallow lsp-server`
/// subcommand can reuse the exact same runtime and stdio wiring.
fn main() -> ExitCode {
    fallow_lsp::run_stdio_server()
}
