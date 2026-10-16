#!/usr/bin/env node

// Rebase one pull request branch on the base branch during a serial merge.
// A branch from before changelog.d/ adds its entries to CHANGELOG.md, so each
// merge makes such a branch conflict there. This script keeps both sides of
// those conflicts, stops on any other conflict, and then checks that the
// rebase kept every change, every entry and every version bump of the branch.

import { existsSync, readFileSync, writeFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { runCliMain } from "./cli-main.mjs";
import { contractSurfacePaths } from "./contract-surfaces.mjs";
import {
  checkRebase,
  git,
  leasePushArgs,
  run,
  shipBranch,
  shipMain,
  worktreeCommand,
} from "./ship-git.mjs";

const CHANGELOG = "CHANGELOG.md";

const USAGE = `Usage: node scripts/ship-rebase.mjs (--pr <number> | --branch <name>) [options]

Rebase a pull request branch on the base branch. Resolve the conflicts
that have one mechanical answer, and stop on any other conflict:
  - CHANGELOG.md: keep both sides.
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
  --push            Push the result with a lease on the old branch tip
                    when all checks pass.
  -h, --help        Show this help.

The rebase, the checks and the push run in a temporary worktree with a
detached HEAD, so your checkout and every local branch stay as they are.
Your checkout can have changes, and it can be on any branch. Git rerere
is off for the rebase. The pre-push hook of your checkout runs on the push.
When the checks pass, the script removes the worktree. After a stop or a
failed check, the script keeps the worktree and prints its path and the
next commands.

After the rebase, the script runs these checks:
  - Outside CHANGELOG.md, the result is the same as a merge of the branch
    into the base. A rebase drops the changes of a merge commit. A path
    where the merge itself conflicts fails the check only when the branch
    has merge commits. Then compare that path by hand.
  - The branch adds and removes the same CHANGELOG lines as before.
  - Each entry that the branch adds to the first release section is still
    in that section. A release on the base can move an entry of the branch
    into the released version. A fix on the old branch does not help,
    because the rebase moves the entry again. When the moved entries are
    the only problem, the check prints the steps that fix the rebased
    branch in the kept worktree: move the entry back, commit, check the
    printed git diff, push with the printed lease command, and run the
    script again. After the push, the next run compares the pushed
    result with itself. It cannot check the first rebase or the manual
    fix again, so the git diff is the only check of the fix. When other
    checks also fail, the script prints no push command. Fix the other
    problems first.
  - The rebase adds no second subsection with the same name to the first
    release section.
  - Each cache version or schema version constant that the branch changed
    still changes.
The merge comparison needs Git 2.38 or later.

Exit codes: 0 when the rebase and the checks pass, 1 when the rebase stops
or a check fails, 2 for invalid input or a failed command.`;

const MARKER = {
  ours: /^<{7}(?: |\r?\n|$)/u,
  base: /^\|{7}(?: |\r?\n|$)/u,
  theirs: /^={7}(?:\r?\n|$)/u,
  end: /^>{7}(?: |\r?\n|$)/u,
};

/**
 * Replace each conflict block in `text` with the lines of both sides: first
 * the side of the base branch, then the side of the rebased commit. Returns
 * `{ text, conflicts }`. Throws when a block is not complete.
 */
export const keepBothSides = (text) => {
  const lines = text.split(/(?<=\n)/u);
  const output = [];
  let state = "outside";
  let ours = [];
  let theirs = [];
  let conflicts = 0;
  for (const line of lines) {
    if (state === "outside" && MARKER.ours.test(line)) {
      state = "ours";
      ours = [];
      theirs = [];
    } else if (state === "ours" && MARKER.base.test(line)) {
      state = "base";
    } else if ((state === "ours" || state === "base") && MARKER.theirs.test(line)) {
      state = "theirs";
    } else if (state === "theirs" && MARKER.end.test(line)) {
      output.push(...ours, ...theirs);
      conflicts += 1;
      state = "outside";
    } else if (state === "ours") {
      ours.push(line);
    } else if (state === "theirs") {
      theirs.push(line);
    } else if (state === "outside") {
      output.push(line);
    }
  }
  if (state !== "outside") {
    throw new Error("A conflict block has no end marker.");
  }
  return { text: output.join(""), conflicts };
};

const resolveChangelog = (cwd) => {
  const file = join(cwd, CHANGELOG);
  if (!existsSync(file)) {
    return { ok: false, reason: `${CHANGELOG} was deleted on one side.` };
  }
  let resolved;
  try {
    resolved = keepBothSides(readFileSync(file, "utf8"));
  } catch (error) {
    return { ok: false, reason: `${CHANGELOG}: ${error.message}` };
  }
  const { text, conflicts } = resolved;
  if (conflicts === 0) {
    return { ok: false, reason: `${CHANGELOG} is unmerged but has no conflict markers.` };
  }
  writeFileSync(file, text);
  git(cwd, ["add", "--", CHANGELOG]);
  return { ok: true };
};

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
  if (path === CHANGELOG) {
    return resolveChangelog(cwd);
  }
  if (GENERATED.has(path)) {
    // During a rebase, "ours" is the base side.
    git(cwd, ["checkout", "--ours", "--", path]);
    git(cwd, ["add", "--", path]);
    state.generated.add(path);
    return { ok: true };
  }
  writeFileSync(join(cwd, path), cacheResolution(cwd, path));
  git(cwd, ["add", "--", path]);
  state.cacheFiles.add(path);
  return { ok: true };
};

