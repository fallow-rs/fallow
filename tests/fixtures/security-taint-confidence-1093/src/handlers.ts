import { execSync } from "node:child_process";

// Arg-level: the sink argument traces to the same-module request read below, so
// the trace source node anchors at that read line, not the file import line.
export const direct = (req: { query: { id: string } }): void => {
  const a = req.query.id;
  execSync(`run ${a}`);
};

// Module-level: `cmd` is a plain parameter that does NOT trace to any source,
// but this module DOES contain an untrusted source (req.query.id above), so the
// sink is module-level reachable, not arg-level. The source node is labeled
// source-module, never an arg-level untrusted-source read.
export const unrelated = (cmd: string): void => {
  execSync(`run ${cmd}`);
};
