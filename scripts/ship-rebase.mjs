#!/usr/bin/env node

// Rebase one pull request branch on the base branch during a serial merge.
// The script resolves the conflicts that have one mechanical answer (cache
// version constants and generated contract files), stops on any other
// conflict, and then checks that the rebase kept every change and every
// version bump of the branch.

import { existsSync, readFileSync, writeFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { branchVersionBumps } from "./check-version-bumps.mjs";
import { runCliMain } from "./cli-main.mjs";
import { contractSurfacePaths } from "./contract-surfaces.mjs";
import {
  checkRebase,
  fetchShipRefs,
  git,
  leasePushArgs,
  run,
  shipBranch,
  shipMain,
  worktreeCommand,
} from "./ship-git.mjs";
import {
  formatVersionChange,
  lostVersionChanges,
  versionChanges,
  versionKeys,
} from "./version-constants.mjs";

const USAGE = `Usage: node scripts/ship-rebase.mjs (--pr <number> | --branch <name>) [options]

Rebase a pull request branch on the base branch. Resolve the conflicts
that have one mechanical answer, and stop on any other conflict:
  - A cache version constant (*CACHE_VERSION): take the base value. After
    the rebase, a new commit sets each cache bump of the branch that the
    rebase lost to the base value + 1.
  - A generated contract file: take the base side. The script then stops
    before the push and prints the regeneration steps.

Options:
  --pr <number>     Read the head branch of this pull request with gh.
  --branch <name>   The branch name on the remote.
  --base <ref>      The ref to rebase onto (default: <remote>/main).
  --remote <name>   The remote of the base and the branch (default: origin).
  --head-remote <remote>
                    The remote or URL that holds the branch, when it is not
                    --remote. With --pr, a pull request from a fork that
                    allows maintainer edits sets it to the fork.
  --upstream <commit>
                    The old tip of the parent branch of a stacked branch.
                    Use it after the parent pull request merged with a
                    squash. The script then rebases only the commits after
                    this commit (git rebase --onto <base> <commit>).
  --push            Push the result with a lease on the old branch tip
                    when all checks pass.
  --if-needed       Rebase only when a squash merge of the branch is not
                    safe as it is. The rebase runs when the merge into the
                    base conflicts, when the branch and the base both change
                    a generated file, or when the merge loses a version bump
                    of the branch. Otherwise the script prints the merge
                    command for the current head and exits with 0. It has
                    no effect with --upstream.
  -h, --help        Show this help.

The rebase, the checks and the push run in a temporary worktree with a
detached HEAD, so your checkout and every local branch stay as they are.
Your checkout can have changes, and it can be on any branch. Git rerere
is off for the rebase. The pre-push hook of your checkout runs on the push.
When the checks pass, the script removes the worktree. After a stop or a
failed check, the script keeps the worktree and prints its path and the
next commands.

After the rebase, the script runs these checks:
  - The result is the same as a merge of the branch into the base. A
    rebase drops the changes of a merge commit. A path where the merge
    itself conflicts fails the check only when the branch has merge
    commits. Then compare that path by hand.
  - Each cache version or schema version constant that the branch changed
    still changes.
The merge comparison needs Git 2.38 or later.

With --upstream, the checks compare only the commits after that commit:
  - The tree check merges the branch into the base with that commit as the
    merge base, because the base already holds the squash of the parent.
  - The hunk check compares the hunks of <commit>..<old tip> with the hunks
    of <base>..<new tip>, without context lines. A hunk that the rebase lost
    or changed fails the check, also when the merge has the same result.
    The check leaves out the generated files that took the base side and
    the cache version lines. A failure prints the git range-diff command
    that compares the commits.
  - The version checks use that commit as the old base of the branch.
The merge comparison with --upstream needs Git 2.40 or later.

Exit codes: 0 when the rebase and the checks pass, 1 when the rebase stops
or a check fails, 2 for invalid input or a failed command.`;

/** Generated contract files. A rebase takes the base side; a regeneration fixes them. */
const GENERATED = new Set(contractSurfacePaths());

const CACHE_DECL =
  /^(?<head>\s*(?:pub(?:\([^)]*\))?\s+)?const\s+(?<name>[A-Z0-9_]*CACHE_VERSION)\s*:\s*(?:u8|u16|u32|u64|u128|usize)\s*=\s*)(?<value>\d+)(?<tail>\s*;.*)$/u;

