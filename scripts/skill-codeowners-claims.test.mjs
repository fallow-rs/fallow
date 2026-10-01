import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

const REPOSITORY_ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const RELEASED_SKILL = "npm/fallow/skills/fallow/SKILL.md";

// The CODEOWNERS resolver in crates/engine/src/codeowners.rs matches each file
// against one compiled glob set. It keeps no per-directory cache, so the
// released skill must not tell agents that it does.
test("released skill does not claim a cache for the CODEOWNERS resolver", () => {
  const text = readFileSync(join(REPOSITORY_ROOT, RELEASED_SKILL), "utf8");
  const ownerLines = text.split("\n").filter((line) => line.includes("--group-by owner"));
  assert.ok(ownerLines.length > 0, "the skill documents --group-by owner");
  const cacheClaims = ownerLines.filter((line) => /\bcache/iu.test(line));
  assert.deepEqual(cacheClaims, []);
});
