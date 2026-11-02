// Negative (literal): merging a fully-literal object source is never captured
// (the source argument is literal), so it must NOT produce a candidate.
import merge from "lodash.merge";

export function applyDefaults(base: Record<string, unknown>): unknown {
  return merge(base, { theme: "dark" });
}
