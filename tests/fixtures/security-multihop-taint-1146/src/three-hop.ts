import { execSync } from "node:child_process";

// A 3-binding chain sits exactly at the hop cap (a = 1, b = 2, c = 3) and is
// still arg-level.
export const threeHop = (req: { query: { id: string } }): void => {
  const a = req.query.id;
  const b = `one-${a}`;
  const c = `two-${b}`;
  execSync(`run ${c}`);
};