/**
 * Resolve the conflicts that have one mechanical answer: CHANGELOG.md (keep
 * both sides), generated contract files (take the base side) and cache
 * version constants (take the base side). Stop on any other conflict.
 */
const resolveMechanical = (cwd, state) => (paths) => {
  const others = paths.filter(
    (path) => path !== CHANGELOG && !GENERATED.has(path) && cacheResolution(cwd, path) === null,
  );
  if (others.length > 0) {
    return {
      ok: false,
      reason: `Conflicts outside ${CHANGELOG}, generated files and cache versions: ${others.join(", ")}`,
    };
  }
  for (const path of paths) {
    const result = resolvePath(cwd, path, state);
    if (!result.ok) {
      return result;
    }
  }
  return { ok: true };
};

const isEntryLine = (line) => line.trim() !== "" && !/^###\s/u.test(line);

/**
 * The CHANGELOG lines that `to` adds and removes against `from`. Blank lines
 * and `###` subsection headings are left out, because a correct resolution
 * can merge or move them.
 */
const changelogLineChanges = (cwd, from, to) => {
  const diff = git(cwd, ["diff", "--no-color", "--no-ext-diff", "-U0", from, to, "--", CHANGELOG]);
  const added = [];
  const removed = [];
  let inHunk = false;
  for (const line of diff.split("\n")) {
    if (line.startsWith("@@")) {
      inHunk = true;
    } else if (inHunk && line.startsWith("+") && isEntryLine(line.slice(1))) {
      added.push(line.slice(1));
    } else if (inHunk && line.startsWith("-") && isEntryLine(line.slice(1))) {
      removed.push(line.slice(1));
    }
  }
  return { added: added.toSorted(), removed: removed.toSorted() };
};

const multisetDifference = (left, right) => {
  const remaining = new Map();
  for (const line of right) {
    remaining.set(line, (remaining.get(line) ?? 0) + 1);
  }
  const extra = [];
  for (const line of left) {
    const count = remaining.get(line) ?? 0;
    if (count > 0) {
      remaining.set(line, count - 1);
    } else {
      extra.push(line);
    }
  }
  return extra;
};

/** The lines of the first `## ` section of `text`, without its heading. */
const firstSectionLines = (text) => {
  const lines = text.split("\n");
  const start = lines.findIndex((line) => line.startsWith("## "));
  if (start === -1) {
    return [];
  }
  const body = lines.slice(start + 1);
  const end = body.findIndex((line) => line.startsWith("## "));
  return end === -1 ? body : body.slice(0, end);
};

