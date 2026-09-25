//! Decide which diagnostics one analysis run sends to the client.
//!
//! The server and the save-to-publish lab bench share these functions, so the
//! bench counts the same publishes that an editor receives. The functions
//! update the pull cache and return the messages to send. They do not send
//! anything, so they hold no lock across an `await`.

use std::ops::Index;

use ls_types::{Diagnostic, Uri};
use rustc_hash::{FxHashMap, FxHashSet};

use crate::diagnostic_filter::filter_disabled_diagnostics;
use crate::document_state::{DocumentState, VersionSnapshot, uri_is_stale};

/// The last diagnostics sent for each URI, with the document version they
/// went out with. Pull requests read it, and a run compares against it to
/// skip URIs whose diagnostics did not change.
#[derive(Debug, Default)]
pub struct DiagnosticCache {
    entries: FxHashMap<Uri, CachedDiagnostics>,
}

#[derive(Debug)]
struct CachedDiagnostics {
    version: Option<i32>,
    diagnostics: Vec<Diagnostic>,
    /// The push namespace of the client holds these diagnostics. The server
    /// clears that namespace for an open document of a pull client, so the
    /// entry can be current for pulls but not for pushes.
    pushed: bool,
}

impl DiagnosticCache {
    pub fn get(&self, uri: &Uri) -> Option<&Vec<Diagnostic>> {
        self.entries.get(uri).map(|entry| &entry.diagnostics)
    }

    #[cfg(test)]
    pub fn contains_key(&self, uri: &Uri) -> bool {
        self.entries.contains_key(uri)
    }

    pub fn insert(&mut self, uri: Uri, version: Option<i32>, diagnostics: Vec<Diagnostic>) {
        self.entries.insert(
            uri,
            CachedDiagnostics {
                version,
                diagnostics,
                pushed: true,
            },
        );
    }

    /// The client may no longer hold the pushed diagnostics for `uri`. The
    /// entry stays for pull requests, but the next run sends it again.
    pub fn forget_push(&mut self, uri: &Uri) {
        if let Some(entry) = self.entries.get_mut(uri) {
            entry.pushed = false;
        }
    }

    pub fn remove(&mut self, uri: &Uri) {
        self.entries.remove(uri);
    }

    #[cfg(all(test, windows))]
    pub fn iter(&self) -> impl Iterator<Item = (&Uri, &Vec<Diagnostic>)> {
        self.entries
            .iter()
            .map(|(uri, entry)| (uri, &entry.diagnostics))
    }

    /// Whether `uri` already went out with these diagnostics and this version.
    fn holds(&self, uri: &Uri, version: Option<i32>, diagnostics: &[Diagnostic]) -> bool {
        self.entries.get(uri).is_some_and(|entry| {
            entry.pushed && entry.version == version && entry.diagnostics.as_slice() == diagnostics
        })
    }
}

impl Index<&Uri> for DiagnosticCache {
    type Output = Vec<Diagnostic>;

    fn index(&self, uri: &Uri) -> &Self::Output {
        &self.entries[uri].diagnostics
    }
}

/// Per-run inputs that decide whether a URI is fresh enough to publish.
pub struct PublishContext<'a> {
    pub disabled: &'a FxHashSet<String>,
    pub snapshot: &'a VersionSnapshot,
    pub live_documents: &'a FxHashMap<Uri, DocumentState>,
}

/// One `textDocument/publishDiagnostics` message that a run may send.
pub struct PlannedPublish {
    pub uri: Uri,
    pub diagnostics: Vec<Diagnostic>,
    pub version: Option<i32>,
    /// The client has the document open. A pull client reads open documents
    /// from the cache, so the server does not push them.
    pub is_live: bool,
}

/// The publishes for the URIs that have findings in this run.
pub struct NewDiagnosticsPlan {
    pub publishes: Vec<PlannedPublish>,
    /// Every URI with findings in this run, stale or not. The clear step and
    /// the next run use it.
    pub new_uris: FxHashSet<Uri>,
}

/// Put the fresh diagnostics of this run into `cache` and return the
/// publishes. A stale URI keeps its last valid cache entry and gets no
/// publish. A URI whose diagnostics and version equal the cache entry also
/// gets no publish: the client already has them.
pub fn plan_new_diagnostics(
    cache: &mut DiagnosticCache,
    diagnostics_by_file: FxHashMap<Uri, Vec<Diagnostic>>,
    context: &PublishContext<'_>,
) -> NewDiagnosticsPlan {
    let mut new_uris = FxHashSet::default();
    let mut publishes = Vec::with_capacity(diagnostics_by_file.len());
    for (uri, diagnostics) in diagnostics_by_file {
        new_uris.insert(uri.clone());
        if uri_is_stale(&uri, context.snapshot, context.live_documents) {
            continue;
        }
        let filtered = filter_disabled_diagnostics(diagnostics, context.disabled);
        let version = context.snapshot.get(&uri).map(|state| state.version);
        if cache.holds(&uri, version, &filtered) {
            continue;
        }
        cache.insert(uri.clone(), version, filtered.clone());
        publishes.push(PlannedPublish {
            is_live: context.live_documents.contains_key(&uri),
            uri,
            diagnostics: filtered,
            version,
        });
    }
    NewDiagnosticsPlan {
        publishes,
        new_uris,
    }
}

/// Remove the URIs that had findings in the previous run but have none now,
/// and return an empty publish for each. A stale URI keeps its diagnostics:
/// it goes back into `new_uris`, so the next run checks it again.
pub fn plan_clears(
    cache: &mut DiagnosticCache,
    previous_uris: &FxHashSet<Uri>,
    new_uris: &mut FxHashSet<Uri>,
    context: &PublishContext<'_>,
) -> Vec<PlannedPublish> {
    let mut clears = Vec::new();
    for old_uri in previous_uris {
        if new_uris.contains(old_uri) {
            continue;
        }
        if uri_is_stale(old_uri, context.snapshot, context.live_documents) {
            new_uris.insert(old_uri.clone());
            continue;
        }
        cache.remove(old_uri);
        clears.push(PlannedPublish {
            uri: old_uri.clone(),
            diagnostics: Vec::new(),
            version: context.snapshot.get(old_uri).map(|state| state.version),
            is_live: context.live_documents.contains_key(old_uri),
        });
    }
    clears
}
