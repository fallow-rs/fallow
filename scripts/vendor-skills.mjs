#!/usr/bin/env node
/**
 * Publish the public Fallow skill contract into the companion skills repo.
 *
 * Canonical source: each released skill `npm/fallow/skills/<name>/` in this
 * repository.
 * Public consumer: `<fallow-skills>/fallow/skills/<name>/`.
 *
 * The public plugin strips unsupported `metadata` frontmatter from SKILL.md,
 * pins each download-and-run form of the fallow package (`npx fallow`) to the
 * version of `npm/fallow/package.json`, and may add host interface files
 * outside the source contract. All remaining skill content stays byte-identical.
 *
 * Usage:
 *   node scripts/vendor-skills.mjs
 *   node scripts/vendor-skills.mjs --check
 *   node scripts/vendor-skills.mjs --help
 *
 * `FALLOW_SKILLS_DIR` may point to the companion repository. Otherwise the
 * script uses `fallow-skills` next to the main working tree of this
 * repository, also from a linked worktree. A missing consumer is always an
 * error so cross-repository checks cannot pass by silently skipping.
 */

import { execFileSync } from "node:child_process";
import {
  existsSync,
  lstatSync,
  mkdirSync,
  readFileSync,
  readdirSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { basename, dirname, join, relative } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

import { RELEASED_SKILLS_ROOT, releasedSkillNames } from "./released-skills.mjs";

const REPO_ROOT = join(dirname(fileURLToPath(import.meta.url)), "..");
const TARGET_SKILLS_ROOT = join("fallow", "skills");
/** The skill whose published tree proves that the companion checkout exists. */
const ANCHOR_SKILL = "fallow";

/**
 * Pair each released skill with its published tree in the companion checkout.
 *
 * @param {string} repoRoot
 * @param {string} companionRoot
 * @returns {{ name: string, canonical: string, published: string }[]}
 */
export const releasedSkillPairs = (repoRoot, companionRoot) =>
  releasedSkillNames(repoRoot).map((name) => ({
    name,
    canonical: join(repoRoot, ...RELEASED_SKILLS_ROOT, name),
    published: join(companionRoot, TARGET_SKILLS_ROOT, name),
  }));

export const listFiles = (dir, base = dir) => {
  const files = [];
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    if (entry.name.startsWith(".")) {
      continue;
    }
    const absolutePath = join(dir, entry.name);
    const metadata = lstatSync(absolutePath);
    if (metadata.isSymbolicLink()) {
      throw new Error(`skill contract cannot contain symlinks: ${absolutePath}`);
    }
    if (metadata.isDirectory()) {
      files.push(...listFiles(absolutePath, base));
    } else if (metadata.isFile()) {
      files.push(relative(base, absolutePath).split("\\").join("/"));
    }
  }
  return files.toSorted();
};

const contractFiles = (dir) =>
  listFiles(dir).filter((path) => path === "SKILL.md" || path.startsWith("references/"));

export const stripUnsupportedMetadata = (content) => {
  const lines = content.split("\n");
  if (lines[0] !== "---") {
    throw new Error("SKILL.md source is missing YAML frontmatter");
  }
  const closing = lines.indexOf("---", 1);
  if (closing === -1) {
    throw new Error("SKILL.md source has unterminated YAML frontmatter");
  }
  const metadata = lines.findIndex((line, index) => index < closing && line === "metadata:");
  if (metadata === -1) {
    return content;
  }
  let end = metadata + 1;
  while (end < closing && (lines[end].startsWith(" ") || lines[end].trim() === "")) {
    end += 1;
  }
  lines.splice(metadata, end - metadata);
  return lines.join("\n");
};

/**
 * A download-and-run form of the fallow package without a version. `--no` and
 * `--no-install` forms run an installed copy and download nothing.
 */
const LAUNCHER = /\b(npx(?: --yes| -y)?|pnpm dlx|bunx|yarn dlx) fallow(?![@\w./-])/gu;
/** The same form with an exact version, as the published copy writes it. */
const PINNED_LAUNCHER = /\b(npx(?: --yes| -y)?|pnpm dlx|bunx|yarn dlx) fallow@\d+\.\d+\.\d+(?![\w.-])/gu;
const EXACT_VERSION = /^\d+\.\d+\.\d+$/u;

