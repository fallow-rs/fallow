#!/usr/bin/env node
/**
 * Prepare one companion commit that publishes the released Fallow skills.
 *
 * The CI job "Public skills contract" compares `npm/fallow/skills` with the
 * companion repository `fallow-rs/fallow-skills`. When a change to the skill
 * lands on `main`, the companion needs a commit that:
 *
 * 1. copies the skill contract from a fallow commit on `origin/main` (the pin),
 * 2. raises the patch version in the four companion version fields,
 * 3. sets `commit` in `source-lock.json` to the pin,
 * 4. passes the checks of the companion.
 *
 * This script makes that commit. It does not push. The pin is HEAD, or the
 * commit that `--pin` names. The script reads the skill from the tree of the
 * pin, not from the working tree.
 *
 * Usage:
 *   node scripts/sync-skills-companion.mjs [--pin <sha>] [--message <text>] [--dry-run]
 *   node scripts/sync-skills-companion.mjs --help
 *
 * `FALLOW_SKILLS_DIR` may point to the companion checkout, as for
 * `scripts/vendor-skills.mjs`.
 */

import { execFileSync, spawnSync } from "node:child_process";
import { existsSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { devNull, tmpdir } from "node:os";
import { dirname, join, relative } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

import { RELEASED_SKILLS_ROOT } from "./released-skills.mjs";
import {
  GIT_LOCATION_VARIABLES,
  companionSkillsRoot,
  diffTrees,
  releasedSkillPairs,
  runVendor,
} from "./vendor-skills.mjs";

const REPO_ROOT = join(dirname(fileURLToPath(import.meta.url)), "..");
const PREFIX = "sync-skills-companion";
const SKILLS_PATH = RELEASED_SKILLS_ROOT.join("/");
const SOURCE_LOCK = "source-lock.json";
const MAIN = "main";
const ORIGIN_MAIN = "refs/remotes/origin/main";
const SHORT_SHA_LENGTH = 12;
const OLD_VERSION_ARG = "<old version>";

/** Each companion file that holds the plugin version, with its count of version fields. */
const VERSION_FILES = [
  { path: "fallow/.codex-plugin/plugin.json", count: 1 },
  { path: "fallow/.claude-plugin/plugin.json", count: 1 },
  { path: ".claude-plugin/marketplace.json", count: 2 },
];

/**
 * The checks of the companion validate workflow that run locally without
 * network access. The workflow runs more steps, for example the plugin
 * validation, and its order differs. Each check runs in the companion
 * checkout with `FALLOW_SOURCE_DIR` set to a checkout of the pin. The first
 * failure stops the sync before the commit.
 */
export const COMPANION_CHECKS = [
  ["node", "scripts/check-source-contract.mjs"],
  ["python3", "scripts/plugin_release.py", "newer-than", OLD_VERSION_ARG],
  ["python3", "scripts/validate_skill_frontmatter.py", "."],
  ["python3", "-m", "unittest", "discover", "-s", "scripts", "-p", "test_*.py"],
  ["node", "--test", "scripts/*.test.mjs"],
];

const USAGE = `Usage:
  node scripts/sync-skills-companion.mjs [--pin <sha>] [--message <text>] [--dry-run]

Options:
  --pin <sha>       publish the skill of this commit on origin/main (default: HEAD)
  --message <text>  use this commit message in the companion
  --dry-run         do the checks, show the changes, and write nothing
  -h, --help        show this help`;

class UsageError extends Error {}

/** A failed precondition. The script stops and tells the maintainer what to do. */
class Refusal extends Error {}

const parseArgs = (argv) => {
  const options = { pin: undefined, message: undefined, dryRun: false, help: false };
  for (let index = 0; index < argv.length; index += 1) {
    const arg = argv[index];
    if (arg === "--pin" || arg === "--message") {
      const value = argv[index + 1];
      if (value === undefined || value.startsWith("-")) {
        throw new UsageError(`${arg} needs a value`);
      }
      options[arg.slice(2)] = value;
      index += 1;
    } else if (arg === "--dry-run") {
      options.dryRun = true;
    } else if (arg === "--help" || arg === "-h") {
      options.help = true;
    } else {
      throw new UsageError(`unknown argument: ${arg}`);
    }
  }
  return options;
};

const cleanEnv = (env) => {
  const result = { ...env };
  for (const name of GIT_LOCATION_VARIABLES) {
    delete result[name];
  }
  return result;
};

const makeGit = (env) => {
  const git = (cwd, ...args) =>
    execFileSync("git", args, {
      cwd,
      env,
      encoding: "utf8",
      stdio: ["ignore", "pipe", "pipe"],
    }).trim();
  git.succeeds = (cwd, ...args) =>
    spawnSync("git", args, { cwd, env, stdio: "ignore" }).status === 0;
  return git;
};

/**
 * Return the pull request numbers in commit subjects, in order, each once.
 *
 * @param {string[]} subjects
 * @returns {string[]}
 */
export const prNumbers = (subjects) => [
  ...new Set(subjects.flatMap((subject) => [...subject.matchAll(/\(#(\d+)\)/gu)].map((m) => m[1]))),
];

/**
 * @param {{ numbers: string[], pin: string }} input
 * @returns {string}
 */
export const commitMessage = ({ numbers, pin }) =>
  numbers.length > 0
    ? `docs: sync the Fallow skill from fallow (${numbers.map((n) => `#${n}`).join(", ")})`
    : `docs: sync the Fallow skill from fallow ${pin.slice(0, SHORT_SHA_LENGTH)}`;

const checkCompanion = (git, companion) => {
  if (!existsSync(join(companion, SOURCE_LOCK))) {
    throw new Refusal(
      `no companion checkout at ${companion}. Set FALLOW_SKILLS_DIR to the fallow-skills checkout.`,
    );
  }
  if (git(companion, "status", "--porcelain") !== "") {
    throw new Refusal(
      `${companion} has uncommitted changes. Commit or remove them, then run again.`,
    );
  }
  const branch = git.succeeds(companion, "symbolic-ref", "--quiet", "HEAD")
    ? git(companion, "symbolic-ref", "--short", "HEAD")
    : "";
  if (branch !== MAIN) {
    throw new Refusal(`${companion} is not on ${MAIN}. Run: git -C ${companion} switch ${MAIN}`);
  }
  git(companion, "fetch", "--quiet", "origin", MAIN);
  const head = git(companion, "rev-parse", "HEAD");
  const upstream = git(companion, "rev-parse", ORIGIN_MAIN);
  if (head === upstream) {
    return;
  }
  if (git.succeeds(companion, "merge-base", "--is-ancestor", head, upstream)) {
    throw new Refusal(
      `${companion} is behind origin/main. Run: git -C ${companion} pull --ff-only origin ${MAIN}`,
    );
  }
  throw new Refusal(
    `${companion} has commits that are not on origin/main. Push or remove them, then run again.`,
  );
};

const resolvePin = (git, repoRoot, requested) => {
  git(repoRoot, "fetch", "--quiet", "origin", MAIN);
  const ref = requested ?? "HEAD";
  if (!git.succeeds(repoRoot, "rev-parse", "--verify", "--quiet", `${ref}^{commit}`)) {
    throw new Refusal(`${ref} is not a commit in ${repoRoot}.`);
  }
  const pin = git(repoRoot, "rev-parse", "--verify", `${ref}^{commit}`);
  if (!git.succeeds(repoRoot, "merge-base", "--is-ancestor", pin, ORIGIN_MAIN)) {
    throw new Refusal(
      `${pin} is not on origin/main. The companion checks out the pin, so merge the change ` +
        "first, or name a commit on origin/main with --pin.",
    );
  }
  // A pin from --pin reads the committed tree, so local edits do not count.
  if (requested === undefined && git(repoRoot, "status", "--porcelain", "--", SKILLS_PATH) !== "") {
    throw new Refusal(
      `${SKILLS_PATH} has uncommitted changes. Commit or remove them, or name a commit with --pin.`,
    );
  }
  return pin;
};

/**
 * Check out the skills of the pin in a temp clone. The clone shares the
 * objects of the fallow repository, so it is fast and needs no network.
 */
const checkoutPin = (git, repoRoot, pin) => {
  const dir = mkdtempSync(join(tmpdir(), `${PREFIX}-`));
  try {
    git(dir, "clone", "--quiet", "--shared", "--no-checkout", repoRoot, dir);
    git(dir, "sparse-checkout", "set", SKILLS_PATH);
    git(dir, "-c", `core.hooksPath=${devNull}`, "checkout", "--quiet", "--detach", pin);
    return dir;
  } catch (caught) {
    rmSync(dir, { recursive: true, force: true });
    throw caught;
  }
};

const readVersion = (companion) => {
  const [codex, claude, marketplace] = VERSION_FILES.map(({ path }) =>
    JSON.parse(readFileSync(join(companion, path), "utf8")),
  );
  const versions = [
    codex.version,
    claude.version,
    marketplace.metadata?.version,
    marketplace.plugins?.[0]?.version,
  ];
  if (new Set(versions).size !== 1) {
    throw new Refusal(
      `the companion versions differ: ${versions.join(", ")}. Make them equal first.`,
    );
  }
  const match = /^(\d+)\.(\d+)\.(\d+)$/u.exec(versions[0] ?? "");
  if (match === null) {
    throw new Refusal(`the companion version ${String(versions[0])} is not MAJOR.MINOR.PATCH.`);
  }
  const [, major, minor, patch] = match;
  return { old: versions[0], next: `${major}.${minor}.${(Number(patch) + 1).toString()}` };
};

const escapeRegExp = (text) => text.replace(/[.*+?^${}()|[\]\\]/gu, "\\$&");

const valuePattern = (key, value) =>
  new RegExp(`("${key}"\\s*:\\s*)"${escapeRegExp(value)}"`, "gu");

/** Refuse unless `file` holds `expected` copies of the `key` value `value`. */
const expectValueCount = (file, key, value, expected) => {
  const count = readFileSync(file, "utf8").match(valuePattern(key, value))?.length ?? 0;
  if (count !== expected) {
    throw new Refusal(
      `expected ${expected.toString()} "${key}" value(s) "${value}" in ${file}, found ${count.toString()}`,
    );
  }
};

/** Replace one JSON string value in place, so the file keeps its format. */
const replaceValue = (file, key, oldValue, newValue, expected) => {
  expectValueCount(file, key, oldValue, expected);
  const content = readFileSync(file, "utf8");
  const pattern = valuePattern(key, oldValue);
  const updated = content.replace(pattern, `$1"${newValue}"`);
  JSON.parse(updated);
  writeFileSync(file, updated);
};

const subjectsBetween = (git, repoRoot, from, to) => {
  try {
    return git(repoRoot, "log", "--format=%s", `${from}..${to}`, "--", SKILLS_PATH)
      .split("\n")
      .filter(Boolean)
      .toReversed();
  } catch {
    // The old pin may be missing from this clone. The message then names the new pin.
    return [];
  }
};

const runChecks = (companion, source, oldVersion, env, log) => {
  for (const [command, ...args] of COMPANION_CHECKS) {
    const resolved = args.map((arg) => (arg === OLD_VERSION_ARG ? oldVersion : arg));
    const line = [command, ...resolved].join(" ");
    log(`${PREFIX}: check: ${line}`);
    const result = spawnSync(command === "node" ? process.execPath : command, resolved, {
      cwd: companion,
      env: { ...env, FALLOW_SOURCE_DIR: source },
      stdio: "inherit",
    });
    if (result.status !== 0) {
      throw new Refusal(
        `the companion check failed: ${line}. The sync changes stay staged in ${companion}. ` +
          `Fix the cause, or discard them with: git -C ${companion} reset --quiet ` +
          `&& git -C ${companion} checkout -- . && git -C ${companion} clean -fd -- fallow`,
      );
    }
  }
};

const syncCompanion = ({ options, repoRoot, env, skipChecks, log, git }) => {
  const companion = companionSkillsRoot({ env, repoRoot });
  checkCompanion(git, companion);
  const pin = resolvePin(git, repoRoot, options.pin);
  const source = checkoutPin(git, repoRoot, pin);
  try {
    const pairs = releasedSkillPairs(source, companion);
    const drift = pairs.map((pair) => ({ ...pair, ...diffTrees(pair.canonical, pair.published) }));
    const changes = drift.flatMap(({ name, missing, extra, changed }) => [
      ...missing.map((path) => `  ${name}: add ${path}`),
      ...changed.map((path) => `  ${name}: update ${path}`),
      ...extra.map((path) => `  ${name}: remove ${path}`),
    ]);
    if (changes.length === 0) {
      log(`${PREFIX}: nothing to sync. The companion matches ${pin}.`);
      return 0;
    }
    const version = readVersion(companion);
    const lockFile = join(companion, SOURCE_LOCK);
    const oldPin = JSON.parse(readFileSync(lockFile, "utf8")).commit;
    const message =
      options.message ??
      commitMessage({ numbers: prNumbers(subjectsBetween(git, repoRoot, oldPin, pin)), pin });
    log(`${PREFIX}: changes from ${pin}:`);
    for (const line of changes) {
      log(line);
    }
    log(`${PREFIX}: version ${version.old} -> ${version.next}`);
    log(`${PREFIX}: source-lock commit ${oldPin} -> ${pin}`);
    log(`${PREFIX}: commit message: ${message}`);
    if (options.dryRun) {
      log(`${PREFIX}: dry run, wrote nothing.`);
      return 0;
    }

    // Check every value to replace before the first write, so a refusal
    // leaves the companion clean.
    for (const { path, count } of VERSION_FILES) {
      expectValueCount(join(companion, path), "version", version.old, count);
    }
    expectValueCount(lockFile, "commit", oldPin, 1);

    for (const { canonical, published } of pairs) {
      runVendor(canonical, published);
    }
    for (const { path, count } of VERSION_FILES) {
      replaceValue(join(companion, path), "version", version.old, version.next, count);
    }
    replaceValue(lockFile, "commit", oldPin, pin, 1);

    // Stage before the checks: the private-data guard of the companion reads
    // `git ls-files`, so it does not see a new file that is still untracked.
    const paths = [
      ...pairs.map(({ published }) => relative(companion, published)),
      ...VERSION_FILES.map(({ path }) => path),
      SOURCE_LOCK,
    ];
    git(companion, "add", "-A", "--", ...paths);
    if (!skipChecks) {
      runChecks(companion, source, version.old, env, log);
    }
    git(companion, "commit", "-S", "--quiet", "-m", message);
    log(`${PREFIX}: committed ${git(companion, "rev-parse", "--short", "HEAD")} in ${companion}.`);
    log("Push it with:");
    log(`  git -C ${companion} push origin ${MAIN}`);
    log('Then run the failed "Public skills contract" job on fallow main again.');
    return 0;
  } finally {
    rmSync(source, { recursive: true, force: true });
  }
};

/**
 * Run the sync. Return the exit code: 0 on success or when nothing changed,
 * 1 on a refusal, 2 on a usage error.
 *
 * @param {{
 *   argv?: string[],
 *   repoRoot?: string,
 *   env?: Record<string, string | undefined>,
 *   skipChecks?: boolean,
 *   log?: (line: string) => void,
 *   error?: (line: string) => void,
 * }} [input] `skipChecks` exists for the tests of this script only.
 * @returns {number}
 */
export const run = ({
  argv = process.argv.slice(2),
  repoRoot = REPO_ROOT,
  env = process.env,
  skipChecks = false,
  log = console.log,
  error = console.error,
} = {}) => {
  let options;
  try {
    options = parseArgs(argv);
  } catch (caught) {
    if (caught instanceof UsageError) {
      error(`${PREFIX}: ${caught.message}\n${USAGE}`);
      return 2;
    }
    throw caught;
  }
  if (options.help) {
    log(USAGE);
    return 0;
  }
  const gitEnv = cleanEnv(env);
  try {
    return syncCompanion({ options, repoRoot, env: gitEnv, skipChecks, log, git: makeGit(gitEnv) });
  } catch (caught) {
    if (caught instanceof Refusal) {
      error(`${PREFIX}: ${caught.message}`);
      return 1;
    }
    throw caught;
  }
};

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  try {
    process.exitCode = run();
  } catch (caught) {
    console.error(`${PREFIX}: ${caught.message}`);
    process.exitCode = 2;
  }
}
