import { execSync } from "node:child_process";

// Excluded from the no-child-process rule via its `exclude` glob.
execSync("echo dev");

export const devOnly = 2;
