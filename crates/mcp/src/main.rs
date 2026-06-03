#![cfg_attr(
    test,
    allow(
        clippy::unwrap_used,
        clippy::expect_used,
        reason = "tests use unwrap and expect to keep fixture setup concise"
    )
)]

use rmcp::ServiceExt;
use rmcp::transport::stdio;
use tracing_subscriber::EnvFilter;

mod params;
mod server;
mod tools;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Honor `--version` / `-V` / `-v` before starting the stdio server, so a
    // version probe gets a parseable `<bin> <version>` line instead of the
    // server hanging on an absent MCP handshake. Matches the CLI's clap shape.
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
        return Ok(());
    }

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
