import { execSync } from "node:child_process";

// No HTTP request anywhere. This sink must NOT inherit
// reachable_from_untrusted_source from the db.query collision in ./repo.
export const run = (r: unknown): void => {
  execSync(`echo ${String(r)}`);
};
