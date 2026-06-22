import type {
  AcceptedJudgment,
  FramingOrigin,
  InlineFraming,
  ValidationEnvelope,
} from "../../../model/agent";

/**
 * Pure helpers that turn graph-validated framing (from the agent-run envelope or
 * the captured-framing reader) into the `signalId`-keyed shape DecisionList reads.
 * Kept dependency-free so the keying/grouping is testable without a DOM harness.
 */

/** A keyed view: all inline framing that anchors to a given decision's signal_id. */
export type FramingBySignal = ReadonlyMap<string, InlineFraming[]>;

/**
 * Tag one accepted judgment with its {@link FramingOrigin}. Origin is decided at
 * the source (the reader), never inferred from content, so a reconstructed item
 * can never be silently relabelled as captured.
 */
export const toInlineFraming = (
  judgment: AcceptedJudgment,
  origin: FramingOrigin,
): InlineFraming => {
  const framing: InlineFraming = {
    signalId: judgment.signal_id,
    origin,
    framing: judgment.agent_framing,
    // Framing is advisory, never a deterministic graph fact, regardless of the
    // backend's self-reported flag; we pin it false to keep the fence honest.
    deterministic: false,
  };
  if (judgment.concern !== undefined) framing.concern = judgment.concern;
  return framing;
};

/**
 * Extract the accepted judgments from a fallow validation envelope as
 * reconstructed inline framing. Rejected (unanchored/stale) judgments are dropped
 * here: only graph-accepted framing is allowed inline next to a decision.
 */
export const acceptedReconstructedFraming = (
  envelope: ValidationEnvelope | null | undefined,
): InlineFraming[] => (envelope?.accepted ?? []).map((j) => toInlineFraming(j, "reconstructed"));

/**
 * Group inline framing by `signalId` for per-decision rendering. Insertion order
 * within a signal is preserved so a stable list renders deterministically. The
 * caller is responsible for origin tagging upstream (this is content-agnostic).
 */
export const groupBySignalId = (items: ReadonlyArray<InlineFraming>): FramingBySignal => {
  const map = new Map<string, InlineFraming[]>();
  for (const item of items) {
    const bucket = map.get(item.signalId);
    if (bucket) {
      bucket.push(item);
    } else {
      map.set(item.signalId, [item]);
    }
  }
  return map;
};