// REBASE_CONFIG sets merge.conflictStyle=merge, so a block has no base part.
const CONFLICT_BLOCK = /^<{7}[^\n]*\n([\s\S]*?)^={7}\n([\s\S]*?)^>{7}[^\n]*\n?/gmu;

/** The cache version names that one side of a block declares, or null. */
const cacheDeclNames = (side) => {
  const names = side
    .split("\n")
    .filter((line) => line.trim() !== "")
    .map((line) => CACHE_DECL.exec(line)?.groups.name ?? null);
  return names.length === 0 || names.includes(null) ? null : names.join(",");
};

/**
 * Resolve each conflict block in which both sides declare the same cache
 * version constants and nothing else: keep the base side. The bump of the
 * branch comes back after the rebase as base value + 1 (`restoreCacheBumps`).
 * Returns the resolved text, or null when a block is not such a block.
 */
export const takeBaseCacheVersions = (text) => {
  let blocks = 0;
  let others = 0;
  const resolved = text.replace(CONFLICT_BLOCK, (block, ours, theirs) => {
    blocks += 1;
    const names = cacheDeclNames(ours);
    if (names === null || names !== cacheDeclNames(theirs)) {
      others += 1;
      return block;
    }
    return ours;
  });
  return blocks > 0 && others === 0 ? resolved : null;
};

const cacheResolution = (cwd, path) => {
  const file = join(cwd, path);
  return path.endsWith(".rs") && existsSync(file)
    ? takeBaseCacheVersions(readFileSync(file, "utf8"))
    : null;
};

const resolvePath = (cwd, path, state) => {
  if (GENERATED.has(path)) {
    // During a rebase, "ours" is the base side.
    git(cwd, ["checkout", "--ours", "--", path]);
    git(cwd, ["add", "--", path]);
    state.generated.add(path);
    return;
  }
  writeFileSync(join(cwd, path), cacheResolution(cwd, path));
  git(cwd, ["add", "--", path]);
  state.cacheFiles.add(path);
};

/**
 * Resolve the conflicts that have one mechanical answer: generated contract
 * files (take the base side) and cache version constants (take the base
 * side). Stop on any other conflict.
 */
const resolveMechanical = (cwd, state) => (paths) => {
  const others = paths.filter(
    (path) => !GENERATED.has(path) && cacheResolution(cwd, path) === null,
  );
  if (others.length > 0) {
    return {
      ok: false,
      reason: `Conflicts outside generated files and cache versions: ${others.join(", ")}`,
    };
  }
  for (const path of paths) {
    resolvePath(cwd, path, state);
  }
  return { ok: true };
};

/**
 * Check that each version constant that the branch changed before the
 * rebase still changes after it. A lost cache bump lets two pull requests
 * ship the same cache version with a different cache format. A lost schema
 * bump does the same for the public output.
 */
const checkVersions = (before, after) => {
  const lost = lostVersionChanges(before, after);
  if (lost.length === 0) {
    return [];
  }
  return [`Version changes that the rebase lost:\n${lost.map(formatVersionChange).join("\n")}`];
};

/** Print the version constants that the rebased branch changes. */
const logVersions = (log, base, changes) =>
  log(
    changes.length === 0
      ? `Version constants: no change against ${base}.`
      : `Version constants against ${base}:\n${changes.map(formatVersionChange).join("\n")}`,
  );

/** True for a `-` or `+` diff line that declares a cache version constant. */
const isCacheVersionDiffLine = (line) => /^[-+]/u.test(line) && CACHE_DECL.test(line.slice(1));

const isCacheBump = ({ name, to }) => name.endsWith("CACHE_VERSION") && /^\d+$/u.test(to ?? "");