/** The count of each `###` heading in the first `## ` section of `text`. */
const subsectionCounts = (text) => {
  const counts = new Map();
  for (const line of firstSectionLines(text)) {
    if (/^###\s/u.test(line)) {
      const heading = line.trim();
      counts.set(heading, (counts.get(heading) ?? 0) + 1);
    }
  }
  return counts;
};

/**
 * The `###` headings that occur more than one time in the first `## `
 * section of `text`, and more often than in `baseText`. A keep-both
 * resolution can add a second `### Fixed` when both sides started the same
 * subsection. A duplicate that the base already has is not a result of the
 * rebase, so it is left out.
 */
export const duplicateSubsections = (text, baseText) => {
  const counts = subsectionCounts(text);
  const baseCounts = subsectionCounts(baseText);
  return [...counts.entries()]
    .filter(([heading, count]) => count > Math.max(1, baseCounts.get(heading) ?? 0))
    .map(([heading]) => heading);
};

/**
 * The entry lines that the branch adds to the first `## ` section and that
 * the rebase put in another section. `added` is the list of lines that the
 * branch adds before the rebase. A release on the base puts a version
 * heading under `## [Unreleased]`, and an entry of the branch can then land
 * under that version, with a clean rebase or with a keep-both resolution.
 * The line check does not see this, because the line itself does not change.
 * The result keeps the line order of the branch, so a wrapped entry stays
 * readable.
 */
export const movedEntries = ({ added, oldTipText, baseText, newTipText }) => {
  const oldFirst = firstSectionLines(oldTipText).filter(isEntryLine);
  const expected = multisetDifference(oldFirst, multisetDifference(oldFirst, added));
  const addedToFirst = multisetDifference(
    firstSectionLines(newTipText).filter(isEntryLine),
    firstSectionLines(baseText).filter(isEntryLine),
  );
  return multisetDifference(expected, addedToFirst);
};

/**
 * The steps that fix a moved entry. The next run fetches the remote branch
 * and rebases it again, so a fix helps only after it is pushed. After the
 * push, the branch is on the base, and the next run compares the pushed
 * result with itself. That run cannot find a problem of the first rebase or
 * of the manual fix, so step 2 is the only check of the fix.
 */
const moveBackSteps = ({ remote, branch, oldTip, newTip, base, worktree }) =>
  [
    `The temporary worktree ${worktree} holds the rebased branch. To fix it, work in that worktree:`,
    "  1. Move these entries to the first release section of CHANGELOG.md and commit the change.",
    "     Make sure that this command shows only the moved entries:",
    `       ${worktreeCommand(worktree, ["diff", newTip, "HEAD"])}`,
    "     The next run cannot check the manual fix, so this step is its only check.",
    "  2. Push the worktree HEAD with a lease on the old branch tip:",
    `       ${worktreeCommand(worktree, leasePushArgs({ remote, branch, oldTip }))}`,
    "  3. Remove the worktree with the command below.",
    `  4. Run the script again. It confirms that the branch is on ${base}.`,
    "     It cannot check the first rebase again.",
  ].join("\n  ");

// A push of the result makes the next run compare the pushed result with
// itself, so the other problems must be fixed before the push.
const OTHER_PROBLEMS_FIRST =
  "Fix the other problems first. After a push of the result, the next run cannot find them, because it compares the pushed result with itself.";

const formatProblem = (label, lines) =>
  `${label}:\n${lines.map((line) => `    ${line}`).join("\n")}`;

/**
 * The problem for the entries in `moved`. The recovery steps push HEAD, so
 * they are printed only when `alone` is true: when the moved entries are
 * the only problem of the run.
 */
const movedProblem = (moved, alone, target) => {
  const label = "Entries of the branch that moved out of the first release section";
  const advice = alone ? moveBackSteps(target) : OTHER_PROBLEMS_FIRST;
  return `${CHANGELOG}: ${formatProblem(label, moved)}\n  ${advice}`;
};

