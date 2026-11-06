import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { after } from "node:test";

// A test file imports this module at the top level, so `after` attaches to
// the root suite of the file and runs once when all tests of the file end.
const created = new Set();
after(() => {
  for (const dir of created) rmSync(dir, { recursive: true, force: true });
  created.clear();
});

/**
 * Creates a new folder in the system temp directory, and removes it when the
 * tests of the calling file end. `prefix` starts the folder name.
 */
export const makeTempDir = (prefix) => {
  const dir = mkdtempSync(join(tmpdir(), prefix));
  created.add(dir);
  return dir;
};
