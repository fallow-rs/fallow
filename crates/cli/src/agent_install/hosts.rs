//! Harness detection for `fallow agent`.
//!
//! Detection is capability-based (which config directories exist, which
//! session variables are set), separate from the vendor sniffing telemetry
//! does. A harness is selected when any project, home, or session signal is
//! present; nothing is fabricated when no signal exists.

use std::path::Path;

use serde::Serialize;

use super::Harness;

/// Why a harness was selected.
#[derive(Clone, Debug, Serialize)]
pub struct Detection {
    pub harness: Harness,
    pub evidence: Vec<String>,
}

/// Detect harnesses from the project root, the home directory, and the
/// process environment.
pub fn detect(root: &Path, home: Option<&Path>) -> Vec<Detection> {
    detect_with(root, home, |key| {
        std::env::var_os(key).is_some_and(|value| !value.is_empty())
    })
}

/// Detection with an injectable environment probe for tests.
pub fn detect_with(
    root: &Path,
    home: Option<&Path>,
    env_set: impl Fn(&str) -> bool,
) -> Vec<Detection> {
    let mut found: Vec<Detection> = Vec::new();
    for harness in Harness::ALL {
        let mut evidence: Vec<String> = Vec::new();
        for (relative, kind) in project_signals(harness) {
            let path = root.join(relative);
            let hit = match kind {
                SignalKind::Dir => path.is_dir(),
                SignalKind::File => path.is_file(),
            };
            if hit {
                evidence.push(relative.to_string());
            }
        }
        if let Some(home) = home {
            let relative = home_signal(harness);
            if home.join(relative).is_dir() {
                evidence.push(format!("~/{relative}"));
            }
        }
        let session = session_signal(harness);
        if env_set(session) {
            evidence.push(format!("${session}"));
        }
        if !evidence.is_empty() {
            found.push(Detection { harness, evidence });
        }
    }
    found
}

#[derive(Clone, Copy)]
enum SignalKind {
    Dir,
    File,
}

const fn project_signals(harness: Harness) -> &'static [(&'static str, SignalKind)] {
    match harness {
        Harness::Claude => &[
            (".claude", SignalKind::Dir),
            ("CLAUDE.md", SignalKind::File),
            (".mcp.json", SignalKind::File),
        ],
        Harness::Codex => &[(".codex", SignalKind::Dir), ("AGENTS.md", SignalKind::File)],
        Harness::Cursor => &[(".cursor", SignalKind::Dir)],
    }
}

const fn home_signal(harness: Harness) -> &'static str {
    match harness {
        Harness::Claude => ".claude",
        Harness::Codex => ".codex",
        Harness::Cursor => ".cursor",
    }
}

/// Environment variable each harness sets in the processes it spawns.
pub const fn session_signal(harness: Harness) -> &'static str {
    match harness {
        Harness::Claude => "CLAUDECODE",
        Harness::Codex => "CODEX_THREAD_ID",
        Harness::Cursor => "CURSOR_AGENT",
    }
}
