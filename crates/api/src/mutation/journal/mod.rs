//! The outcome journal of a mutation run.
//!
//! A run appends each final result to `<cache dir>/mutation/journal.jsonl`
//! when the result is final, so an interrupted run keeps its finished
//! results. `fallow mutation --resume` reads the journal and does not run
//! the journaled mutants again. The journal is not a report.
//!
//! The first line is a header with the fingerprint of the run. Each next
//! line is one final result with the key of the result cache. The run writes
//! each line with one write call and flushes it to disk. A read ignores a
//! line that does not parse, for example a last line that an interrupt cut.
//!
//! Only a `killed` result and a `survived` result that a retry confirmed are
//! final. A run without `--resume` starts a new journal. A run with
//! `--resume` keeps the valid lines of a matching journal and appends to
//! them. The journal does not depend on `--no-cache`.

use std::fs::{File, OpenOptions};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use fallow_output::MutantStatus;
use rustc_hash::{FxHashMap, FxHashSet};
use serde::{Deserialize, Serialize};

use super::protocol::Ready;
use super::result_cache::ResultCache;
use super::scheduler::{CachedResult, OutcomeJournal, ResultLookup, ScheduledMutant};

/// The file name of the journal below `<cache dir>/mutation`.
pub const JOURNAL_FILE: &str = "journal.jsonl";
/// The version of the line format. A change to the header or the line shape
/// needs a new version.
pub const JOURNAL_FORMAT: u32 = 1;
/// The length of a run directory hash in a mismatch message.
const SHOWN_HASH_LEN: usize = 12;
/// The largest journal that `--resume` reads. A line is about 200 bytes, so
/// this is far above the 20000 entries of the result cache.
const MAX_JOURNAL_BYTES: u64 = 64 * 1024 * 1024;
/// The file name of a journal that the run did not read.
const UNREAD_FILE: &str = "journal.jsonl.unread";
/// A temporary journal file older than this belongs to a run that ended
/// before its rename. A newer one can belong to a live run.
const STALE_TEMPORARY: std::time::Duration = std::time::Duration::from_hours(1);

/// The fingerprint of a run. A journal resumes only a run with an equal
/// fingerprint. The run limits (`--max-mutants`, `--time-budget`,
/// `--concurrency`) are not in it, so a resumed run can use other limits.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JournalHeader {
    pub journal_format: u32,
    pub fallow_version: String,
    pub runner: String,
    /// The runtime name and version, for example `node 24.1.0`.
    pub runtime: String,
    /// The content hash of the run directory: each planned path and its
    /// instrumented text.
    pub run_dir: String,
    pub runner_config: Option<String>,
    /// The scope, for example `all` or `changed against main`.
    pub scope: String,
    /// The operator names. `None` uses each operator.
    pub operators: Option<Vec<String>>,
    pub allow_side_effects: bool,
}

impl JournalHeader {
    /// The inputs that differ between this journal header and the header of
    /// the current run, with both values.
    fn differences(&self, current: &Self) -> Vec<String> {
        let operators = |names: &Option<Vec<String>>| {
            names
                .as_ref()
                .map_or_else(|| "each operator".to_owned(), |names| names.join(","))
        };
        let config = |config: &Option<String>| config.clone().unwrap_or_else(|| "none".to_owned());
        let hash = |hash: &str| hash.chars().take(SHOWN_HASH_LEN).collect::<String>();
        let pairs = [
            (
                "journal format",
                self.journal_format.to_string(),
                current.journal_format.to_string(),
            ),
            (
                "Fallow version",
                self.fallow_version.clone(),
                current.fallow_version.clone(),
            ),
            ("runner", self.runner.clone(), current.runner.clone()),
            ("runtime", self.runtime.clone(), current.runtime.clone()),
            (
                "planned source and instrumentation",
                hash(&self.run_dir),
                hash(&current.run_dir),
            ),
            (
                "runner config",
                config(&self.runner_config),
                config(&current.runner_config),
            ),
            ("scope", self.scope.clone(), current.scope.clone()),
            (
                "operators",
                operators(&self.operators),
                operators(&current.operators),
            ),
            (
                "side-effect mutants",
                self.allow_side_effects.to_string(),
                current.allow_side_effects.to_string(),
            ),
        ];
        pairs
            .into_iter()
            .filter(|(_, journal, run)| journal != run)
            .map(|(label, journal, run)| format!("{label} (journal: {journal}, this run: {run})"))
            .collect()
    }
}

/// One final result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Line {
    id: String,
    key: String,
    status: MutantStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    killed_by: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    duration_ms: Option<u64>,
}

/// True for a status that the journal keeps as final.
const fn is_final(status: MutantStatus) -> bool {
    matches!(status, MutantStatus::Killed | MutantStatus::Survived)
}

/// A journal whose header does not match the current run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JournalMismatch {
    /// The inputs that differ, with both values.
    pub differences: Vec<String>,
}

