import { execSync } from "node:child_process";

// A 4-binding chain exceeds the hop cap: `d` is never recorded as tainted, so
// the sink argument does not trace to the read and the candidate degrades to
// module-level reachability (this module still contains the source read), not
// a false arg-level claim.
export const fourHop = (req: { query: { id: string } }): void => {
  const a = req.query.id;
  const b = `one-${a}`;
  const c = `two-${b}`;
  const d = `three-${c}`;
  execSync(`run ${d}`);
};
