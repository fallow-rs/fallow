/**
 * The released Fallow skills: every directory under `npm/fallow/skills/` that
 * has a `SKILL.md`. The npm package ships each of them, the CLI embeds them
 * (`crates/cli/build.rs`), the maintainer tree mirrors them, and the companion
 * `fallow-skills` repository vendors them.
 */

import { existsSync, readdirSync } from "node:fs";
import { join } from "node:path";

export const RELEASED_SKILLS_ROOT = ["npm", "fallow", "skills"];

/**
 * Names of the released skills under `repoRoot`, sorted. Directories that
 * start with `_` or `.` hold build artifacts and never count as a skill.
 *
 * @param {string} repoRoot
 * @returns {string[]}
 */
export const releasedSkillNames = (repoRoot) => {
  const root = join(repoRoot, ...RELEASED_SKILLS_ROOT);
  if (!existsSync(root)) {
    return [];
  }
  return readdirSync(root, { withFileTypes: true })
    .filter((entry) => entry.isDirectory() && !/^[_.]/u.test(entry.name))
    .filter((entry) => existsSync(join(root, entry.name, "SKILL.md")))
    .map((entry) => entry.name)
    .toSorted();
};
