#!/usr/bin/env node

// Rebase one pull request branch of the public documentation repository
// (fallow-rs/docs) during a serial merge. Every content change updates
// public-content-manifest.json, so that file is the expected conflict. The
// script regenerates it for each commit that conflicts on it and stops on
// any other conflict. Run it from a checkout of the documentation repository.

import { existsSync } from "node:fs";
import { join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { runCliMain } from "./cli-main.mjs";
import { checkRebase, git, run, shipBranch, shipMain } from "./ship-git.mjs";

const MANIFEST = "public-content-manifest.json";
const GENERATOR = "scripts/public-content.mjs";

const USAGE = `Usage: node <fallow>/scripts/ship-docs-rebase.mjs (--pr <number> | --branch <name>) [options]

Run from a checkout of fallow-rs/docs. Rebase a pull request branch on the
base branch. Regenerate ${MANIFEST} for each commit that conflicts on it.
Stop on any other conflict.

The rebase, the checks and the push run in a temporary worktree with a
detached HEAD, so your checkout and every local branch stay as they are.
The manifest generator reads the directory. The fresh worktree has no
untracked or ignored files, so a draft page of your checkout does not get
into the manifest. The generator and the tests use only Node built-ins,
so the worktree needs no npm install.

Options:
  --pr <number>     Read the head branch of this pull request with gh.
  --branch <name>   The branch name on the remote.
  --base <ref>      The ref to rebase onto (default: <remote>/main).
  --remote <name>   The remote of the branch (default: origin).
  --push            Push the result with a lease on the old branch tip
                    when all checks pass.
  -h, --help        Show this help.

Git rerere is off for the rebase. The pre-push hook of your checkout runs
on the push. When the checks pass, the script removes the worktree. After
a stop or a failed check, the script keeps the worktree and prints its
path and the next commands.

After the rebase, the script checks that the result
is the same as a merge of the branch into the base outside ${MANIFEST}.
A rebase drops the changes of a merge commit. A path where the merge
itself conflicts fails the check only when the branch has merge commits.
Then compare that path by hand. The script also runs
\`node ${GENERATOR} --check\` and \`npm test\`. The merge comparison needs
Git 2.38 or later.

Exit codes: 0 when the rebase and the checks pass, 1 when the rebase stops
or a check fails, 2 for invalid input or a failed command.`;

const regenerateManifest = (cwd) => (paths) => {
  const others = paths.filter((path) => path !== MANIFEST);
  if (others.length > 0) {
    return { ok: false, reason: `Conflicts outside ${MANIFEST}: ${others.join(", ")}` };
  }
  // The generator writes the whole file from the content files, so it does
  // not read the conflict markers. A failure stops the rebase, so the report
  // says that the rebase is still in progress.
  const result = run("node", [GENERATOR, "--write"], { cwd, allowFailure: true });
  if (result.status !== 0) {
    const output = `${result.stdout}${result.stderr}`.trim();
    return {
      ok: false,
      reason: `node ${GENERATOR} --write failed (exit ${result.status})\n${output}`,
    };
  }
  git(cwd, ["add", "--", MANIFEST]);
  return { ok: true };
};

const CHECKS = [
  { label: `node ${GENERATOR} --check`, command: "node", args: [GENERATOR, "--check"] },
  { label: "npm test", command: "npm", args: ["test"] },
];

/** Run one check in `cwd`. Returns `[]` when it passes, or one problem. */
const runCheck =
  (cwd) =>
  ({ label, command, args }) => {
    const result = run(command, args, { cwd, allowFailure: true });
    if (result.status === 0) {
      return [];
    }
    return [`${label} (exit ${result.status})\n${`${result.stdout}${result.stderr}`.trim()}`];
  };

/**
 * Rebase the documentation branch in a temporary worktree and run the
 * checks of that repository. Print the report to `log` and return the exit
 * code.
 */
const shipDocsRebase = (cwd, options, log = console.log) => {
  if (!existsSync(join(cwd, GENERATOR))) {
    throw new Error(`${cwd} has no ${GENERATOR}. Run this script in a fallow-rs/docs checkout.`);
  }
  // The generator writes the manifest from the files in the directory. The
  // fresh worktree holds only the files of the branch, so no untracked page
  // gets into the manifest of the pushed commits.
  return shipBranch(cwd, log, options, {
    resolveConflicts: regenerateManifest,
    // The generator check covers the manifest, so the tree check leaves it out.
    check: (worktree, rebased) =>
      checkRebase(worktree, log, options, rebased, {
        ignored: [MANIFEST],
        checks: () => CHECKS.flatMap(runCheck(worktree)),
        passed: [`Checks passed: ${CHECKS.map(({ label }) => label).join(", ")}.`],
      }),
  });
};

if (process.argv[1] !== undefined && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  runCliMain(shipMain(USAGE, shipDocsRebase));
}