/**
 * Compare the CHANGELOG line changes of the branch before the rebase with
 * the changes after it. Each side is `{ added, removed }`. Returns a list of
 * problems. An empty list means the rebase kept every line the branch adds,
 * added no other line, and removed only the lines the branch removed before.
 */
export const changelogProblems = (before, after) =>
  [
    ["Lines that the branch added and the rebase lost", before.added, after.added],
    ["Lines that the rebase added and the branch did not add", after.added, before.added],
    ["Base lines that the rebase removed", after.removed, before.removed],
    ["Lines that the branch removed and the rebase kept", before.removed, after.removed],
  ]
    .map(([label, left, right]) => [label, multisetDifference(left, right)])
    .filter(([, lines]) => lines.length > 0)
    .map(([label, lines]) => formatProblem(label, lines));

/**
 * Check the CHANGELOG changes of the branch before the rebase
 * (`oldBase..oldTip`) against the changes after it (`baseTip..newTip`).
 * Then check the subsections of the first release section. Returns
 * `{ problems, moved }`, where `moved` is the list of new entries of the
 * branch that are no longer in the first release section.
 */
const checkChangelog = (cwd, { oldBase, oldTip, baseTip, newTip }) => {
  const before = changelogLineChanges(cwd, oldBase, oldTip);
  const problems = changelogProblems(before, changelogLineChanges(cwd, baseTip, newTip));
  const newTipText = git(cwd, ["show", `${newTip}:${CHANGELOG}`]);
  const baseText = git(cwd, ["show", `${baseTip}:${CHANGELOG}`]);
  const moved = movedEntries({
    added: before.added,
    oldTipText: git(cwd, ["show", `${oldTip}:${CHANGELOG}`]),
    baseText,
    newTipText,
  });
  const duplicates = duplicateSubsections(newTipText, baseText);
  if (duplicates.length > 0) {
    problems.push(
      formatProblem("Subsections that occur two times in the first release section", duplicates),
    );
  }
  return { problems: problems.map((problem) => `${CHANGELOG}: ${problem}`), moved };
};

