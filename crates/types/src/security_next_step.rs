//! The `Next:` step of a `client-server-leak` candidate.
//!
//! The human CLI output and the LSP hover print the same step, so the two
//! texts cannot drift apart.

use crate::results::{SecurityFinding, TraceHopRole};

/// The `category` of a `client-server-leak` candidate that reaches
/// server-only code.
const SERVER_ONLY_CATEGORY: &str = "server-only-import";

const SERVER_ONLY_STEP: &str = "Next: check whether this server-only code is meant to run on \
    the client. If it is pulled in only through next/dynamic(..., { ssr: false }), type-only, \
    or removed at build time, mark it as a false positive. fallow already excludes a \"use \
    server\" module whose value exports are all async functions. If the sink is a \"use \
    server\" module, move its non-action export (a top-level const, a re-export, or a default \
    value) to a server-only file.";

const DIRECT_READ_STEP: &str = "Next: check whether the env read on this line runs in code \
    that ships to the browser. If only server code runs it, or a build step removes it, mark \
    it as a false positive.";

const IMPORT_STEP: &str = "Next: check whether this import can ship a secret to the browser. \
    If it is type-only, server-only, or removed at build time, mark it as a false positive.";

/// The `Next:` step of a `client-server-leak` candidate.
///
/// A candidate whose trace is one `secret-source` hop reads the secret in the
/// client file itself, so the step talks about the env read. A longer trace
/// reaches the secret through an import, so the step talks about the import.
#[must_use]
pub fn client_server_leak_next_step(finding: &SecurityFinding) -> &'static str {
    if finding.category.as_deref() == Some(SERVER_ONLY_CATEGORY) {
        return SERVER_ONLY_STEP;
    }
    let direct_read = matches!(
        finding.trace.as_slice(),
        [hop] if hop.role == TraceHopRole::SecretSource
    );
    if direct_read {
        DIRECT_READ_STEP
    } else {
        IMPORT_STEP
    }
}
