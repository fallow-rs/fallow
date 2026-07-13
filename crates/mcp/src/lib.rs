#![cfg_attr(
    test,
    allow(
        clippy::unwrap_used,
        clippy::expect_used,
        reason = "tests use unwrap and expect to keep fixture setup concise"
    )
)]

use std::process::ExitCode;

use rmcp::ServiceExt;
use rmcp::transport::stdio;
use tracing_subscriber::EnvFilter;

mod params;
mod server;
mod tools;

/// Run the MCP stdio server and return the process exit code.
///
/// The standalone `fallow-mcp` binary and the multicall `fallow mcp-server`
/// subcommand both delegate here, so the runtime construction, stdio wiring,
/// and version-probe semantics stay identical regardless of entry point. A
/// dedicated multi-threaded runtime is built here (rather than via
/// `#[tokio::main]`) so the synchronous CLI can call this without an ambient
/// async context.
pub fn run_stdio_server() -> ExitCode {
    // Honor `--version` / `-V` / `-v` before starting the stdio server, so a
    // version probe gets a parseable `<bin> <version>` line instead of the
    // server hanging on an absent MCP handshake. Matches the CLI's clap shape.
    // The multicall entry passes argv as `fallow mcp-server --version`, which
    // still matches here because the scan skips only the program name.
    if std::env::args()
        .skip(1)
        .any(|arg| arg == "--version" || arg == "-V" || arg == "-v")
    {
        #[expect(
            clippy::print_stdout,
            reason = "version query writes to stdout by design"
        )]
        {
            println!("fallow-mcp {}", env!("CARGO_PKG_VERSION"));
        }
        return ExitCode::SUCCESS;
    }

    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => {
            #[expect(
                clippy::print_stderr,
                reason = "startup failure diagnostic writes to stderr by design"
            )]
            {
                eprintln!("fallow-mcp: failed to start tokio runtime: {error}");
            }
            return ExitCode::FAILURE;
        }
    };

    match runtime.block_on(serve_stdio()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            #[expect(
                clippy::print_stderr,
                reason = "server failure diagnostic writes to stderr by design"
            )]
            {
                eprintln!("fallow-mcp: {error}");
            }
            ExitCode::FAILURE
        }
    }
}

/// Serve the MCP tool router over stdin/stdout until the client disconnects.
/// Split out of [`run_stdio_server`] so runtime construction stays synchronous.
async fn serve_stdio() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(EnvFilter::from_default_env())
        .with_ansi(false)
        .init();

    let server = server::FallowMcp::new();
    let service = server.serve(stdio()).await?;
    service.waiting().await?;
    Ok(())
}