// A cache version or a schema version constant. The value is an unsigned
// integer or a string literal.
const VERSION_LINE =
  /^([+-])\s*(?:pub(?:\([^)]*\))?\s+)?const\s+([A-Z0-9_]*(?:CACHE|SCHEMA)_VERSION)\s*:\s*(?:u8|u16|u32|u64|u128|usize|&(?:'static\s+)?str)\s*=\s*(\d+|"[^"]*")\s*;/u;

const DEV_NULL = "/dev/null";

/** The path of a `--- a/<path>` or `+++ b/<path>` diff header line. */
const headerPath = (line) => line.slice(4).replace(/^[ab]\//u, "");

/**
 * The version constants whose value `to` changes against `from`: the cache
 * versions (`GRAPH_CACHE_VERSION`, the extraction `CACHE_VERSION` and the
 * others) and the schema versions of the public output
 * (`CHECK_SCHEMA_VERSION` and the others). A deleted file is keyed by its
 * old path. Returns `[{ file, name, from, to }]`, sorted by file and name.
 */
const versionChanges = (cwd, from, to) => {
  const diff = git(cwd, ["diff", "--no-color", "--no-ext-diff", "-U0", from, to, "--", "*.rs"]);
  const values = new Map();
  let oldFile = null;
  let file = null;
  for (const line of diff.split("\n")) {
    if (line.startsWith("--- ")) {
      oldFile = headerPath(line);
      continue;
    }
    if (line.startsWith("+++ ")) {
      file = line.slice(4) === DEV_NULL ? oldFile : headerPath(line);
      continue;
    }
    const match = line.match(VERSION_LINE);
    if (match === null || file === null) {
      continue;
    }
    const [, sign, name, value] = match;
    const key = `${file}\0${name}`;
    const entry = values.get(key) ?? { file, name, from: null, to: null };
    entry[sign === "-" ? "from" : "to"] = value;
    values.set(key, entry);
  }
  return [...values.values()]
    .filter((entry) => entry.from !== entry.to)
    .toSorted((left, right) =>
      left.file === right.file
        ? left.name.localeCompare(right.name)
        : left.file.localeCompare(right.file),
    );
};

const formatVersionChange = ({ file, name, from, to }) =>
  `    ${file}: ${name} ${from ?? "(new)"} -> ${to ?? "(removed)"}`;

/**
 * Check that each version constant that the branch changed before the
 * rebase still changes after it. A lost cache bump lets two pull requests
 * ship the same cache version with a different cache format. A lost schema
 * bump does the same for the public output.
 */
const checkVersions = (before, after) => {
  const kept = new Set(after.map(({ file, name }) => `${file}\0${name}`));
  const lost = before.filter(({ file, name }) => !kept.has(`${file}\0${name}`));
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
  const kept = new Set(
    versionChanges(cwd, baseTip, "HEAD").map(({ file, name }) => `${file}\0${name}`),
  );
  const bumped = versionChanges(cwd, oldBase, oldTip)
    .filter(isCacheBump)
    .filter(({ file, name }) => !kept.has(`${file}\0${name}`))
    .filter(({ file, name }) => bumpConstant(cwd, file, name));
  if (bumped.length > 0) {
    git(cwd, ["add", "--", ...new Set(bumped.map(({ file }) => file))]);
    run(
      "git",
      [...hooks, "commit", "--quiet", "-m", "chore: bump cache versions above the base branch"],
      {
        cwd,
      },
    );
  }
  return bumped;
};

const regenerateSteps = ({ remote, branch, oldTip, worktree }) =>
  [
    `The temporary worktree ${worktree} holds the rebased branch. Regenerate there:`,
    `  1. ${worktreeCommand(worktree, ["status"])} must be clean. Then, in that directory:`,
    "     npm ci && pnpm --dir editors/vscode install && npm run generate:contracts",
    "  2. Commit the regenerated files.",
    "  3. Push the worktree HEAD with a lease on the old branch tip:",
    `       ${worktreeCommand(worktree, leasePushArgs({ remote, branch, oldTip }))}`,
    "  4. Remove the worktree with the command below.",
  ].join("\n  ");

/**
 * Generated files that took the base side lack the change of the branch
 * until a regeneration, so the run must not push them as they are.
 */
const generatedProblem = (paths, alone, target) =>
  `${formatProblem("Generated files that took the base side and need a regeneration", paths)}\n  ${
    alone ? regenerateSteps(target) : OTHER_PROBLEMS_FIRST
  }`;

/**
 * The problems with recovery steps. Each set of steps ends with a push, so
 * a problem prints its steps only when it is the only problem of the run.
 */
const recoveryProblems = (moved, generated, alone, target) =>
  [
    { items: moved, problem: movedProblem },
    { items: generated, problem: generatedProblem },
  ]
    .filter(({ items }) => items.length > 0)
    .map(({ items, problem }, _, active) => problem(items, alone && active.length === 1, target));

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
    ignored: [
      CHANGELOG,
      ...generated,
      ...state.cacheFiles,
      ...state.bumped.map(({ file }) => file),
    ],
    checks: (tips, treeProblems) => {
      const changelog = checkChangelog(cwd, tips);
      const others = [
        ...changelog.problems,
        ...checkVersions(versionChanges(cwd, oldBase, oldTip), versionsAfter),
      ];
      const target = { ...tips, remote: rebased.remote, base: options.base };
      const alone = treeProblems.length === 0 && others.length === 0;
      return [...recoveryProblems(changelog.moved, generated, alone, target), ...others];
    },
    passed: [
      "CHANGELOG check: the rebase kept the lines of the branch and removed no base line.",
      "CHANGELOG check: the new entries of the branch are in the first release section.",
    ],
  });
};

/**
 * Rebase the branch in a temporary worktree and run the checks. Print the
 * report to `log` and return the exit code.
 */
const shipRebase = (cwd, options, log = console.log) => {
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
