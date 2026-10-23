#!/usr/bin/env node

// Fail a pull request when a cache version or schema version bump of the
// branch no longer changes the value on the current base. The base can make
// the same bump after the fork: git then merges the two equal edits with no
// conflict, and a warm cache of the old format matches the new version.

import { resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { parseArgs } from "node:util";

import { runCliMain } from "./cli-main.mjs";
import { git } from "./ship-git.mjs";
import { formatVersionChange, lostVersionChanges, versionChanges } from "./version-constants.mjs";

const USAGE = `Usage: node scripts/check-version-bumps.mjs --base <ref> [--head <ref>]

Find each *CACHE_VERSION and *SCHEMA_VERSION constant that the commits of
<head> change (the non-merge commits that <base> does not contain). Fail
when <head> has the same value as <base> for such a constant. <head>
defaults to HEAD.`;

/**
 * The net version changes of the non-merge commits in `base..head`: the
 * first old value and the last new value of each constant. A merge of the
 * base into the branch moves the merge base past an equal bump, so the
 * merge base cannot show the bumps of the branch.
 */
export const branchVersionBumps = (cwd, base, head) => {
  const commits = git(cwd, ["rev-list", "--no-merges", "--reverse", `${base}..${head}`])
    .split("\n")
    .filter((commit) => commit !== "");
  const net = new Map();
  for (const commit of commits) {
    for (const change of versionChanges(cwd, `${commit}^`, commit)) {
      const key = `${change.file}\0${change.name}`;
      net.set(key, { ...change, from: net.get(key)?.from ?? change.from });
    }
  }
  // A removal shares no version with a new format, and a reverted bump is
  // no bump.
  return [...net.values()].filter(({ from, to }) => to !== null && from !== to);
};

/**
 * The bumps of the branch for which `head` has the same value as `base`.
 */
export const staleVersionBumps = (cwd, base, head) =>
  lostVersionChanges(branchVersionBumps(cwd, base, head), versionChanges(cwd, base, head));

const main = () => {
  const { values } = parseArgs({
    options: { base: { type: "string" }, head: { type: "string", default: "HEAD" } },
  });
  if (values.base === undefined) {
    console.error(USAGE);
    return 2;
  }
  const stale = staleVersionBumps(process.cwd(), values.base, values.head);
  if (stale.length === 0) {
    console.log(`Version constants: each bump still changes the value on ${values.base}.`);
    return 0;
  }
  console.error(
    [
      `Version bumps of the branch that no longer change the value on ${values.base}:`,
      ...stale.map(formatVersionChange),
      `Set each constant above the value on ${values.base}.`,
    ].join("\n"),
  );
  return 1;
};

if (process.argv[1] !== undefined && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  runCliMain(main);
}
