/**
 * Render model for the MODEL-INFERRED trade-off surface, the non-deterministic
 * companion to the deterministic {@link import("./walkthrough").Decision} surface.
 *
 * Where decisions are proved from the module graph (a `signal_id` fallow can
 * re-validate), trade-offs are a model reading the diff through the lenses fallow
 * cannot see (abstraction, error handling, data-model shape, ...). They are NEVER
 * graph facts: `deterministic` is pinned to the literal `false` so a consumer can
 * never mistake one for a decision. See the `fallow-review` skill's
 * `tradeoff-elicitation.md` for the producing prompt and its honesty contract.
 */

/** Trade-off magnitude/sureness band. Both `consequence` and `confidence` use it. */
export type Severity = "low" | "medium" | "high";

/**
 * One model-inferred trade-off, anchored to a changed `file:line` (or the literal
 * `"cross-cutting"`). `observed` is a neutral fact, `tradeoff` is the model's
 * inference (gain and cost), `question` is the open call the human owns. Pinned
 * `deterministic: false`: this is never a graph fact.
 */
export type TradeOff = {
  /** Stable per item (`to:<anchor>:<lens>`); dedupes re-runs, keeps dismissals sticky. */
  id: string;
  /** A real changed `file:line`, or the literal `"cross-cutting"` (anchorless slot). */
  anchor: string;
  /** The lens the trade-off was read through (e.g. `error-handling`, `data-model`). */
  lens: string;
  /** FACT: what the change does, read straight from the diff. Neutral, no verdict. */
  observed: string;
  /** INFERENCE: what it gains and what it costs. The model's reading, not ground truth. */
  tradeoff: string;
  /** DECISION: the genuinely-open question the human owns. Names no fix. */
  question: string;
  /** Impact if the call is wrong. What the surface ranks and caps on. */
  consequence: Severity;
  /** How strongly the diff itself supports the reading. Orthogonal to `consequence`. */
  confidence: Severity;
  /** Provenance hint (author-agent at write-time), NOT a trust score. Never authority. */
  captured: boolean;
  /** Always the literal `false`: a trade-off is never a deterministic graph fact. */
  deterministic: false;
};

/**
 * The single envelope the trade-off prompt emits, normalized to the renderer's
 * camelCase. `abstained: true` is the terminal "looked, found nothing
 * consequential" state and forces `tradeoffs: []`; it is distinct from a parse
 * failure (which yields a non-abstained empty envelope, never a fake abstain).
 */
export type TradeOffEnvelope = {
  graphSnapshotHash: string;
  abstained: boolean;
  tradeoffs: TradeOff[];
};
