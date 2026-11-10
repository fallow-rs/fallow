//! The verdict of a replay: one export called on the original and on one
//! mutant, a fixed number of times per side, in turns.
//!
//! A difference counts only when each side is stable: each call of a side
//! gives the same outcome and the same value. A replay never decides that a
//! mutant is equivalent. Equal results say only that this input shows no
//! difference.

use fallow_output::{MutationReplayOutcome, MutationReplaySide, MutationReplayVerdict};

use crate::mutation::protocol::{ReplayCall, ReplayOutcome, ReplaySide};

/// The JSON text of `value` with the keys of each object in sorted order. Two
/// results that differ only in the key order of an object are equal, because
/// a deep equality check of a test does not see the order.
fn canonical(value: &serde_json::Value) -> String {
    fn sorted(value: &serde_json::Value) -> serde_json::Value {
        match value {
            serde_json::Value::Object(map) => {
                let mut entries: Vec<_> = map.iter().collect();
                entries.sort_by(|left, right| left.0.cmp(right.0));
                serde_json::Value::Object(
                    entries
                        .into_iter()
                        .map(|(key, item)| (key.clone(), sorted(item)))
                        .collect(),
                )
            }
            serde_json::Value::Array(items) => {
                serde_json::Value::Array(items.iter().map(sorted).collect())
            }
            other => other.clone(),
        }
    }
    sorted(value).to_string()
}

/// The comparison key of one call.
fn key(call: &ReplayCall) -> (ReplayOutcome, String) {
    (call.outcome, canonical(&call.value))
}

/// The single key of each call of `side`, or `None` when the calls differ.
fn stable_key(calls: &[ReplayCall], side: ReplaySide) -> Option<(ReplayOutcome, String)> {
    let mut keys = calls.iter().filter(|call| call.side == side).map(key);
    let first = keys.next()?;
    keys.all(|other| other == first).then_some(first)
}

/// The verdict of the call records of one replay.
pub fn verdict(calls: &[ReplayCall]) -> MutationReplayVerdict {
    if calls
        .iter()
        .any(|call| call.outcome == ReplayOutcome::NotCallable)
    {
        return MutationReplayVerdict::NotCallable;
    }
    if calls
        .iter()
        .any(|call| call.outcome == ReplayOutcome::TimedOut)
    {
        return MutationReplayVerdict::Timeout;
    }
    match (
        stable_key(calls, ReplaySide::Original),
        stable_key(calls, ReplaySide::Mutant),
    ) {
        (Some(original), Some(mutant)) if original != mutant => MutationReplayVerdict::Witness,
        (Some(_), Some(_)) => MutationReplayVerdict::NoWitnessFound,
        _ => MutationReplayVerdict::Unstable,
    }
}

/// Checks that the records hold `calls_per_side` calls of each side, in
/// turns, with indexes from 1. A companion that breaks the contract gives
/// the message of the first fault.
pub fn check_calls(calls: &[ReplayCall], calls_per_side: u32) -> Result<(), String> {
    let expected = (1..=calls_per_side)
        .flat_map(|index| [(ReplaySide::Original, index), (ReplaySide::Mutant, index)]);
    let actual: Vec<_> = calls.iter().map(|call| (call.side, call.index)).collect();
    if actual.iter().copied().eq(expected) {
        Ok(())
    } else {
        Err(format!(
            "the replay result has {} calls out of the contract order; expected {calls_per_side} calls of each side in turns",
            calls.len()
        ))
    }
}

/// The output side of a protocol side.
pub const fn output_side(side: ReplaySide) -> MutationReplaySide {
    match side {
        ReplaySide::Original => MutationReplaySide::Original,
        ReplaySide::Mutant => MutationReplaySide::Mutant,
    }
}

