//! File access for the Fallow Impact store: paths, the lock, the strict read
//! and the write transaction.

use super::*;
use crate::state_file::{Existing, load_existing};

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

/// The resolved per-project store-file path for `root`, for `status` display
/// (so a wrong key is debuggable). `None` when no config dir is resolvable.
#[must_use]
pub fn resolved_store_path(root: &Path) -> Option<PathBuf> {
    store_path(root)
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
fn newer_schema_in(content: &str, path: &Path) -> Option<NewerStoreSchema> {
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
        Self::acquire_for_store(&store_path(root)?)
    }

    /// Block until the lock of the store file at `store` is held.
    fn acquire_for_store(store: &Path) -> Option<Self> {
        let lock_path = lock_path_for(store);
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
///
/// # Errors
///
/// Returns [`StoreWriteError`] when no config folder resolves or the write
/// fails.
pub fn set_global_default(on: bool) -> Result<bool, StoreWriteError> {
    let was = load_global_default();
    let path = global_config_path().ok_or(StoreWriteError::NoConfigDir)?;
    let io = |source| StoreWriteError::Io {
        path: path.clone(),
        source,
    };
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(io)?;
    }
    let config = GlobalImpactConfig {
        default_enabled: on,
    };
    let json =
        serde_json::to_string_pretty(&config).map_err(|err| io(std::io::Error::other(err)))?;
    fallow_config::atomic_write(&path, json.as_bytes()).map_err(io)?;
    Ok(was != on)
}

/// The per-project store directory (`<config-dir>/fallow/impact/`), for the
/// `impact --all` human discoverability footer. `None` when no config dir.
#[must_use]
pub fn store_dir() -> Option<PathBuf> {
    impact_config_dir().map(|d| d.join("impact"))
}

/// Why a store file gives no usable store.
#[derive(Debug)]
pub enum StoreReadError {
    /// A newer Fallow wrote the store. `best_effort` holds the typed view when
    /// the content still fits the model of this build.
    Newer {
        schema: NewerStoreSchema,
        best_effort: Option<Box<ImpactStore>>,
    },
    /// The file exists and the read failed. Invalid UTF-8 is a read failure.
    Unreadable {
        path: PathBuf,
        source: std::io::Error,
    },
    /// The file exists and does not parse as a store.
    Invalid { path: PathBuf, reason: String },
}

impl StoreReadError {
    /// The user-facing explanation, with the next step.
    #[must_use]
    pub fn message(&self) -> String {
        match self {
            Self::Newer { schema, .. } => schema.message(),
            Self::Unreadable { path, source } => format!(
                "fallow impact: could not read the store at {} ({source}). The store is unchanged. Correct the access to the file and its folder, or run `fallow impact reset` to remove the store.",
                path.display()
            ),
            Self::Invalid { path, reason } => format!(
                "fallow impact: the store at {} does not parse ({reason}). The store is unchanged. Run `fallow impact enable` to replace it, or `fallow impact reset` to remove it.",
                path.display()
            ),
        }
    }
}

struct NewerStore {
    schema: NewerStoreSchema,
    best_effort: Option<Box<ImpactStore>>,
}

/// Check the raw schema marker before the typed result is used. A newer schema
/// can change the shape of a known field, so the typed parse alone cannot tell
/// a newer store from a corrupt store.
fn parse_store_text(content: &str, path: &Path) -> Result<Result<ImpactStore, NewerStore>, String> {
    let typed = serde_json::from_str::<ImpactStore>(content);
    if let Some(schema) = newer_schema_in(content, path) {
        return Ok(Err(NewerStore {
            schema,
            best_effort: typed.ok().map(Box::new),
        }));
    }
    typed.map(Ok).map_err(|err| err.to_string())
}

/// Read one store file. `Ok(None)` means that the file does not exist. This
/// function never writes and never logs.
pub(super) fn read_store_file(path: &Path) -> Result<Option<ImpactStore>, StoreReadError> {
    match load_existing(path, |content| parse_store_text(content, path)) {
        Existing::Absent => Ok(None),
        Existing::Loaded(Ok(store)) => Ok(Some(store)),
        Existing::Loaded(Err(newer)) => Err(StoreReadError::Newer {
            schema: newer.schema,
            best_effort: newer.best_effort,
        }),
        Existing::Unreadable(source) => Err(StoreReadError::Unreadable {
            path: path.to_path_buf(),
            source,
        }),
        Existing::Invalid(reason) => Err(StoreReadError::Invalid {
            path: path.to_path_buf(),
            reason,
        }),
    }
}

/// The marker that stops a second import of the legacy in-repo store. It is a
/// sibling of the store file, so it stays when the store file is removed.
pub(super) fn migration_marker_path(store: &Path) -> PathBuf {
    store.with_extension("migrated")
}

/// The legacy in-repo store as a current store, without a write. `Absent` when
/// the marker exists or the legacy file does not exist.
fn legacy_view(root: &Path, store: &Path) -> Existing<ImpactStore> {
    if migration_marker_path(store).exists() {
        return Existing::Absent;
    }
    let parsed = load_existing(&legacy_store_path(root), |content| {
        serde_json::from_str::<LegacyFlatStore>(content).map_err(|err| err.to_string())
    });
    match parsed {
        Existing::Absent => Existing::Absent,
        Existing::Unreadable(err) => Existing::Unreadable(err),
        Existing::Invalid(reason) => Existing::Invalid(reason),
        Existing::Loaded(legacy) => {
            let (_, worktree, display) = project_identity(root);
            let mut store = legacy.into_store(&worktree);
            store.label = display;
            Existing::Loaded(store)
        }
    }
}

/// Load the store for a report. This function never writes. A missing store
/// gives the legacy in-repo store when one exists and was not imported, else
/// the defaults. A store that is not usable gives the defaults and a warning.
/// A store from a newer Fallow gives a best-effort view and a warning.
pub fn load(root: &Path) -> ImpactStore {
    let Some(path) = store_path(root) else {
        return ImpactStore::default();
    };
    match read_store_file(&path) {
        Ok(Some(store)) => store,
        Ok(None) => match legacy_view(root, &path) {
            Existing::Loaded(store) => store,
            Existing::Absent | Existing::Unreadable(_) | Existing::Invalid(_) => {
                ImpactStore::default()
            }
        },
        Err(err) => {
            tracing::warn!("{}", err.message());
            match err {
                StoreReadError::Newer { best_effort, .. } => {
                    best_effort.map_or_else(ImpactStore::default, |store| *store)
                }
                StoreReadError::Unreadable { .. } | StoreReadError::Invalid { .. } => {
                    ImpactStore::default()
                }
            }
        }
    }
}

/// Stable failure states for status-bar callers. The statusline loader never
/// logs the store path or substitutes corrupt/newer data with an empty report.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatuslineLoadError {
    /// The store is unreadable or corrupt.
    DataUnavailable,
    /// The store was written by a newer Fallow whose schema this build cannot read.
    NewerSchema,
}

