// This module has no Node import, because the runtime of the Vitest setup
// file loads it also in browser mode.

/** The most distinct inputs that a record pass keeps for one function. */
export const RECORD_LIMIT = 8;

/**
 * The most calls of one function that a record pass encodes. A function that
 * a test calls in a hot loop then costs at most this many encodings.
 */
export const RECORD_CALL_LIMIT = 64;

/**
 * The records of a map from a function key to `{ inputs, refused }` as the
 * `recorded_args` entries of the protocol: the key, the inputs and the
 * reasons with their counts, in first record order.
 */
export const recordEntries = (records) =>
  [...records].map(([key, entry]) => ({
    key,
    inputs: [...entry.inputs.values()],
    not_replayable: [...entry.refused].map(([reason, count]) => ({ reason, count })),
  }));