/// The output outcome of a protocol outcome.
pub const fn output_outcome(outcome: ReplayOutcome) -> MutationReplayOutcome {
    match outcome {
        ReplayOutcome::Returned => MutationReplayOutcome::Returned,
        ReplayOutcome::Threw => MutationReplayOutcome::Threw,
        ReplayOutcome::TimedOut => MutationReplayOutcome::TimedOut,
        ReplayOutcome::NotCallable => MutationReplayOutcome::NotCallable,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn call(
        side: ReplaySide,
        index: u32,
        outcome: ReplayOutcome,
        value: serde_json::Value,
    ) -> ReplayCall {
        ReplayCall {
            side,
            index,
            outcome,
            value,
            text: String::new(),
        }
    }

    /// Three turns with the values of each side.
    fn turns(original: [serde_json::Value; 3], mutant: [serde_json::Value; 3]) -> Vec<ReplayCall> {
        original
            .into_iter()
            .zip(mutant)
            .enumerate()
            .flat_map(|(index, (left, right))| {
                let index = u32::try_from(index).unwrap_or(0) + 1;
                [
                    call(ReplaySide::Original, index, ReplayOutcome::Returned, left),
                    call(ReplaySide::Mutant, index, ReplayOutcome::Returned, right),
                ]
            })
            .collect()
    }

    #[test]
    fn stable_sides_that_differ_give_a_witness() {
        let calls = turns(
            [json!(5), json!(5), json!(5)],
            [json!(-1), json!(-1), json!(-1)],
        );
        assert_eq!(verdict(&calls), MutationReplayVerdict::Witness);
    }

    #[test]
    fn equal_sides_give_no_witness_found_and_never_equivalent() {
        let calls = turns(
            [json!(5), json!(5), json!(5)],
            [json!(5), json!(5), json!(5)],
        );
        assert_eq!(verdict(&calls), MutationReplayVerdict::NoWitnessFound);
    }

    #[test]
    fn a_side_that_changes_between_calls_is_unstable_even_when_the_sides_differ() {
        // A value from an unfixed clock: the sides differ on each call, but
        // the original is not stable, so no call pair is a witness.
        let calls = turns(
            [json!(1), json!(2), json!(3)],
            [json!(9), json!(9), json!(9)],
        );
        assert_eq!(verdict(&calls), MutationReplayVerdict::Unstable);
        let mutant = turns(
            [json!(1), json!(1), json!(1)],
            [json!(9), json!(8), json!(9)],
        );
        assert_eq!(verdict(&mutant), MutationReplayVerdict::Unstable);
    }

    #[test]
    fn a_thrown_error_differs_from_a_returned_value_with_the_same_text() {
        let mut calls = turns(
            [json!(1), json!(1), json!(1)],
            [json!(1), json!(1), json!(1)],
        );
        for call in calls
            .iter_mut()
            .filter(|call| call.side == ReplaySide::Mutant)
        {
            call.outcome = ReplayOutcome::Threw;
        }
        assert_eq!(verdict(&calls), MutationReplayVerdict::Witness);
    }

    #[test]
    fn the_key_order_of_an_object_is_no_difference() {
        let calls = turns(
            [
                json!({"a": 1, "b": 2}),
                json!({"a": 1, "b": 2}),
                json!({"a": 1, "b": 2}),
            ],
            [
                json!({"b": 2, "a": 1}),
                json!({"b": 2, "a": 1}),
                json!({"b": 2, "a": 1}),
            ],
        );
        assert_eq!(verdict(&calls), MutationReplayVerdict::NoWitnessFound);
    }

    #[test]
    fn a_timeout_or_a_missing_function_decides_before_the_values() {
        let mut calls = turns(
            [json!(1), json!(1), json!(1)],
            [json!(2), json!(2), json!(2)],
        );
        calls[3].outcome = ReplayOutcome::TimedOut;
        assert_eq!(verdict(&calls), MutationReplayVerdict::Timeout);
        calls[0].outcome = ReplayOutcome::NotCallable;
        assert_eq!(verdict(&calls), MutationReplayVerdict::NotCallable);
    }

    #[test]
    fn the_call_order_must_follow_the_contract() {
        let calls = turns(
            [json!(1), json!(1), json!(1)],
            [json!(2), json!(2), json!(2)],
        );
        assert!(check_calls(&calls, 3).is_ok());
        assert!(check_calls(&calls, 2).is_err());
        let mut swapped = calls.clone();
        swapped.swap(0, 1);
        assert!(check_calls(&swapped, 3).is_err());
        assert!(check_calls(&calls[..5], 3).is_err());
    }
}
