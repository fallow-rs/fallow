use ls_types::Uri;
use rustc_hash::FxHashMap;

/// Per-document state tracked by the LSP: the `version` integer supplied by
/// the client on every `did_open` / `did_change` plus the latest text. The
/// version is the load-bearing piece for the staleness check in
/// diagnostic publishing; see `.claude/rules/lsp-server.md` for the
/// "diagnostic publish staleness" invariant.
#[derive(Debug, Clone)]
pub struct DocumentState {
    pub version: i32,
    pub text: String,
}

/// Per-URI document state captured at `run_analysis` entry, threaded through to
/// diagnostic publishing so it can drop per-URI publishes whose open buffer
/// differs from what the disk-based analyzer saw.
#[derive(Debug, Clone, Copy)]
pub struct DocumentSnapshot {
    pub version: i32,
    pub matches_disk: bool,
}

pub type VersionSnapshot = FxHashMap<Uri, DocumentSnapshot>;

pub fn document_matches_disk(uri: &Uri, text: &str) -> bool {
    uri.to_file_path()
        .and_then(|path| std::fs::read_to_string(path).ok())
        .is_some_and(|disk_text| disk_text == text)
}

fn opened_mid_run_buffer_matches_disk(uri: &Uri, state: &DocumentState) -> bool {
    document_matches_disk(uri, &state.text)
}

/// Decide whether a URI is stale relative to a captured version snapshot.
///
/// A URI is stale when we cannot prove that the analysis ran against the same
/// document state the LSP currently holds for that URI. Three conditions count:
///   1. The URI was in the snapshot and the live version advanced past it
///      (strict `>`; equal versions mean the same document state). The user
///      edited the file during the analysis run.
///   2. The URI was in the snapshot and the live document is now absent
///      (closed via `did_close` between snapshot and publish; we cannot prove
///      the client still owns the document).
///   3. The URI is absent from the snapshot but present in `live_documents`
///      and the live buffer differs from the on-disk file (opened or edited
///      between snapshot and publish; the analysis ran without seeing the
///      buffer the client now holds). If the live buffer still matches disk,
///      the analysis did see the same text and the URI is safe to publish/cache.
///
/// URIs absent from both the snapshot and `live_documents` are not stale:
/// these are cross-file diagnostics anchored to files the user never
/// `did_open`'d via the LSP (for example `package.json` for unlisted
/// dependencies or `pnpm-workspace.yaml` for catalog references). No version
/// race exists for them.
pub fn uri_is_stale(
    uri: &Uri,
    snapshot: &VersionSnapshot,
    live_documents: &FxHashMap<Uri, DocumentState>,
) -> bool {
    match (snapshot.get(uri), live_documents.get(uri)) {
        (Some(snapshot_state), Some(live_state)) => {
            !snapshot_state.matches_disk || live_state.version > snapshot_state.version
        }
        (Some(_), None) => true,
        (None, Some(live_state)) => !opened_mid_run_buffer_matches_disk(uri, live_state),
        (None, None) => false,
    }
}