/// What a `--resume` read of the journal found.
enum ReadOutcome {
    /// The final results of a matching journal, one for each mutant id.
    Entries(Vec<Line>),
    /// No usable journal. The text says why.
    Missing(String),
    /// A journal that the run did not read: too large, or an IO error. The
    /// run moves it aside and does not erase it. The text says why.
    Unread(String),
    Mismatch(JournalMismatch),
}

/// Reads the journal at `path`. The size check comes before the read, so a
/// journal over `max_bytes` is never read into memory. Each line is parsed
/// as bytes, so a last line that an interrupt cut inside a multibyte
/// character is one bad line, not a bad file.
fn read_journal(path: &Path, header: &JournalHeader, max_bytes: u64) -> ReadOutcome {
    match std::fs::metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return ReadOutcome::Missing("no journal exists".to_owned());
        }
        Err(error) => {
            return ReadOutcome::Unread(format!("the journal does not read ({error})"));
        }
        Ok(metadata) if metadata.len() > max_bytes => {
            return ReadOutcome::Unread(format!("the journal is larger than {max_bytes} bytes"));
        }
        Ok(_) => {}
    }
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) => {
            return ReadOutcome::Unread(format!("the journal does not read ({error})"));
        }
    };
    let mut lines = bytes.split(|&byte| byte == b'\n');
    let Some(stored) = lines
        .next()
        .and_then(|line| serde_json::from_slice::<JournalHeader>(line).ok())
    else {
        return ReadOutcome::Missing("the journal has no valid header".to_owned());
    };
    let differences = stored.differences(header);
    if !differences.is_empty() {
        return ReadOutcome::Mismatch(JournalMismatch { differences });
    }
    // The last line for an id wins: a mutant that ran again after a key
    // change appends a newer line.
    let mut entries: FxHashMap<String, Line> = FxHashMap::default();
    for line in lines.filter_map(|line| serde_json::from_slice::<Line>(line).ok()) {
        if is_final(line.status) {
            entries.insert(line.id.clone(), line);
        }
    }
    let mut entries: Vec<Line> = entries.into_values().collect();
    entries.sort_by(|left, right| left.id.cmp(&right.id));
    ReadOutcome::Entries(entries)
}

