mod cache;
mod constants;
mod model;
mod protocol;
mod setup;

use std::io::{self, BufReader, Write};
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use serde::Serialize;

use crate::cache::{ModelPaths, cache_root, inspect_cache};
use crate::constants::{
    DOWNLOAD_BYTES, EMBEDDING_SEMANTICS_VERSION, MODEL_DIMENSIONS, MODEL_HUB_URL, MODEL_ID,
    MODEL_LICENSE, MODEL_LICENSE_URL, MODEL_MAX_TOKENS, MODEL_REVISION, PROTOCOL_VERSION,
};

#[derive(Parser)]
#[command(name = "fallow-similar-code", version, about)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Report local model and protocol readiness as JSON.
    Status {
        /// Emit the status contract as JSON.
        #[arg(long)]
        json: bool,
    },
    /// Download and verify the pinned Apache-2.0 local model.
    Setup {
        /// Confirm that setup must use the local provider.
        #[arg(long, required = true)]
        local: bool,
        /// Emit the setup result as JSON.
        #[arg(long)]
        json: bool,
    },
    /// Serve versioned embed-functions requests as JSONL on stdin/stdout.
    Serve,
}

#[derive(Serialize)]
struct StatusOutput {
    protocol_version: u32,
    embedding_semantics_version: u32,
    sidecar_version: &'static str,
    model_ready: bool,
    model_id: &'static str,
    model_revision: &'static str,
    dimensions: usize,
    max_tokens: usize,
    license: &'static str,
    cache_dir: String,
    download_bytes: u64,
    analysis_offline: bool,
    integrity_verified: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    problem: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    downloaded: Option<bool>,
}

#[derive(Serialize)]
struct CliErrorOutput {
    protocol_version: u32,
    kind: &'static str,
    error: CliError,
}

#[derive(Serialize)]
struct CliError {
    code: &'static str,
    message: String,
    retryable: bool,
}

#[must_use]
pub fn run() -> ExitCode {
    let cli = Cli::parse();
    match run_command(cli.command) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            let output = CliErrorOutput {
                protocol_version: PROTOCOL_VERSION,
                kind: "similar-code-sidecar-error",
                error: CliError {
                    code: "sidecar-error",
                    message,
                    retryable: false,
                },
            };
            let _ = write_json_line(&mut io::stdout().lock(), &output);
            ExitCode::from(2)
        }
    }
}

#[expect(
    clippy::needless_pass_by_value,
    reason = "clap supplies the owned command and dispatch consumes its variants"
)]
fn run_command(command: Command) -> Result<(), String> {
    let root = cache_root()?;
    let paths = ModelPaths::from_cache_root(&root);
    match command {
        Command::Status { json: _ } => {
            let status = status_output(&paths, None);
            write_json_line(&mut io::stdout().lock(), &status)
        }
        Command::Setup { local, json: _ } => {
            if !local {
                return Err("only the explicit local provider is supported".to_string());
            }
            disclose_setup(&paths)?;
            let result = setup::install(&paths)?;
            drop(model::LocalModel::load(&paths)?);
            let status = status_output(&paths, Some(result.downloaded));
            if !status.model_ready {
                return Err(status
                    .problem
                    .unwrap_or_else(|| "model setup did not produce a ready cache".to_string()));
            }
            write_json_line(&mut io::stdout().lock(), &status)
        }
        Command::Serve => {
            let stdin = io::stdin();
            let mut input = BufReader::new(stdin.lock());
            protocol::serve(&mut input, &mut io::stdout().lock(), &paths)
        }
    }
}

fn status_output(paths: &ModelPaths, downloaded: Option<bool>) -> StatusOutput {
    let status = inspect_cache(paths, true);
    StatusOutput {
        protocol_version: PROTOCOL_VERSION,
        embedding_semantics_version: EMBEDDING_SEMANTICS_VERSION,
        sidecar_version: env!("CARGO_PKG_VERSION"),
        model_ready: status.ready,
        model_id: MODEL_ID,
        model_revision: MODEL_REVISION,
        dimensions: MODEL_DIMENSIONS,
        max_tokens: MODEL_MAX_TOKENS,
        license: MODEL_LICENSE,
        cache_dir: paths.directory.to_string_lossy().into_owned(),
        download_bytes: DOWNLOAD_BYTES,
        analysis_offline: true,
        integrity_verified: status.ready,
        problem: status.problem,
        downloaded,
    }
}

fn disclose_setup(paths: &ModelPaths) -> Result<(), String> {
    let mut stderr = io::stderr().lock();
    writeln!(stderr, "Fallow similar-code local model setup")
        .and_then(|()| writeln!(stderr, "Model: {MODEL_ID}@{MODEL_REVISION}"))
        .and_then(|()| {
            writeln!(
                stderr,
                "License: {MODEL_LICENSE} ({MODEL_LICENSE_URL})"
            )
        })
        .and_then(|()| writeln!(stderr, "Source: {MODEL_HUB_URL}"))
        .and_then(|()| writeln!(stderr, "Destination: {}", paths.directory.display()))
        .and_then(|()| writeln!(stderr, "Download: {DOWNLOAD_BYTES} bytes"))
        .and_then(|()| {
            writeln!(
                stderr,
                "Memory: allow approximately 1.2 GB peak RAM at the enforced batch size of 1."
            )
        })
        .and_then(|()| {
            writeln!(
                stderr,
                "Privacy: setup downloads model artifacts only. Analysis is offline, and source is accepted only through stdin and is never logged or cached."
            )
        })
        .map_err(|error| format!("failed to render setup disclosure: {error}"))
}

fn write_json_line(output: &mut dyn Write, value: &impl Serialize) -> Result<(), String> {
    serde_json::to_writer(&mut *output, value)
        .map_err(|error| format!("failed to serialize sidecar output: {error}"))?;
    output
        .write_all(b"\n")
        .and_then(|()| output.flush())
        .map_err(|error| format!("failed to write sidecar output: {error}"))
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        reason = "test fixture construction must fail immediately"
    )]

    use super::*;

    #[test]
    fn status_contract_reports_exact_model_provenance() {
        let directory = tempfile::tempdir().expect("tempdir");
        let paths = ModelPaths::from_cache_root(directory.path());
        let status = status_output(&paths, None);
        assert_eq!(status.protocol_version, PROTOCOL_VERSION);
        assert_eq!(
            status.embedding_semantics_version,
            EMBEDDING_SEMANTICS_VERSION
        );
        assert_eq!(status.model_revision, MODEL_REVISION);
        assert_eq!(status.dimensions, 768);
        assert_eq!(status.max_tokens, 1024);
        assert_eq!(status.license, "Apache-2.0");
        assert!(status.analysis_offline);
        assert!(!status.model_ready);
    }

    #[test]
    fn cli_accepts_api_compatibility_flags() {
        let status = Cli::try_parse_from(["fallow-similar-code", "status", "--json"])
            .expect("status arguments");
        assert!(matches!(status.command, Command::Status { json: true }));

        let setup = Cli::try_parse_from(["fallow-similar-code", "setup", "--local", "--json"])
            .expect("setup arguments");
        assert!(matches!(
            setup.command,
            Command::Setup {
                local: true,
                json: true
            }
        ));
    }
}