/// Load Impact state without writes, warnings, or path-bearing diagnostics.
///
/// A corrupt, unreadable, or newer-schema store is reported as unavailable so
/// the compact renderer cannot mislabel lost data as "off".
///
/// # Errors
///
/// Returns [`StatuslineLoadError`] when the user store or the legacy store
/// exists and is not usable.
pub fn load_statusline(root: &Path) -> Result<ImpactStore, StatuslineLoadError> {
    let Some(path) = store_path(root) else {
        return Ok(ImpactStore::default());
    };
    match read_store_file(&path) {
        Ok(Some(store)) => Ok(store),
        Ok(None) => match legacy_view(root, &path) {
            Existing::Loaded(store) => Ok(store),
            Existing::Absent => Ok(ImpactStore::default()),
            Existing::Unreadable(_) | Existing::Invalid(_) => {
                Err(StatuslineLoadError::DataUnavailable)
            }
        },
        Err(StoreReadError::Newer { .. }) => Err(StatuslineLoadError::NewerSchema),
        Err(StoreReadError::Unreadable { .. } | StoreReadError::Invalid { .. }) => {
            Err(StatuslineLoadError::DataUnavailable)
        }
    }
}

/// Enumerate every per-project store in `<config-dir>/fallow/impact/`, returning
/// `(project_key, store)` pairs plus the count of files that failed to parse.
/// Read-only; never writes. The global `impact.json` toggle is a sibling FILE of
/// this dir (one level up), so it is naturally excluded. Corrupt/newer-schema
/// files are skipped and counted, never substituted with a default store.
#[must_use]
pub(super) fn load_all() -> (Vec<(String, ImpactStore)>, usize) {
    let Some(dir) = impact_config_dir().map(|d| d.join("impact")) else {
        return (Vec::new(), 0);
    };
    let Ok(read) = std::fs::read_dir(&dir) else {
        return (Vec::new(), 0);
    };
    let mut stores = Vec::new();
    let mut unreadable = 0usize;
    for entry in read.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let Some(key) = path.file_stem().and_then(|s| s.to_str()).map(str::to_owned) else {
            continue;
        };
        match read_store_file(&path) {
            Ok(Some(store)) => stores.push((key, store)),
            Ok(None) => {}
            Err(_) => unreadable += 1,
        }
    }
    (stores, unreadable)
}

