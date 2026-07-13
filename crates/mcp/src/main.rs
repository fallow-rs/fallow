use std::process::ExitCode;

/// Thin delegator to the MCP server library entry. The full server lives in
/// `fallow_mcp::run_stdio_server` so the multicall `fallow mcp-server`
/// subcommand can reuse the exact same runtime and stdio wiring.
fn main() -> ExitCode {
    fallow_mcp::run_stdio_server()
}