/**
 * Pins each download-and-run form of the fallow package to `version`. The
 * Claude plugin directory refuses a plugin whose skill text runs an unpinned
 * package, because the downloaded code can change after review.
 */
export const pinLaunchers = (content, version) => {
  if (!EXACT_VERSION.test(version)) {
    throw new Error(`launcher pin needs an exact fallow version, got ${version}`);
  }
  return content.replace(LAUNCHER, `$1 fallow@${version}`);
};

/**
 * The text with each pinned launcher written without its version. The check
 * compares this form, so a release that raises the package version does not
 * make the published copy drift. The sync then writes the new version.
 */
const unpinLaunchers = (content) => content.replace(PINNED_LAUNCHER, "$1 fallow");

const hasUnpinnedLauncher = (content) => {
  LAUNCHER.lastIndex = 0;
  return LAUNCHER.test(content);
};

/** The version of the npm package that ships the skill in `root` (`npm/fallow/skills/<name>`). */
const packageVersion = (root) => {
  const manifest = join(root, "..", "..", "package.json");
  if (!existsSync(manifest)) {
    throw new Error(`skill text runs fallow through a launcher, but ${manifest} is missing`);
  }
  return JSON.parse(readFileSync(manifest, "utf8")).version;
};

/** The skill file as the published copy writes it. */
const sourceContent = (root, path) => {
  const content = readFileSync(join(root, path), "utf8");
  const stripped = path === "SKILL.md" ? stripUnsupportedMetadata(content) : content;
  return hasUnpinnedLauncher(stripped) ? pinLaunchers(stripped, packageVersion(root)) : stripped;
};

/** True when `published` is the source file with every launcher pinned to some exact version. */
const matchesSource = (root, path, published) => {
  const content = readFileSync(join(root, path), "utf8");
  const stripped = path === "SKILL.md" ? stripUnsupportedMetadata(content) : content;
  return !hasUnpinnedLauncher(published) && unpinLaunchers(published) === unpinLaunchers(stripped);
};

export const diffTrees = (canonical, published) => {
  const canonicalFiles = new Set(contractFiles(canonical));
  const publishedFiles = existsSync(published) ? new Set(contractFiles(published)) : new Set();
  const missing = [...canonicalFiles].filter((path) => !publishedFiles.has(path));
  const extra = [...publishedFiles].filter((path) => !canonicalFiles.has(path));
  const changed = [...canonicalFiles].filter(
    (path) =>
      publishedFiles.has(path) &&
      !matchesSource(canonical, path, readFileSync(join(published, path), "utf8")),
  );
  return { missing, extra, changed };
};

const showDiff = (canonical, published, path) => {
  try {
    execFileSync(
      "git",
      [
        "--no-pager",
        "diff",
        "--no-index",
        "--unified=1",
        "--",
        join(published, path),
        join(canonical, path),
      ],
      { stdio: "inherit" },
    );
  } catch {
    // A content difference makes git exit 1. The comparison above is decisive.
  }
};

export const runCheck = (canonical, published, { renderDiffs = true } = {}) => {
  const drift = diffTrees(canonical, published);
  if (Object.values(drift).every((paths) => paths.length === 0)) {
    console.log("vendor-skills: published public skill matches the Fallow source contract");
    return 0;
  }
  console.error("vendor-skills: public skill contract drift\n");
  for (const path of drift.missing) {
    console.error(`  missing from published skill: ${path}`);
  }
  for (const path of drift.extra) {
    console.error(`  stale published contract file: ${path}`);
  }
  for (const path of drift.changed) {
    console.error(`  differs: ${path}`);
  }
  console.error("\nSynchronize with: node scripts/vendor-skills.mjs\n");
  if (renderDiffs) {
    for (const path of drift.changed) {
      showDiff(canonical, published, path);
    }
  }
  return 1;
};