/** Set `name` in `file` to its current value + 1. Returns false when it is absent. */
const bumpConstant = (cwd, file, name) => {
  const path = join(cwd, file);
  if (!existsSync(path)) {
    return false;
  }
  const lines = readFileSync(path, "utf8").split("\n");
  const index = lines.findIndex((line) => CACHE_DECL.exec(line)?.groups.name === name);
  if (index === -1) {
    return false;
  }
  const { head, value, tail } = CACHE_DECL.exec(lines[index]).groups;
  lines[index] = `${head}${Number(value) + 1}${tail}`;
  writeFileSync(path, lines.join("\n"));
  return true;
};

/**
 * A cache bump of the branch that the rebase lost (the base side won a
 * conflict, or the base made the same bump) gets the base value + 1 in a new
 * commit. Two branches must never ship one cache version with two formats.
 * Returns the bumped `{ file, name }` entries.
 */
const restoreCacheBumps = (cwd, { oldBase, oldTip, baseTip, hooks }) => {
  const kept = versionKeys(versionChanges(cwd, baseTip, "HEAD"));
  const bumped = versionChanges(cwd, oldBase, oldTip)
    .filter(isCacheBump)
    .filter(({ file, name }) => !kept.has(`${file}\0${name}`))
    .filter(({ file, name }) => bumpConstant(cwd, file, name));
  if (bumped.length > 0) {
    git(cwd, ["add", "--", ...new Set(bumped.map(({ file }) => file))]);
    run(
      "git",
      [
        ...hooks,
        "commit",
        "-s",
        "--quiet",
        "-m",
        "chore: bump cache versions above the base branch",
      ],
      {
        cwd,
      },
    );
  }
  return bumped;
};

const formatProblem = (label, lines) =>
  `${label}:\n${lines.map((line) => `    ${line}`).join("\n")}`;

const regenerateSteps = ({ remote, branch, oldTip, worktree }) =>
  [
    `The temporary worktree ${worktree} holds the rebased branch. Regenerate there:`,
    `  1. ${worktreeCommand(worktree, ["status"])} must be clean. Then, in that directory:`,
    "     pnpm install && pnpm --dir editors/vscode install && npm run generate:contracts",
    "  2. Commit the regenerated files.",
    "  3. Push the worktree HEAD with a lease on the old branch tip:",
    `       ${worktreeCommand(worktree, leasePushArgs({ remote, branch, oldTip }))}`,
    "  4. Remove the worktree with the command below.",
  ].join("\n  ");

// A push of the result makes the next run compare the pushed result with
// itself, so the other problems must be fixed before the push.
const OTHER_PROBLEMS_FIRST =
  "Fix the other problems first. After a push of the result, the next run cannot find them, because it compares the pushed result with itself.";

/**
 * Generated files that took the base side lack the change of the branch
 * until a regeneration, so the run must not push them as they are. The
 * regeneration steps end with a push, so they are printed only when `alone`
 * is true: when these files are the only problem of the run.
 */
const generatedProblems = (paths, alone, target) =>
  paths.length === 0
    ? []
    : [
        `${formatProblem("Generated files that took the base side and need a regeneration", paths)}\n  ${
          alone ? regenerateSteps(target) : OTHER_PROBLEMS_FIRST
        }`,
      ];

/**
 * Run the checks of the rebase result in the temporary worktree `cwd`.
 * Print the report to `log` and return the exit code.
 */
const checkResult = (cwd, log, options, rebased, state) => {
  const { oldBase, oldTip, baseTip, newTip } = rebased;
  const versionsAfter = versionChanges(cwd, baseTip, newTip);
  logVersions(log, options.base, versionsAfter);
  for (const { file, name } of state.bumped) {
    log(`Cache version: ${file}: ${name} set to the base value + 1 in a new commit.`);
  }
  const generated = [...state.generated].toSorted();
  return checkRebase(cwd, log, options, rebased, {
    ignored: [...generated, ...state.cacheFiles, ...state.bumped.map(({ file }) => file)],
    // A cache version line takes the base value + 1, so only that line of
    // a cache file can differ from the branch.
    hunks: { ignored: generated, dropLine: isCacheVersionDiffLine },
    checks: (tips, treeProblems) => {
      const others = checkVersions(versionChanges(cwd, oldBase, oldTip), versionsAfter);
      const target = { ...tips, remote: rebased.remote };
      const alone = treeProblems.length === 0 && others.length === 0;
      return [...generatedProblems(generated, alone, target), ...others];
    },
    passed: [],
  });
};

