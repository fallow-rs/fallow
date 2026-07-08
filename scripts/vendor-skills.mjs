#!/usr/bin/env node
/**
 * Keep the vendored agent skill tree in sync with its canonical source.
 *
 * Source of truth: the standalone `fallow-rs/fallow-skills` repo
 * (`<fallow-skills>/fallow/skills/fallow/`). It is also the published Claude
 * plugin. Hand-edit there.
 *
 * Vendored copy: `npm/fallow/skills/fallow/` in THIS repo. It is committed and
 * shipped verbatim inside the npm package (`npm/fallow/package.json` lists
 * `skills` in `files`), so `npm install fallow` bundles the skill next to the
 * binary. It must be a mechanical mirror of the canonical tree, never
 * hand-edited independently.
 *
 * Two modes:
 *   node scripts/vendor-skills.mjs           re-vendor: copy canonical -> vendored
 *   node scripts/vendor-skills.mjs --check    drift gate: exit 1 if they differ
 *
 * Canonical location resolves from `FALLOW_SKILLS_DIR` (the fallow-skills repo
 * root), else `../fallow-skills` next to this repo (same convention as
 * scripts/check_telemetry_doc_sync.py). When the default path is absent (a
 * contributor without the sibling repo) `--check` skips with a warning and
 * exits 0; an explicitly-set `FALLOW_SKILLS_DIR` that is missing is a hard
 * error. Zero dependencies; Node >= 18.
 */

import { execFileSync } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, readdirSync, rmSync, writeFileSync } from "node:fs";
import { dirname, join, relative } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const REPO_ROOT = join(dirname(fileURLToPath(import.meta.url)), "..");
const VENDORED_TREE = join(REPO_ROOT, "npm", "fallow", "skills", "fallow");
const SKILL_SUBPATH = join("fallow", "skills", "fallow");

/** Recursively list files under `dir` as base-relative POSIX paths, sorted.
 * Dotfiles (e.g. `.DS_Store`) are ignored so incidental local cruft on one
 * side never trips the gate. */
export const listFiles = (dir, base = dir) => {
  const out = [];
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    if (entry.name.startsWith(".")) {
      continue;
    }
    const full = join(dir, entry.name);
    if (entry.isDirectory()) {
      out.push(...listFiles(full, base));
    } else if (entry.isFile()) {
      out.push(relative(base, full).split("\\").join("/"));
    }
  }
  return out.toSorted();
};

const bytes = (root, relPath) => readFileSync(join(root, relPath));

/** `"version": "x.y.z"` strings (in example JSON inside the reference docs) track
 * the fallow release version and are bumped on a staggered cadence at release
 * time: `/fallow-release` step 5c bumps the vendored copy inline while canonical
 * catches up later (step 10a-pre). The drift gate ignores those lines so it does
 * not false-fail on that transient window; content drift (plugin counts,
 * descriptions, commands, flags) is unaffected. The release's own step-10c
 * byte-identical `diff -r` stays the authoritative version-sync check. */
const VERSION_STRING = /"version":\s*"[^"]*"/g;
const normalize = (buf) => buf.toString("utf8").replace(VERSION_STRING, '"version": "<v>"');

/** Compare the two trees. Returns { missing, extra, changed } relative-path
 * lists: `missing` exists in canonical but not vendored, `extra` exists in
 * vendored but not canonical, `changed` exists in both with differing content
 * (byte-for-byte, except `"version"` strings; see `normalize`). */
export const diffTrees = (canonical, vendored) => {
  const canonicalFiles = new Set(listFiles(canonical));
  const vendoredFiles = existsSync(vendored) ? new Set(listFiles(vendored)) : new Set();
  const missing = [...canonicalFiles].filter((f) => !vendoredFiles.has(f));
  const extra = [...vendoredFiles].filter((f) => !canonicalFiles.has(f));
  const changed = [...canonicalFiles].filter(
    (f) => vendoredFiles.has(f) && normalize(bytes(canonical, f)) !== normalize(bytes(vendored, f)),
  );
  return { missing, extra, changed };
};

/** Best-effort unified diff for a changed file; git is present in CI and dev
 * shells, but the gate never depends on it (byte comparison already decided). */
const showDiff = (canonical, vendored, relPath) => {
  try {
    execFileSync(
      "git",
      [
        "--no-pager",
        "diff",
        "--no-index",
        "--unified=1",
        "--",
        join(vendored, relPath),
        join(canonical, relPath),
      ],
      { stdio: "inherit" },
    );
  } catch {
    // git exits 1 when files differ (expected) or is unavailable; ignore.
  }
};

const resolveCanonical = () => {
  const explicit = process.env.FALLOW_SKILLS_DIR || "";
  const root = explicit || join(REPO_ROOT, "..", "fallow-skills");
  const tree = join(root, SKILL_SUBPATH);
  return { tree, present: existsSync(join(tree, "SKILL.md")), explicit: Boolean(explicit) };
};

export const runCheck = (canonical, vendored = VENDORED_TREE) => {
  const { missing, extra, changed } = diffTrees(canonical, vendored);
  if (missing.length === 0 && extra.length === 0 && changed.length === 0) {
    console.log("vendor-skills: npm/fallow/skills is in sync with canonical fallow-skills");
    return 0;
  }
  console.error("vendor-skills: DRIFT between npm/fallow/skills and canonical fallow-skills\n");
  for (const f of missing) {
    console.error(`  missing from vendored (present in canonical): ${f}`);
  }
  for (const f of extra) {
    console.error(`  extra in vendored (absent from canonical):    ${f}`);
  }
  for (const f of changed) {
    console.error(`  differs: ${f}`);
  }
  console.error("\nRe-vendor with: node scripts/vendor-skills.mjs\n");
  for (const f of changed) {
    showDiff(canonical, vendored, f);
  }
  return 1;
};

export const runVendor = (canonical, vendored = VENDORED_TREE) => {
  const { missing, extra, changed } = diffTrees(canonical, vendored);
  for (const relPath of [...missing, ...changed]) {
    const dest = join(vendored, relPath);
    mkdirSync(dirname(dest), { recursive: true });
    writeFileSync(dest, bytes(canonical, relPath));
  }
  for (const relPath of extra) {
    rmSync(join(vendored, relPath));
  }
  const touched = missing.length + changed.length + extra.length;
  console.log(
    touched === 0
      ? "vendor-skills: already in sync; nothing to copy"
      : `vendor-skills: re-vendored ${touched} file(s) (${missing.length} added, ${changed.length} updated, ${extra.length} removed)`,
  );
  return 0;
};

const main = (argv = process.argv.slice(2)) => {
  const check = argv.includes("--check");
  const { tree, present, explicit } = resolveCanonical();
  if (!present) {
    const message = `canonical fallow-skills not found at ${tree}`;
    if (check && !explicit) {
      console.warn(`vendor-skills: ${message}; skipping drift check`);
      return 0;
    }
    throw new Error(`${message}${explicit ? " (FALLOW_SKILLS_DIR is set)" : ""}`);
  }
  return check ? runCheck(tree) : runVendor(tree);
};

// Only run when executed directly (not when imported by the test), so importing
// never triggers a re-vendor or a "canonical not found" throw.
if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  try {
    process.exitCode = main();
  } catch (error) {
    console.error(`vendor-skills: ${error.message}`);
    process.exitCode = 2;
  }
}
