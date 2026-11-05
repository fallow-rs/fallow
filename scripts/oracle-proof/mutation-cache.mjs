// The result cache oracle of crates/api/src/mutation/cache_model, ported
// onto commits from before it existed.

import { cpSync, existsSync, readFileSync, symlinkSync, writeFileSync } from "node:fs";
import { join } from "node:path";

const TESTS = "crates/api/src/mutation/cache_model";
const MUTATION_MOD = "crates/api/src/mutation/mod.rs";
const RUNNER_MODULES = "tools/mutation-runner/node_modules";
const E2E_ANCHOR = "mod e2e_tests;\n";
const MODULE_LINE = "#[cfg(all(test, unix))]\nmod cache_model;\n";

/**
 * Install the current result cache model tests in `tree`. The tests run
 * the Vitest package of `tools/mutation-runner`, so the tree links the
 * installed packages of the source checkout. Returns a note per change.
 */
const port = (tree, source) => {
  const notes = [`copied ${TESTS} from the source checkout`];
  cpSync(join(source, TESTS), join(tree, TESTS), { recursive: true });
  const modPath = join(tree, MUTATION_MOD);
  const text = readFileSync(modPath, "utf8");
  if (!/^mod cache_model;$/m.test(text)) {
    if (!text.includes(E2E_ANCHOR)) {
      throw new Error(`${MUTATION_MOD}: the anchor ${JSON.stringify(E2E_ANCHOR)} is missing`);
    }
    writeFileSync(modPath, text.replace(E2E_ANCHOR, `${E2E_ANCHOR}${MODULE_LINE}`));
  }
  if (!existsSync(join(tree, RUNNER_MODULES))) {
    const installed = join(source, RUNNER_MODULES);
    if (!existsSync(installed)) {
      throw new Error(`run \`npm ci --prefix tools/mutation-runner\` in ${source} first`);
    }
    symlinkSync(installed, join(tree, RUNNER_MODULES), "dir");
    notes.push(`linked ${RUNNER_MODULES} from the source checkout`);
  }
  return notes;
};

const CARGO_TEST = ["test", "-p", "fallow-api", "--lib", "mutation::cache_model"];
const REGRESSION_TEST = [
  ...CARGO_TEST.slice(0, -1),
  "mutation::cache_model::sequences::regression_sequences_match_cache_free_runs",
  "--",
  "--exact",
];
/** A missing runner fails the run, so a skip never passes as a proof. */
const REQUIRED = { FALLOW_MUTATION_E2E_REQUIRED: "1" };

/**
 * The result cache oracle: the result cache key covers each project file
 * that the test runner reads, and a warm mutation run equals a `--no-cache`
 * run after each step of an edit sequence.
 */
export const mutationCache = {
  name: "mutation-cache",
  description: "read-set oracle and warm equals cold for the mutation result cache",
  fixes: [
    {
      commit: "922168dc2",
      scope: "the #3473 regression sequences",
      reason:
        "the commit does not have the later tsconfig extends key, and does not key a Bun preload file when no package enables the Bun plugin, so the other tests fail on it",
      test: ["cargo", REGRESSION_TEST],
      env: { ...REQUIRED, FALLOW_MUTATION_CACHE_MODEL_REGRESSIONS: "(#3473)" },
    },
  ],
  port,
  backports: [],
  build: ["cargo", [...CARGO_TEST, "--no-run"]],
  test: ["cargo", CARGO_TEST],
};