/// Why a store change was not saved.
#[derive(Debug)]
pub enum StoreWriteError {
    /// No user config folder resolves, so there is no place for the store.
    NoConfigDir,
    /// The store on disk is newer or not usable. The file is unchanged.
    Read(StoreReadError),
    /// The folder or the file could not be written.
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
}

impl StoreWriteError {
    /// The user-facing explanation.
    #[must_use]
    pub fn message(&self) -> String {
        match self {
            Self::NoConfigDir => "fallow impact: no user config folder resolves (HOME, XDG_CONFIG_HOME or APPDATA is not set). The change was not saved. Set HOME, then run the command again.".to_owned(),
            Self::Read(err) => err.message(),
            Self::Io { path, source } => format!(
                "fallow impact: could not write {}: {source}. The change was not saved. Check that the folder exists and that you can write to it, then run the command again.",
                path.display()
            ),
        }
    }
}

/// The result of the change closure of `transact`.
pub(super) enum Change<T> {
    /// Save the store, then return the value.
    Write(T),
    /// Leave the file as it is, then return the value.
    Keep(T),
}

/// What `transact` does with a store that exists and does not parse.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum OnInvalid {
    /// Stop with [`StoreWriteError::Read`]. Each routine write uses this.
    Stop,
    /// Start from an empty store. Only `fallow impact enable` uses this.
    Replace,
}

/// Give a store that never had a schema version the current one, so the
/// next read knows its shape.
pub(super) fn stamp_schema_version(store: &mut ImpactStore) {
    if store.schema_version == 0 {
        store.schema_version = STORE_SCHEMA_VERSION;
    }
}

/// Write `store` to `path` with an atomic replace.
pub(super) fn write_store(store: &ImpactStore, path: &Path) -> Result<(), StoreWriteError> {
    let io = |source| StoreWriteError::Io {
        path: path.to_path_buf(),
        source,
    };
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(io)?;
    }
    let json = serde_json::to_string_pretty(store).map_err(|err| io(std::io::Error::other(err)))?;
    fallow_config::atomic_write(path, json.as_bytes()).map_err(io)
}

