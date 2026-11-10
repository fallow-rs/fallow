//! The witness search of a `fallow mutation --record-args values` run: the
//! recorded inputs of each open task (a survived or uncovered mutant) are
//! replayed on the original and on the mutant, in one companion of the run.
//!
//! The report gets the reference of each replay (the verdict and the input
//! id). The values stay in the cache (`witnesses.json`), and `fallow mutation
//! tasks` joins them. A replay never gives the verdict `equivalent`.

use std::collections::BTreeMap;
use std::path::Path;
use std::time::{Duration, Instant};

use fallow_engine::mutation::{MutationPlan, SourceKind, export_containing};
use fallow_output::{
    MutantResult, MutantStatus, MutantWitness, MutationReplayOutput, MutationReplaySide,
    MutationReplayVerdict,
};
use rustc_hash::FxHashMap;

use super::mutation_replay::{ReplayTarget, recorded_inputs, replay_inputs};
use crate::mutation::protocol::{InitRequest, Request, Response};
use crate::mutation::recorded_args::now_secs;
use crate::mutation::recorded_args::witnesses::{StoredSide, StoredWitness, store_witnesses};
use crate::mutation::transport::{CompanionLink, CompanionSpawner, ProcessSpawner};

/// The most open tasks that one run replays, in rank order.
const MAX_WITNESS_MUTANTS: usize = 20;
/// The time limit of one replay call of the search.
const WITNESS_CALL_TIMEOUT: Duration = Duration::from_secs(5);

/// The companion facts of the run that the search reuses.
pub(super) struct WitnessSearch {
    pub spawner: ProcessSpawner,
    pub init: InitRequest,
    pub runner_timeout: Duration,
    pub deadline: Option<Instant>,
}

/// A companion of the search, started on first use.
struct SearchLink<'a> {
    search: &'a WitnessSearch,
    link: Option<Box<dyn CompanionLink>>,
    /// False when the companion cannot start or cannot replay. The search
    /// then ends.
    usable: bool,
}

impl SearchLink<'_> {
    fn get(&mut self) -> Option<&mut Box<dyn CompanionLink>> {
        if self.link.is_none() && self.usable {
            self.link = self.start();
            self.usable = self.link.is_some();
        }
        self.link.as_mut()
    }

    fn start(&self) -> Option<Box<dyn CompanionLink>> {
        let mut link = self.search.spawner.spawn().ok()?;
        match link.request(
            &Request::Init(self.search.init.clone()),
            self.search.runner_timeout,
        ) {
            Ok(Response::Ready(ready)) if ready.capabilities.replay => Some(link),
            _ => {
                link.shutdown();
                None
            }
        }
    }

    /// Drops the companion after a fault, so the next replay starts a new
    /// one.
    fn reset(&mut self) {
        if let Some(mut link) = self.link.take() {
            link.shutdown();
        }
    }
}

const fn is_open(status: MutantStatus) -> bool {
    matches!(status, MutantStatus::Survived | MutantStatus::NoCoverage)
}

fn first_side(output: &MutationReplayOutput, side: MutationReplaySide) -> Option<StoredSide> {
    output
        .calls
        .iter()
        .find(|call| call.side == side)
        .map(|call| StoredSide {
            outcome: serde_json::to_value(call.outcome)
                .ok()
                .and_then(|value| value.as_str().map(str::to_owned))
                .unwrap_or_default(),
            value: call.value.clone(),
            text: call.text.clone(),
        })
}

/// The stored values of a witness: its input and the first call of each side.
fn stored_witness(output: &MutationReplayOutput, now: u64) -> Option<StoredWitness> {
    if output.verdict != MutationReplayVerdict::Witness {
        return None;
    }
    Some(StoredWitness {
        recorded_at: now,
        input_id: output.input.input_id.clone()?,
        args: output.input.args.clone(),
        original: first_side(output, MutationReplaySide::Original)?,
        mutant: first_side(output, MutationReplaySide::Mutant)?,
    })
}

/// The export that a replay of `mutant_id` calls, from the current source.
fn export_of(
    plan: &MutationPlan,
    root: &Path,
    sources: &mut FxHashMap<String, Option<String>>,
    mutant_id: &str,
) -> Option<(String, String)> {
    let planned = plan
        .mutants
        .iter()
        .find(|mutant| mutant.mutant.id.as_str() == mutant_id)?;
    let file = planned.file.clone();
    let kind = SourceKind::from_path(&file)?;
    let function = planned.mutant.enclosing_function.as_ref()?;
    let source = sources
        .entry(file.clone())
        .or_insert_with(|| {
            let text = std::fs::read_to_string(root.join(&file)).ok()?;
            Some(
                text.strip_prefix('\u{feff}')
                    .map_or_else(|| text.clone(), str::to_owned),
            )
        })
        .as_deref()?;
    let export = export_containing(source, kind, function.span)?;
    Some((file, export))
}

/// Replays the recorded inputs of the open tasks of `results`, sets the
/// `witness` of each task that has recorded inputs, and stores the values
/// of each witness in the cache.
pub(super) fn search_witnesses(
    search: &WitnessSearch,
    plan: &MutationPlan,
    root: &Path,
    cache_dir: &Path,
    results: &mut [MutantResult],
) {
    let now = now_secs();
    let mut order: Vec<usize> = (0..results.len())
        .filter(|&index| is_open(results[index].status))
        .collect();
    order.sort_by_key(|&index| results[index].location.rank.unwrap_or(u32::MAX));
    let mut sources = FxHashMap::default();
    let mut link = SearchLink {
        search,
        link: None,
        usable: true,
    };
    let mut stored = BTreeMap::new();
    let mut searched = 0;
    for index in order {
        if searched >= MAX_WITNESS_MUTANTS
            || search
                .deadline
                .is_some_and(|deadline| Instant::now() >= deadline)
        {
            break;
        }
        let mutant_id = results[index].location.mutant_id.clone();
        let Some((file, export)) = export_of(plan, root, &mut sources, &mutant_id) else {
            continue;
        };
        let Ok(inputs) = recorded_inputs(cache_dir, &file, &export) else {
            continue;
        };
        let Some(companion) = link.get() else {
            break;
        };
        searched += 1;
        let target = ReplayTarget {
            file: &file,
            mutant_id: &mutant_id,
            export: &export,
        };
        let Ok(output) = replay_inputs(companion, &target, &inputs, WITNESS_CALL_TIMEOUT) else {
            link.reset();
            continue;
        };
        if output.verdict == MutationReplayVerdict::Timeout {
            link.reset();
        }
        if let Some(witness) = stored_witness(&output, now) {
            stored.insert(mutant_id.clone(), witness);
        }
        results[index].witness = Some(MutantWitness {
            verdict: output.verdict,
            input_id: output.input.input_id.clone(),
            inputs_checked: output.inputs_checked,
            reason: output.reason.clone(),
        });
    }
    link.reset();
    // The witnesses of an earlier run describe older code, so the store
    // keeps only the witnesses of this run.
    let _ = store_witnesses(cache_dir, stored);
}
