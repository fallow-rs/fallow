//! File access for the Fallow Impact store: paths, the lock, the strict read
//! and the write transaction.

use super::*;

/// Path to the per-project store file in the user's private config dir, or
/// `None` when no config dir is resolvable (e.g. stripped CI env), in which
/// case Impact is inert (no persistence). NEVER writes into the analyzed repo.
pub(super) fn store_path(root: &Path) -> Option<PathBuf> {
    let (project_key, _, _) = project_identity(root);
    Some(
        impact_config_dir()?
            .join("impact")
            .join(format!("{project_key}.json")),
    )
}

/// Path to a project's legacy in-repo store (`<root>/.fallow/impact.json`),
/// read ONCE for migration into the user store, never written.
pub(super) fn legacy_store_path(root: &Path) -> PathBuf {
    root.join(".fallow").join(STORE_FILE)
}

/// An Impact store written by a newer Fallow. This build refuses to write it,
/// because a typed round trip through the older [`ImpactStore`] drops every
/// field and every record shape that this build does not know.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewerStoreSchema {
    path: PathBuf,
    schema_version: u32,
    writer_version: Option<String>,
}

impl NewerStoreSchema {
    /// The user-facing explanation, with the next step.
    #[must_use]
    pub fn message(&self) -> String {
        let writer = self
            .writer_version
            .as_deref()
            .map_or_else(String::new, |version| {
                format!(" (newest record by fallow {version})")
            });
        format!(
            "fallow impact: the store at {} has schema_version {}{writer}, but this build supports up to schema_version {}. The store is unchanged. Upgrade fallow, or use the newer fallow binary, to change or record Impact data for this project.",
            self.path.display(),
            self.schema_version,
            STORE_SCHEMA_VERSION,
        )
    }
}

/// Read only the schema marker and the newest writer version of a raw store.
/// This works when the typed parse fails, because a newer schema can change
/// the shape of a known field. Returns `None` for a supported or corrupt store.
pub(super) fn newer_schema_in(content: &str, path: &Path) -> Option<NewerStoreSchema> {
    let raw: serde_json::Value = serde_json::from_str(content).ok()?;
    let schema_version = raw.get("schema_version")?.as_u64()?;
    if schema_version <= u64::from(STORE_SCHEMA_VERSION) {
        return None;
    }
    Some(NewerStoreSchema {
        path: path.to_path_buf(),
        schema_version: u32::try_from(schema_version).unwrap_or(u32::MAX),
        writer_version: newest_writer_version(&raw),
    })
}

/// The fallow version on the newest record of either track, when present.
fn newest_writer_version(raw: &serde_json::Value) -> Option<String> {
    ["records", "project_records"]
        .iter()
        .filter_map(|track| raw.get(*track)?.as_array()?.last())
        .filter_map(|record| {
            let timestamp = record.get("timestamp")?.as_str()?;
            let version = record.get("version")?.as_str()?;
            Some((timestamp, version))
        })
        .max_by(|a, b| a.0.cmp(b.0))
        .map(|(_, version)| version.to_owned())
}

/// The advisory-lock sidecar path for a store file (`<store>.json.lock`).
pub(super) fn lock_path_for(store: &Path) -> PathBuf {
    let mut raw = store.as_os_str().to_owned();
    raw.push(".lock");
    PathBuf::from(raw)
}

/// Advisory lock serialising the load -> mutate -> save critical section of an
/// Impact record across concurrent `fallow` processes.
///
/// Two worktrees of the same repo collapse to the SAME store key (and SAME
/// store path), so a pre-commit gate firing in both at once would otherwise
/// lost-update each other (A loads, B loads, A saves, B saves => A's record is
/// dropped). Records BLOCK on this lock so a contended run is serialised rather
/// than dropped. The `.lock` sidecar is intentionally never deleted (an
/// unlinked-but-locked inode plus a racer's `O_CREAT` would split the lock
/// across two inodes); the kernel releases the lock when the handle drops,
/// including at process exit, so an abandoned record never wedges the next run.
pub(super) struct ImpactStoreLock {
    _file: std::fs::File,
}

impl ImpactStoreLock {
    /// Block until the per-project store lock for `root` is held. Best-effort:
    /// returns `None` (proceed unlocked) when no store path resolves or the lock
    /// file cannot be opened/locked, so a lock-layer failure never drops a
    /// record (it only loses the cross-worktree serialisation guarantee).
    pub(super) fn acquire(root: &Path) -> Option<Self> {
        let lock_path = lock_path_for(&store_path(root)?);
        if let Some(parent) = lock_path.parent()
            && std::fs::create_dir_all(parent).is_err()
        {
            return None;
        }
        let file = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(&lock_path)
            .ok()?;
        match file.lock() {
            Ok(()) => Some(Self { _file: file }),
            Err(err) => {
                tracing::debug!(error = %err, "could not acquire impact store lock");
                None
            }
        }
    }
}

/// The user-global Impact default, stored at `<config-dir>/fallow/impact.json`
/// (sibling to `telemetry.json`). A single toggle: when on, new projects record
/// without a per-repo `enable`. A per-repo explicit decision always wins.
#[derive(Debug, Default, Serialize, Deserialize)]
struct GlobalImpactConfig {
    #[serde(default)]
    default_enabled: bool,
}

fn global_config_path() -> Option<PathBuf> {
    Some(impact_config_dir()?.join(STORE_FILE))
}

/// Whether the user-global default is on. False when unset or unreadable.
pub(super) fn load_global_default() -> bool {
    let Some(path) = global_config_path() else {
        return false;
    };
    std::fs::read_to_string(&path)
        .ok()
        .and_then(|c| serde_json::from_str::<GlobalImpactConfig>(&c).ok())
        .is_some_and(|c| c.default_enabled)
}

/// Set the user-global default. Returns whether the value changed.
pub fn set_global_default(on: bool) -> bool {
    let was = load_global_default();
    if let Some(path) = global_config_path() {
        if let Some(parent) = path.parent()
            && std::fs::create_dir_all(parent).is_err()
        {
            return false;
        }
        let config = GlobalImpactConfig {
            default_enabled: on,
        };
        if let Ok(json) = serde_json::to_string_pretty(&config) {
            let _ = fallow_config::atomic_write(&path, json.as_bytes());
        }
    }
    was != on
}

/// The per-project store directory (`<config-dir>/fallow/impact/`), for the
/// `impact --all` human discoverability footer. `None` when no config dir.
#[must_use]
pub fn store_dir() -> Option<PathBuf> {
    impact_config_dir().map(|d| d.join("impact"))
}
