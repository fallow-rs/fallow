import { execSync } from "node:child_process";

// The issue #1146 headline: `a` is the direct source read (hop 1) and `b`
// chains once (hop 2), so the sink argument traces back to the original read
// and the candidate is arg-level with the trace anchored at that read line.
export const twoHop = (req: { query: { id: string } }): void => {
  const a = req.query.id;
  const b = `wrap-${a}`;
  execSync(`run ${b}`);
};
