#!/usr/bin/env node
/**
 * Publish the public Fallow skill contract into the companion skills repo.
 *
 * Canonical source: `npm/fallow/skills/fallow/` in this repository.
 * Public consumer: `<fallow-skills>/fallow/skills/fallow/`.
 *
 * The public plugin strips unsupported `metadata` frontmatter from SKILL.md
 * and may add host interface files outside the source contract. References and
 * all remaining skill content stay byte-identical.
 *
 * Usage:
 *   node scripts/vendor-skills.mjs
 *   node scripts/vendor-skills.mjs --check
 *
 * `FALLOW_SKILLS_DIR` may point to the companion repository. Otherwise the
 * script uses `../fallow-skills`. A missing consumer is always an error so
 * cross-repository checks cannot pass by silently skipping.
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
import { dirname, join, relative } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const REPO_ROOT = join(dirname(fileURLToPath(import.meta.url)), "..");
const CANONICAL_TREE = join(REPO_ROOT, "npm", "fallow", "skills", "fallow");
const TARGET_SUBPATH = join("fallow", "skills", "fallow");

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

const sourceContent = (root, path) => {
  const content = readFileSync(join(root, path), "utf8");
  return path === "SKILL.md" ? stripUnsupportedMetadata(content) : content;
};

export const diffTrees = (canonical, published) => {
  const canonicalFiles = new Set(contractFiles(canonical));
  const publishedFiles = existsSync(published) ? new Set(contractFiles(published)) : new Set();
  const missing = [...canonicalFiles].filter((path) => !publishedFiles.has(path));
  const extra = [...publishedFiles].filter((path) => !canonicalFiles.has(path));
  const changed = [...canonicalFiles].filter(
    (path) =>
      publishedFiles.has(path) &&
      sourceContent(canonical, path) !== readFileSync(join(published, path), "utf8"),
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

const resolvePublished = () => {
  const root = process.env.FALLOW_SKILLS_DIR || join(REPO_ROOT, "..", "fallow-skills");
  const tree = join(root, TARGET_SUBPATH);
  return { tree, present: existsSync(join(tree, "SKILL.md")) };
};

export const decide = ({ present, check }) => {
  if (!present) {
    return { action: "error" };
  }
  return { action: check ? "check" : "vendor" };
};

export const main = (argv = process.argv.slice(2)) => {
  const check = argv.includes("--check");
  const { tree, present } = resolvePublished();
  const { action } = decide({ present, check });
  if (action === "error") {
    throw new Error(`published fallow-skills contract not found at ${tree}`);
  }
  return action === "check" ? runCheck(CANONICAL_TREE, tree) : runVendor(CANONICAL_TREE, tree);
};

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  try {
    process.exitCode = main();
  } catch (error) {
    console.error(`vendor-skills: ${error.message}`);
    process.exitCode = 2;
  }
}
