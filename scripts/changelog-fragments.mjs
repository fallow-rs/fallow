#!/usr/bin/env node

// Each pull request writes its changelog entries to its own file under
// changelog.d/. Two pull requests then never change the same lines, so a merge
// of one cannot make the other conflict. The release collects the fragments
// into the [Unreleased] section of CHANGELOG.md.

import { execFileSync } from "node:child_process";
import { existsSync, readdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { parseArgs as parseOptions } from "node:util";

import { runCliMain } from "./cli-main.mjs";

export const FRAGMENT_DIR = "changelog.d";
const CHANGELOG = "CHANGELOG.md";
const README = "README.md";
const EM_DASH = String.fromCodePoint(0x2014);

/** The `###` headings a fragment may use, in the order of a release section. */
export const KINDS = [
  "Breaking",
  "Added",
  "Changed",
  "Deprecated",
  "Removed",
  "Fixed",
  "Performance",
  "Security",
];

const USAGE = `Usage: node scripts/changelog-fragments.mjs <command> [options]

Commands:
  check [--base <ref>]  Validate every fragment in ${FRAGMENT_DIR}/. With --base,
                        also fail when the [Unreleased] section of ${CHANGELOG}
                        differs from <ref>: a pull request adds a fragment and
                        leaves ${CHANGELOG} alone.
  collect [--dry-run]   Move every fragment into the [Unreleased] section of
                        ${CHANGELOG} and delete the fragment files. --dry-run
                        prints the section and changes nothing.`;

const HEADING = /^### (?<name>.+?)\s*$/u;
const FENCE = /^\s*(```|~~~)/u;

/**
 * Split markdown into the text before the first `###` heading and one block
 * per heading. A `###` line inside a fenced code block is body text.
 */
export const splitSections = (text) => {
  const blocks = [{ name: null, lines: [] }];
  let fenced = false;
  for (const line of text.split(/\r?\n/u)) {
    fenced = FENCE.test(line) !== fenced;
    const heading = fenced ? null : HEADING.exec(line);
    if (heading) {
      blocks.push({ name: heading.groups.name, lines: [] });
    } else {
      blocks.at(-1).lines.push(line);
    }
  }
  const [preamble, ...sections] = blocks.map(({ name, lines }) => ({
    name,
    body: lines.join("\n").trim(),
  }));
  return { preamble: preamble.body, sections };
};

const KIND_LIST = `use one of: ${KINDS.join(", ")}`;

const sectionProblems = (sections) =>
  sections.flatMap(({ name, body }, index) => [
    KINDS.includes(name) ? null : `has the unknown heading "### ${name}" (${KIND_LIST})`,
    sections.findIndex((other) => other.name === name) < index ? `repeats "### ${name}"` : null,
    body === "" ? `has an empty "### ${name}" section` : null,
  ]);

/** The problems of one fragment, each prefixed with its path. */
export const fragmentProblems = (path, text) => {
  const { preamble, sections } = splitSections(text);
  return [
    preamble === "" ? null : "has text before its first ### heading",
    sections.length === 0 ? `has no ### heading (${KIND_LIST})` : null,
    ...sectionProblems(sections),
    text.includes(EM_DASH) ? "contains an em-dash, which the release notes inherit" : null,
  ]
    .filter((problem) => problem !== null)
    .map((problem) => `${path}: ${problem}`);
};

/** Fragment file names in `dir`, sorted, without the README. */
export const fragmentNames = (dir) =>
  existsSync(dir)
    ? readdirSync(dir)
        .filter((name) => name.endsWith(".md") && name !== README)
        .toSorted()
    : [];

const UNRELEASED = /^## \[Unreleased\][^\n]*\n/mu;
const NEXT_SECTION = /^## \[/mu;

/** The `[Unreleased]` body, with its offsets in the changelog. */
export const unreleasedSection = (changelog) => {
  const header = UNRELEASED.exec(changelog);
  if (!header) {
    throw new Error(`${CHANGELOG} has no "## [Unreleased]" section`);
  }
  const start = header.index + header[0].length;
  const next = NEXT_SECTION.exec(changelog.slice(start));
  const end = next ? start + next.index : changelog.length;
  return { start, end, body: changelog.slice(start, end) };
};

/**
 * Merge the existing `[Unreleased]` body and the fragments, in that order,
 * into one body with each heading once. Known headings follow `KINDS`; other
 * headings that the existing body already had keep their order after them.
 */
export const mergeUnreleased = (existing, fragments) => {
  const merged = new Map();
  const add = ({ name, body }) => {
    if (body !== "") {
      merged.set(name, [...(merged.get(name) ?? []), body]);
    }
  };
  const current = splitSections(existing);
  if (current.preamble !== "") {
    throw new Error(`${CHANGELOG}: the [Unreleased] section has text before its first ### heading`);
  }
  current.sections.forEach(add);
  fragments.forEach((text) => splitSections(text).sections.forEach(add));

  const order = [
    ...KINDS.filter((name) => merged.has(name)),
    ...[...merged.keys()].filter((name) => !KINDS.includes(name)),
  ];
  return order.map((name) => `### ${name}\n\n${merged.get(name).join("\n\n")}\n`).join("\n");
};

// CHANGELOG.md is larger than the 1 MiB default buffer of `execFileSync`.
const GIT_MAX_BUFFER = 256 * 1024 * 1024;

const git = (cwd, args) =>
  execFileSync("git", args, {
    cwd,
    encoding: "utf8",
    maxBuffer: GIT_MAX_BUFFER,
    stdio: ["ignore", "pipe", "pipe"],
  });

/**
 * Seconds since the epoch of the commit that added each fragment, so the
 * release lists entries in merge order. An untracked fragment has no entry.
 */
const addedTimes = (root) => {
  const log = git(root, [
    "log",
    "--diff-filter=A",
    "--format=%x00%ct",
    "--name-only",
    "--",
    FRAGMENT_DIR,
  ]);
  const times = new Map();
  for (const record of log.split("\0").slice(1)) {
    const [time, ...paths] = record.trim().split("\n");
    // `git log` lists the newest commit first. A fragment that a later commit
    // deleted and added again keeps its newest add time.
    for (const path of paths.filter((name) => name !== "" && !times.has(name))) {
      times.set(path, Number(time));
    }
  }
  return times;
};

/** Fragment names in merge order; untracked fragments last, by name. */
export const orderedFragments = (root) => {
  const times = addedTimes(root);
  const timeOf = (name) => times.get(`${FRAGMENT_DIR}/${name}`) ?? Number.POSITIVE_INFINITY;
  return fragmentNames(join(root, FRAGMENT_DIR)).toSorted(
    (a, b) => timeOf(a) - timeOf(b) || a.localeCompare(b),
  );
};

const assertNoProblems = (problems) => {
  if (problems.length > 0) {
    throw new Error(problems.join("\n"));
  }
};

const checkFragments = (root) => {
  const dir = join(root, FRAGMENT_DIR);
  return fragmentNames(dir).flatMap((name) =>
    fragmentProblems(`${FRAGMENT_DIR}/${name}`, readFileSync(join(dir, name), "utf8")),
  );
};

const unreleasedAt = (root, ref) =>
  unreleasedSection(git(root, ["show", `${ref}:${CHANGELOG}`])).body.trim();

/**
 * A pull request that edits `[Unreleased]` brings back the conflicts that the
 * fragments remove, so the check compares the section with the fork point of
 * the branch. A release on the base branch after the fork is not a change of
 * the branch.
 */
const checkUnreleasedUnchanged = (root, base) => {
  const forkPoint = git(root, ["merge-base", "HEAD", base]).trim();
  const current = unreleasedSection(readFileSync(join(root, CHANGELOG), "utf8")).body.trim();
  if (current === unreleasedAt(root, forkPoint)) {
    return [];
  }
  return [
    `${CHANGELOG}: this branch changes the [Unreleased] section. Move the new entries to a file in ${FRAGMENT_DIR}/ (see ${FRAGMENT_DIR}/${README}) and restore the section as it is on ${base}.`,
  ];
};

const collect = (root, dryRun, log) => {
  assertNoProblems(checkFragments(root));
  const dir = join(root, FRAGMENT_DIR);
  const names = orderedFragments(root);
  const path = join(root, CHANGELOG);
  const changelog = readFileSync(path, "utf8");
  const { start, end, body } = unreleasedSection(changelog);
  const merged = mergeUnreleased(
    body,
    names.map((name) => readFileSync(join(dir, name), "utf8")),
  );
  if (dryRun) {
    log(merged);
    return 0;
  }
  const section = merged === "" ? "\n" : `\n${merged}\n`;
  writeFileSync(path, `${changelog.slice(0, start)}${section}${changelog.slice(end)}`);
  for (const name of names) {
    rmSync(join(dir, name));
  }
  log(`Collected ${names.length} fragment(s) into the [Unreleased] section of ${CHANGELOG}.`);
  return 0;
};

const COMMAND_OPTIONS = {
  check: { base: { type: "string" } },
  collect: { "dry-run": { type: "boolean" } },
};

export const parseArgs = ([command, ...args]) => {
  if (!Object.hasOwn(COMMAND_OPTIONS, command ?? "")) {
    throw new Error(USAGE);
  }
  const { values } = parseOptions({ args, options: COMMAND_OPTIONS[command], strict: true });
  return { command, base: values.base, dryRun: values["dry-run"] === true };
};

/** Run one command against the repository at `root`. Return the exit code. */
export const changelogFragments = (
  argv,
  root = resolve(dirname(fileURLToPath(import.meta.url)), ".."),
  log = console.log,
) => {
  const options = parseArgs(argv);
  if (options.command === "collect") {
    return collect(root, options.dryRun, log);
  }
  assertNoProblems([
    ...checkFragments(root),
    ...(options.base === undefined ? [] : checkUnreleasedUnchanged(root, options.base)),
  ]);
  log(`${FRAGMENT_DIR}: ${fragmentNames(join(root, FRAGMENT_DIR)).length} fragment(s) are valid.`);
  return 0;
};

if (process.argv[1] !== undefined && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  runCliMain(() => changelogFragments(process.argv.slice(2)));
}