/// Removes each temporary journal file in `dir` that is older than
/// [`STALE_TEMPORARY`]. A run that ended between its write and its rename
/// leaves one. A newer file can belong to a concurrent live run, so it
/// stays.
fn remove_stale_temporaries(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let prefix = format!("{JOURNAL_FILE}.");
    for entry in entries.filter_map(Result::ok) {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if !(name.starts_with(&prefix)
            && Path::new(name.as_ref())
                .extension()
                .is_some_and(|extension| extension == "tmp"))
        {
            continue;
        }
        let stale = entry
            .metadata()
            .and_then(|metadata| metadata.modified())
            .ok()
            .and_then(|modified| modified.elapsed().ok())
            .is_some_and(|age| age > STALE_TEMPORARY);
        if stale {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

/// Writes the header and `entries` to a temporary file, renames it to
/// `path`, and opens `path` to append. The rename drops a cut last line, so
/// each appended line starts on a new line.
fn write_start(path: &Path, header: &JournalHeader, entries: &[Line]) -> Result<File, String> {
    let fail = |error: &dyn std::fmt::Display| format!("cannot write {}: {error}", path.display());
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| fail(&error))?;
        remove_stale_temporaries(parent);
    }
    let mut text = serde_json::to_string(header).map_err(|error| fail(&error))?;
    text.push('\n');
    for entry in entries {
        text.push_str(&serde_json::to_string(entry).map_err(|error| fail(&error))?);
        text.push('\n');
    }
    let temporary = path.with_extension(format!("jsonl.{}.tmp", std::process::id()));
    let written = File::create(&temporary)
        .and_then(|mut file| {
            file.write_all(text.as_bytes())?;
            file.sync_data()
        })
        .and_then(|()| std::fs::rename(&temporary, path));
    if let Err(error) = written {
        let _ = std::fs::remove_file(&temporary);
        return Err(fail(&error));
    }
    OpenOptions::new()
        .append(true)
        .open(path)
        .map_err(|error| fail(&error))
}

/// The outcome journal of one run.
#[derive(Debug)]
pub struct Journal {
    /// The key source: the result cache of the run, or a cache that only
    /// computes keys with `--no-cache`.
    keys: Arc<ResultCache>,
    /// The open journal file. `None` after a failed write.
    file: Mutex<Option<File>>,
    /// The final results of the interrupted run. Empty without `--resume`.
    resume: FxHashMap<String, Line>,
    /// The mutant ids whose journal key does not match this run.
    rejected: Mutex<FxHashSet<String>>,
    /// The first write error.
    write_error: Mutex<Option<String>>,
}

/// The journal of a run, and why a `--resume` run started from the
/// beginning.
pub struct Opened {
    pub journal: Journal,
    /// Set when `--resume` found no usable journal.
    pub note: Option<String>,
}

impl Journal {
    /// The path of the journal below `mutation_cache`.
    #[must_use]
    pub fn path(mutation_cache: &Path) -> PathBuf {
        mutation_cache.join(JOURNAL_FILE)
    }

    /// Starts the journal of a run with `header`. With `resume`, it first
    /// reads the journal of an earlier run: a matching journal gives its
    /// final results, and a missing journal gives a note. A failed write
    /// gives a journal that writes nothing and reports the error.
    ///
    /// # Errors
    ///
    /// Returns the differing inputs when `resume` is set and the journal
    /// header does not match `header`.
    pub fn open(
        mutation_cache: &Path,
        header: &JournalHeader,
        keys: Arc<ResultCache>,
        resume: bool,
    ) -> Result<Opened, JournalMismatch> {
        Self::open_capped(mutation_cache, header, keys, resume, MAX_JOURNAL_BYTES)
    }

    /// [`Journal::open`] with the largest journal that `--resume` reads.
    fn open_capped(
        mutation_cache: &Path,
        header: &JournalHeader,
        keys: Arc<ResultCache>,
        resume: bool,
        max_bytes: u64,
    ) -> Result<Opened, JournalMismatch> {
        let path = Self::path(mutation_cache);
        let fresh = |reason: &str| format!("the run started from the beginning, because {reason}");
        let mut start_error = None;
        let (entries, note) = if resume {
            match read_journal(&path, header, max_bytes) {
                ReadOutcome::Entries(entries) => (entries, None),
                ReadOutcome::Missing(reason) => (Vec::new(), Some(fresh(&reason))),
                ReadOutcome::Unread(reason) => {
                    // Never erase a journal that the run did not read.
                    let aside = mutation_cache.join(UNREAD_FILE);
                    if let Err(error) = std::fs::rename(&path, &aside) {
                        start_error = Some(format!(
                            "cannot move the unread journal {} aside: {error}",
                            path.display()
                        ));
                    }
                    let note = format!("{reason}; the run moved it to {UNREAD_FILE}");
                    (Vec::new(), Some(fresh(&note)))
                }
                ReadOutcome::Mismatch(mismatch) => return Err(mismatch),
            }
        } else {
            (Vec::new(), None)
        };
        // A journal that could not move aside stays as it is, and this run
        // writes no journal.
        let started = match start_error {
            Some(error) => Err(error),
            None => write_start(&path, header, &entries),
        };
        let (file, write_error) = match started {
            Ok(file) => (Some(file), None),
            Err(error) => (None, Some(error)),
        };
        let journal = Self {
            keys,
            file: Mutex::new(file),
            resume: entries
                .into_iter()
                .map(|line| (line.id.clone(), line))
                .collect(),
            rejected: Mutex::new(FxHashSet::default()),
            write_error: Mutex::new(write_error),
        };
        Ok(Opened { journal, note })
    }

    /// The journal results whose key does not match this run.
    #[must_use]
    pub fn rejected(&self) -> usize {
        self.rejected.lock().map_or(0, |rejected| rejected.len())
    }

    /// The first write error of the run.
    #[must_use]
    pub fn write_error(&self) -> Option<String> {
        self.write_error.lock().ok().and_then(|error| error.clone())
    }

    fn note_write_error(&self, error: String) {
        if let Ok(mut first) = self.write_error.lock() {
            first.get_or_insert(error);
        }
    }
}

impl OutcomeJournal for Journal {
    fn key(
        &self,
        ready: &Ready,
        mutant: &ScheduledMutant,
        tests: &[String],
        test_files: &[String],
    ) -> String {
        self.keys.key(ready, mutant, tests, test_files)
    }

    fn resumed(&self, mutant_id: &str, key: &str) -> Option<CachedResult> {
        let line = self.resume.get(mutant_id)?;
        if line.key != key {
            if let Ok(mut rejected) = self.rejected.lock() {
                rejected.insert(mutant_id.to_owned());
            }
            return None;
        }
        Some(CachedResult {
            status: line.status,
            killed_by: line.killed_by.clone(),
            duration_ms: line.duration_ms,
        })
    }

    fn append(&self, mutant_id: &str, key: &str, result: &CachedResult) {
        let line = Line {
            id: mutant_id.to_owned(),
            key: key.to_owned(),
            status: result.status,
            killed_by: result.killed_by.clone(),
            duration_ms: result.duration_ms,
        };
        let Ok(mut text) = serde_json::to_string(&line) else {
            return;
        };
        text.push('\n');
        let Ok(mut file) = self.file.lock() else {
            return;
        };
        let Some(handle) = file.as_mut() else {
            return;
        };
        if let Err(error) = handle
            .write_all(text.as_bytes())
            .and_then(|()| handle.sync_data())
        {
            *file = None;
            drop(file);
            self.note_write_error(format!("cannot append to the journal: {error}"));
        }
    }
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "tests unwrap to keep assertions short")]
mod tests;
