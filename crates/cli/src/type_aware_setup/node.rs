//! Ask Node for the platform that the companion runs on.
//!
//! The TypeScript platform package follows the platform and architecture of
//! Node, not of Fallow. An x64 Node on an arm64 Mac needs the x64 bundle, so
//! setup asks the `node` that the companion will use.

use std::io::Read as _;
use std::process::{Command, Stdio};
use std::time::Instant;

use serde::Deserialize;

use super::NODE_PROBE_TIMEOUT;

/// Lowest Node major version that the companion supports.
const MINIMUM_NODE_MAJOR: u32 = 20;
const PROBE_SCRIPT: &str = "process.stdout.write(JSON.stringify({platform:process.platform,arch:process.arch,version:process.versions.node}))";
const MAX_PROBE_OUTPUT: u64 = 4096;

/// What Node reports about itself.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub(super) struct NodeInfo {
    pub(super) platform: String,
    pub(super) arch: String,
    pub(super) version: String,
}

pub(super) fn missing_node_message() -> String {
    format!(
        "Node.js {MINIMUM_NODE_MAJOR} or later must be on PATH. The type-aware companion runs on Node. Install Node, then run `fallow type-aware setup` again."
    )
}

/// Parse and check the output of the probe script.
pub(super) fn parse_probe(output: &[u8]) -> Result<NodeInfo, String> {
    let info: NodeInfo = serde_json::from_slice(output)
        .map_err(|error| format!("cannot read the platform that Node reports: {error}"))?;
    let major = info
        .version
        .split('.')
        .next()
        .and_then(|major| major.parse::<u32>().ok())
        .ok_or_else(|| format!("Node reports an invalid version `{}`", info.version))?;
    if major < MINIMUM_NODE_MAJOR {
        return Err(format!(
            "Node {} is on PATH, but the type-aware companion needs Node.js {MINIMUM_NODE_MAJOR} or later.",
            info.version
        ));
    }
    Ok(info)
}

/// Run `node` from PATH with the probe script. Relative PATH entries are
/// dropped, so a `node` in the working directory is never started.
pub(super) fn probe() -> Result<NodeInfo, String> {
    let path = std::env::var_os("PATH").map(|value| {
        let absolute = std::env::split_paths(&value)
            .filter(|entry| entry.is_absolute())
            .collect::<Vec<_>>();
        std::env::join_paths(absolute).unwrap_or_default()
    });
    let mut command = Command::new("node");
    command
        .arg("-e")
        .arg(PROBE_SCRIPT)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    if let Some(path) = path {
        command.env("PATH", path);
    }
    let mut child = command.spawn().map_err(|_| missing_node_message())?;
    let started = Instant::now();
    let status = loop {
        if let Some(status) = child
            .try_wait()
            .map_err(|error| format!("cannot run node: {error}"))?
        {
            break status;
        }
        if started.elapsed() > NODE_PROBE_TIMEOUT {
            let _ = child.kill();
            let _ = child.wait();
            return Err("node did not report its platform in time".to_owned());
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    };
    let mut output = Vec::new();
    if let Some(stdout) = child.stdout.take() {
        stdout
            .take(MAX_PROBE_OUTPUT)
            .read_to_end(&mut output)
            .map_err(|error| format!("cannot read the output of node: {error}"))?;
    }
    if !status.success() {
        return Err(missing_node_message());
    }
    parse_probe(&output)
}
