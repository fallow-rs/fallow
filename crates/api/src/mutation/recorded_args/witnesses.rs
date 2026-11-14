//! The results of the witness replays of a run, kept next to the recorded
//! inputs (`<cache dir>/mutation/witnesses.json`) with the same time to live
//! and file mode. The report holds only the reference: the verdict and the
//! input id. `fallow mutation tasks` joins the values from here.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::{TIME_TO_LIVE_SECS, write_private};

const WITNESS_FORMAT: u32 = 1;

/// The result of one side of a witness replay.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredSide {
    pub outcome: String,
    pub value: serde_json::Value,
    pub text: String,
}

/// The values of one witness.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredWitness {
    pub recorded_at: u64,
    pub input_id: String,
    pub args: Vec<serde_json::Value>,
    pub original: StoredSide,
    pub mutant: StoredSide,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct WitnessStore {
    format: u32,
    /// Keyed by mutant id.
    witnesses: BTreeMap<String, StoredWitness>,
}

/// The path of the witness store below the cache directory.
pub fn witness_path(cache_dir: &Path) -> PathBuf {
    cache_dir.join("mutation").join("witnesses.json")
}

const fn is_fresh(witness: &StoredWitness, now: u64) -> bool {
    witness.recorded_at <= now && now - witness.recorded_at <= TIME_TO_LIVE_SECS
}

fn read(path: &Path) -> WitnessStore {
    std::fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<WitnessStore>(&bytes).ok())
        .filter(|store| store.format == WITNESS_FORMAT)
        .unwrap_or_default()
}

/// Replaces the witnesses of the store with the witnesses of a run. A run
/// that searched for witnesses describes the current code, so the earlier
/// witnesses go.
///
/// # Errors
///
/// Returns the IO error of the write.
pub fn store_witnesses(
    cache_dir: &Path,
    witnesses: BTreeMap<String, StoredWitness>,
) -> std::io::Result<()> {
    crate::mutation::last_report::ignore_cache_dir(cache_dir)?;
    let path = witness_path(cache_dir);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let store = WitnessStore {
        format: WITNESS_FORMAT,
        witnesses,
    };
    let bytes = serde_json::to_vec_pretty(&store).map_err(std::io::Error::other)?;
    write_private(&path, &bytes)
}

/// The witnesses of a store, keyed by mutant id.
pub type WitnessMap = BTreeMap<String, StoredWitness>;

/// The witnesses of the store of `cache_dir`.
pub fn read_witnesses(cache_dir: &Path) -> WitnessMap {
    read(&witness_path(cache_dir)).witnesses
}

/// The fresh witness of `mutant_id` with the input `input_id`, or `None`.
pub fn fresh_witness<'a>(
    witnesses: &'a WitnessMap,
    mutant_id: &str,
    input_id: &str,
    now: u64,
) -> Option<&'a StoredWitness> {
    witnesses
        .get(mutant_id)
        .filter(|witness| witness.input_id == input_id && is_fresh(witness, now))
}

/// Removes the expired witnesses of the store.
pub fn prune_witnesses(cache_dir: &Path, now: u64) {
    let path = witness_path(cache_dir);
    if !path.is_file() {
        return;
    }
    let mut store = read(&path);
    let before = store.witnesses.len();
    store.witnesses.retain(|_, witness| is_fresh(witness, now));
    if store.witnesses.len() == before {
        return;
    }
    if store.witnesses.is_empty() {
        let _ = std::fs::remove_file(&path);
    } else if let Ok(bytes) = serde_json::to_vec_pretty(&store) {
        let _ = write_private(&path, &bytes);
    }
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "tests expect to keep the setup short")]
mod tests {
    use super::*;
    use serde_json::json;

    fn side(value: serde_json::Value) -> StoredSide {
        StoredSide {
            outcome: "returned".to_owned(),
            text: value.to_string(),
            value,
        }
    }

    fn witness(recorded_at: u64) -> StoredWitness {
        StoredWitness {
            recorded_at,
            input_id: "abc".to_owned(),
            args: vec![json!(1)],
            original: side(json!(2)),
            mutant: side(json!(0)),
        }
    }

    #[test]
    fn a_witness_reads_back_by_mutant_and_input_and_expires() {
        let dir = tempfile::tempdir().expect("dir");
        store_witnesses(
            dir.path(),
            BTreeMap::from([("m1".to_owned(), witness(100))]),
        )
        .expect("store");
        let witnesses = read_witnesses(dir.path());
        assert!(fresh_witness(&witnesses, "m1", "abc", 100).is_some());
        assert!(fresh_witness(&witnesses, "m1", "other", 100).is_none());
        assert!(fresh_witness(&witnesses, "m1", "abc", 100 + TIME_TO_LIVE_SECS + 1).is_none());
        prune_witnesses(dir.path(), 100 + TIME_TO_LIVE_SECS + 1);
        assert!(!witness_path(dir.path()).exists());
    }
}