/// The one way to change a project store: lock, strict read, change, atomic
/// write. A store that a newer Fallow wrote, and a store that cannot be read,
/// stop the transaction. When the user store is absent, the legacy in-repo
/// store is imported once.
///
/// `change` must not call a function of this module. A second lock on the
/// same store in one process never returns.
pub(super) fn transact<T>(
    root: &Path,
    on_invalid: OnInvalid,
    change: impl FnOnce(&mut ImpactStore) -> Change<T>,
) -> Result<T, StoreWriteError> {
    let path = store_path(root).ok_or(StoreWriteError::NoConfigDir)?;
    let _lock = ImpactStoreLock::acquire(root);
    let (mut store, imported) = match read_store_file(&path) {
        Ok(Some(store)) => (store, false),
        Ok(None) => match legacy_view(root, &path) {
            Existing::Loaded(store) => (store, true),
            Existing::Absent | Existing::Unreadable(_) | Existing::Invalid(_) => {
                (ImpactStore::default(), false)
            }
        },
        Err(StoreReadError::Invalid { .. }) if on_invalid == OnInvalid::Replace => {
            (ImpactStore::default(), false)
        }
        Err(err) => return Err(StoreWriteError::Read(err)),
    };
    let value = match change(&mut store) {
        Change::Write(value) => {
            write_store(&store, &path)?;
            value
        }
        Change::Keep(value) => {
            if imported {
                let _ = write_store(&store, &path);
            }
            value
        }
    };
    if imported && path.exists() {
        let _ = mark_migrated(&path);
    }
    Ok(value)
}

/// Record that the legacy in-repo store must not be imported again.
fn mark_migrated(store: &Path) -> Result<(), StoreWriteError> {
    let marker = migration_marker_path(store);
    let io = |source| StoreWriteError::Io {
        path: marker.clone(),
        source,
    };
    if let Some(parent) = marker.parent() {
        std::fs::create_dir_all(parent).map_err(io)?;
    }
    std::fs::write(&marker, b"").map_err(io)
}

/// Mark the project as imported, then remove its store file. `Ok(false)`
/// when the file did not exist.
fn remove_store(path: &Path) -> Result<bool, StoreWriteError> {
    let _lock = ImpactStoreLock::acquire_for_store(path);
    mark_migrated(path)?;
    match std::fs::remove_file(path) {
        Ok(()) => Ok(true),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(source) => Err(StoreWriteError::Io {
            path: path.to_path_buf(),
            source,
        }),
    }
}

/// Delete THIS project's store file. Returns whether a file was removed.
/// The import marker stays, so the legacy in-repo store is not imported
/// again. Without a config folder there is no store, and the result is
/// `Ok(false)`.
///
/// # Errors
///
/// Returns [`StoreWriteError`] when the marker or the removal fails.
pub fn reset(root: &Path) -> Result<bool, StoreWriteError> {
    let Some(path) = store_path(root) else {
        return Ok(false);
    };
    remove_store(&path)
}

/// Delete every project store file, and mark the project at `root` as
/// imported. The lock files, the import markers and the global default
/// toggle stay. Returns whether a file was removed.
///
/// # Errors
///
/// Returns [`StoreWriteError`] when a marker or a removal fails.
pub fn reset_all(root: &Path) -> Result<bool, StoreWriteError> {
    let mut removed = reset(root)?;
    let Some(dir) = store_dir() else {
        return Ok(removed);
    };
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Ok(removed);
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) == Some("json") {
            removed |= remove_store(&path)?;
        }
    }
    Ok(removed)
}

/// Map the result of a record transaction. Recording is ancillary to the
/// analysis: a missing config folder or a failed write does not warn.
pub(super) fn finish_record(
    root: &Path,
    outcome: Result<Result<bool, String>, StoreWriteError>,
) -> Result<(), String> {
    match outcome {
        Ok(Ok(recorded)) => {
            if recorded && let Some(max_age) = resolve_store_max_age() {
                sweep_old_stores(&project_identity(root).0, max_age);
            }
            Ok(())
        }
        Ok(Err(message)) => Err(message),
        Err(StoreWriteError::NoConfigDir) => Ok(()),
        Err(StoreWriteError::Io { path, source }) => {
            tracing::debug!(path = %path.display(), error = %source, "could not write the impact store");
            Ok(())
        }
        Err(err @ StoreWriteError::Read(_)) => {
            let message = err.message();
            tracing::warn!("{message}");
            Err(message)
        }
    }
}
