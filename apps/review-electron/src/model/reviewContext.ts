/**
 * Render model for the AUTHOR-AGENT review brief: the CAPTURE artifact the agent
 * that DID the work records at write-time. It is the honest, primary orientation
 * surface , "what the change is + the specific things that need the human's taste"
 * , and is distinct from the live "Agent Review" reconstruct panel (which re-derives
 * framing after the fact). Nothing here is graph-validated: it is anchored prose the
 * author recorded, presented as their context, not as a re-checkable fact.
 */

/** One author-recorded "what to review here, and why", optionally anchored to a line. */
export type ReviewContextItem = {
  /** A `file:line` (clickable, opens the diff); may be empty for a general note. */
  anchor: string;
  /** The author agent's note: what to review here, and why it needs the human. */
  note: string;
};

/**
 * The single envelope the author agent persists at write-time. `summary` is the
 * overview (what the change does + where taste is needed), `author` is provenance
 * (which agent produced it), `items` are the specific anchored asks.
 */
export type ReviewContext = {
  /** The author agent's overview: what the change does + where taste is needed. */
  summary: string;
  /** Which agent produced this brief, e.g. `"claude-code"` (attribution, not authority). */
  author: string;
  /** The specific things that want the human's taste, each optionally anchored. */
  items: ReviewContextItem[];
};