// `git merge-tree` exits with 1 when the merge has conflicts.
const MERGE_TREE_CONFLICT_EXIT = 1;

/** Paths that `from..to` changes. */
const changedPaths = (cwd, from, to) =>
  new Set(
    git(cwd, ["diff", "--name-only", "--no-renames", "-z", from, to])
      .split("\0")
      .filter((path) => path !== ""),
  );

/**
 * The reasons why a squash merge of the old branch tip into the base is not
 * safe without a rebase. An empty list means that the merge has no
 * conflict, keeps each version bump of the branch, and needs no
 * mechanical resolution that the rebase does. The bumps of the branch come
 * from its own commits, because a merge of the base into the branch moves
 * the merge base past an equal bump.
 */
const rebaseReasons = (cwd, { oldBase, oldTip, baseTip }) => {
  const merge = run("git", ["merge-tree", "--write-tree", "--no-messages", baseTip, oldTip], {
    cwd,
    allowFailure: true,
  });
  if (merge.status === MERGE_TREE_CONFLICT_EXIT) {
    return ["The merge into the base has a conflict."];
  }
  if (merge.status !== 0) {
    throw new Error(`git merge-tree failed (exit ${merge.status})\n${merge.stderr.trim()}`);
  }
  const mergeTree = merge.stdout.trim().split("\n")[0];
  const reasons = [];
  const branchPaths = changedPaths(cwd, oldBase, oldTip);
  const basePaths = changedPaths(cwd, oldBase, baseTip);
  const generated = [...GENERATED].filter((path) => branchPaths.has(path) && basePaths.has(path));
  if (generated.length > 0) {
    reasons.push(
      formatProblem("Generated files that the branch and the base both change", generated),
    );
  }
  reasons.push(
    ...checkVersions(
      branchVersionBumps(cwd, baseTip, oldTip),
      versionChanges(cwd, baseTip, mergeTree),
    ),
  );
  return reasons;
};

/**
 * With `--if-needed`, report whether the branch can merge without a rebase.
 * Returns 0 when it can, or `null` when the rebase must run.
 */
const skipNeedlessRebase = (cwd, log, options) => {
  if (!options.ifNeeded || options.upstream !== null) {
    return null;
  }
  const refs = fetchShipRefs(cwd, options);
  const reasons = rebaseReasons(cwd, refs);
  if (reasons.length > 0) {
    log(`Rebase needed for ${refs.branch}:\n${reasons.map((reason) => `  ${reason}`).join("\n")}`);
    return null;
  }
  const pr = options.pr ?? "<number>";
  log(
    [
      `No rebase needed: ${refs.branch} merges into ${options.base} without a conflict and keeps each version bump.`,
      "Merge the reviewed head without a rebase:",
      `  gh pr merge ${pr} --auto --squash --match-head-commit ${refs.oldTip} --subject "<conventional subject> (#${pr})"`,
    ].join("\n"),
  );
  return 0;
};

/**
 * Rebase the branch in a temporary worktree and run the checks. Print the
 * report to `log` and return the exit code.
 */
const shipRebase = (cwd, options, log = console.log) => {
  const skipped = skipNeedlessRebase(cwd, log, options);
  if (skipped !== null) {
    return skipped;
  }
  const state = { generated: new Set(), cacheFiles: new Set(), bumped: [] };
  return shipBranch(cwd, log, options, {
    resolveConflicts: (worktree) => resolveMechanical(worktree, state),
    afterRebase: (worktree, refs) => {
      state.bumped = restoreCacheBumps(worktree, refs);
    },
    check: (worktree, rebased) => checkResult(worktree, log, options, rebased, state),
  });
};

if (process.argv[1] !== undefined && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  runCliMain(shipMain(USAGE, shipRebase));
}
