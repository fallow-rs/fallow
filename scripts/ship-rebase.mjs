#!/usr/bin/env node

// Rebase one pull request branch on the base branch during a serial merge.
// Every pull request adds CHANGELOG entries, so each merge makes the next
// branch conflict in CHANGELOG.md. This script keeps both sides of those
// conflicts, stops on any other conflict, and then checks that the rebase
// kept every change, every entry and every version bump of the branch.

import { existsSync, readFileSync, writeFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { runCliMain } from "./cli-main.mjs";
import {
  checkRebase,
  git,
  leasePushArgs,
  shipBranch,
  shipMain,
  worktreeCommand,
} from "./ship-git.mjs";

const CHANGELOG = "CHANGELOG.md";

const USAGE = `Usage: node scripts/ship-rebase.mjs (--pr <number> | --branch <name>) [options]

Rebase a pull request branch on the base branch. Resolve CHANGELOG.md
conflicts by keeping both sides. Stop on any other conflict.

Options:
  --pr <number>     Read the head branch of this pull request with gh.
  --branch <name>   The branch name on the remote.
  --base <ref>      The ref to rebase onto (default: <remote>/main).
  --remote <name>   The remote of the branch (default: origin).
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

const resolveChangelogOnly = (cwd) => (paths) => {
  const others = paths.filter((path) => path !== CHANGELOG);
  if (others.length > 0) {
    return { ok: false, reason: `Conflicts outside ${CHANGELOG}: ${others.join(", ")}` };
  }
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

/**
 * Run the checks of the rebase result in the temporary worktree `cwd`.
 * Print the report to `log` and return the exit code.
 */
const checkResult = (cwd, log, options, rebased) => {
  const { oldBase, oldTip, baseTip, newTip } = rebased;
  const versionsAfter = versionChanges(cwd, baseTip, newTip);
  logVersions(log, options.base, versionsAfter);
  return checkRebase(cwd, log, options, rebased, {
    ignored: [CHANGELOG],
    checks: (tips, treeProblems) => {
      const changelog = checkChangelog(cwd, tips);
      const others = [
        ...changelog.problems,
        ...checkVersions(versionChanges(cwd, oldBase, oldTip), versionsAfter),
      ];
      if (changelog.moved.length === 0) {
        return others;
      }
      const alone = treeProblems.length === 0 && others.length === 0;
      const target = { ...tips, remote: options.remote, base: options.base };
      return [movedProblem(changelog.moved, alone, target), ...others];
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
const shipRebase = (cwd, options, log = console.log) =>
  shipBranch(cwd, log, options, {
    resolveConflicts: resolveChangelogOnly,
    check: (worktree, rebased) => checkResult(worktree, log, options, rebased),
  });

if (process.argv[1] !== undefined && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  runCliMain(shipMain(USAGE, shipRebase));
}