export const runVendor = (canonical, published) => {
  const drift = diffTrees(canonical, published);
  for (const path of [...drift.missing, ...drift.changed]) {
    const destination = join(published, path);
    mkdirSync(dirname(destination), { recursive: true });
    writeFileSync(destination, sourceContent(canonical, path));
  }
  for (const path of drift.extra) {
    rmSync(join(published, path));
  }
  const touched = drift.missing.length + drift.changed.length + drift.extra.length;
  console.log(
    touched === 0
      ? "vendor-skills: public skill already matches the source contract"
      : `vendor-skills: synchronized ${touched.toString()} public contract file(s)`,
  );
  return 0;
};

// These variables point git at one checkout. The lookup passes its own
// directory, so an inherited value from a hook must not win.
export const GIT_LOCATION_VARIABLES = [
  "GIT_DIR",
  "GIT_WORK_TREE",
  "GIT_COMMON_DIR",
  "GIT_INDEX_FILE",
];

const mainWorkingTree = (repoRoot, env) => {
  const gitEnv = { ...process.env, ...env };
  for (const name of GIT_LOCATION_VARIABLES) {
    delete gitEnv[name];
  }
  try {
    const commonDir = execFileSync(
      "git",
      ["rev-parse", "--path-format=absolute", "--git-common-dir"],
      { cwd: repoRoot, env: gitEnv, encoding: "utf8", stdio: ["ignore", "pipe", "ignore"] },
    ).trim();
    // A linked worktree shares the `.git` directory of the main working tree.
    return basename(commonDir) === ".git" ? dirname(commonDir) : repoRoot;
  } catch {
    return repoRoot;
  }
};

/**
 * Return the companion `fallow-skills` checkout. `FALLOW_SKILLS_DIR` wins.
 * Otherwise the companion sits next to the main working tree, so a linked
 * worktree finds the same checkout as the main one.
 *
 * @param {{ env?: Record<string, string | undefined>, repoRoot?: string }} [options]
 * @returns {string}
 */
export const companionSkillsRoot = ({ env = process.env, repoRoot = REPO_ROOT } = {}) =>
  env.FALLOW_SKILLS_DIR || join(dirname(mainWorkingTree(repoRoot, env)), "fallow-skills");

const resolvePublished = () => {
  const root = companionSkillsRoot();
  const tree = join(root, TARGET_SKILLS_ROOT, ANCHOR_SKILL);
  return { root, tree, present: existsSync(join(tree, "SKILL.md")) };
};

export const decide = ({ present, check }) => {
  if (!present) {
    return { action: "error" };
  }
  return { action: check ? "check" : "vendor" };
};

const USAGE = `Usage:
  node scripts/vendor-skills.mjs          copy the skill contract into fallow-skills
  node scripts/vendor-skills.mjs --check  compare the two trees and write nothing`;

const KNOWN_ARGUMENTS = new Set(["--check", "--help", "-h"]);

export const main = (argv = process.argv.slice(2)) => {
  // An unknown argument must not fall through to a write into the companion repo.
  const unknown = argv.find((arg) => !KNOWN_ARGUMENTS.has(arg));
  if (unknown !== undefined) {
    throw new Error(`unknown argument: ${unknown}\n${USAGE}`);
  }
  if (argv.includes("--help") || argv.includes("-h")) {
    console.log(USAGE);
    return 0;
  }
  const check = argv.includes("--check");
  const { root, tree, present } = resolvePublished();
  const { action } = decide({ present, check });
  if (action === "error") {
    throw new Error(`published fallow-skills contract not found at ${tree}`);
  }
  const run = action === "check" ? runCheck : runVendor;
  // Run every skill, so one call reports all drift instead of the first one.
  const codes = releasedSkillPairs(REPO_ROOT, root).map(({ name, canonical, published }) => {
    console.log(`vendor-skills: ${name}`);
    return run(canonical, published);
  });
  return Math.max(0, ...codes);
};

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  try {
    process.exitCode = main();
  } catch (error) {
    console.error(`vendor-skills: ${error.message}`);
    process.exitCode = 2;
  }
}
