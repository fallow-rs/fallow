use std::process::ExitCode;

/// Multicall entry for the packaged `fallow` binary.
///
/// The npm platform packages and the VS Code extension ship a single binary
/// (renamed to `fallow` at packaging time) that must answer as the CLI, the
/// LSP server, and the MCP server. The two hidden server subcommands route to
/// the bundled server entries; every other invocation delegates verbatim to
/// the fallow CLI, so `fallow --version`, `fallow dead-code`, and every other
/// command behave byte-for-byte like the standalone `fallow` CLI binary. That
/// verbatim delegation is pinned by the parity test in `tests/parity.rs`.
///
/// The server entries read `std::env::args()` directly (only to answer
/// `--version`; the protocol runs over stdin), so the leading `lsp-server` /
/// `mcp-server` token is inert for them and does not need stripping.
fn main() -> ExitCode {
    match std::env::args_os()
        .nth(1)
        .as_deref()
        .and_then(|arg| arg.to_str())
    {
        Some("lsp-server") => fallow_lsp::run_stdio_server(),
        Some("mcp-server") => fallow_mcp::run_stdio_server(),
        _ => fallow_cli::run(),
    }
}
