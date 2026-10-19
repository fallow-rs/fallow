#!/usr/bin/env node
/**
 * Write seed inputs for the extraction fuzz targets from the test fixtures.
 *
 * Each seed is one selector byte, then the source of a fixture file. The
 * selector picks the file name that `parse_from_content` sees, so the byte
 * comes from the list in `fuzz/fuzz_targets/common/extract_paths.rs`. The
 * script takes a fixed number of small files per selector in path order, so
 * a second run gives the same seeds.
 *
 * Run: `node scripts/fuzz-seed-extract.mjs`
 */

import { createHash } from "node:crypto";
import { mkdirSync, readFileSync, readdirSync, rmSync, statSync, writeFileSync } from "node:fs";
import { dirname, join, relative, sep } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = join(dirname(fileURLToPath(import.meta.url)), "..");
const TARGETS = ["fuzz_extract", "fuzz_extract_cache"];
const SEED_PREFIX = "fixture_";
/** Small seeds keep the corpus fast to load; libFuzzer grows them later. */
const MAX_SEED_BYTES = 3072;
const SEEDS_PER_SELECTOR = 8;
const SKIPPED_DIRS = new Set(["node_modules", ".git", "dist", "build"]);
const SPECIAL_SUFFIXES = [".d.ts", ".component.ts"];

/** Reads the selector file names in their order in the Rust list. */
export const readSelectorPaths = (source) =>
  [...source.matchAll(/^\s+"([^"]+)",$/gm)].map((match) => match[1]);

/** Returns the selector index for a fixture path, or -1 when none fits. */
export const selectorFor = (file, selectorPaths) => {
  const name = file.split("/").pop();
  const special = SPECIAL_SUFFIXES.find((suffix) => name.endsWith(suffix));
  if (special) return selectorPaths.findIndex((path) => path.endsWith(special));
  const dot = name.lastIndexOf(".");
  if (dot <= 0) return -1;
  const extension = name.slice(dot);
  return selectorPaths.findIndex(
    (path) => path.endsWith(extension) && !SPECIAL_SUFFIXES.some((suffix) => path.endsWith(suffix)),
  );
};

/** Lists the files below `dir` in path order, without the skipped directories. */
export const listFixtureFiles = (dir) =>
  readdirSync(dir, { recursive: true, withFileTypes: true })
    .filter((entry) => entry.isFile())
    .map((entry) => join(entry.parentPath, entry.name))
    .filter(
      (file) =>
        !relative(dir, file)
          .split(sep)
          .some((part) => SKIPPED_DIRS.has(part)),
    )
    .toSorted();

const fitsSeed = (file) => statSync(file).size <= MAX_SEED_BYTES;
const toPosix = (path) => path.split(sep).join("/");

/** Groups up to `SEEDS_PER_SELECTOR` small files by their selector index. */
export const pickFixtures = (fixturesDir, selectorPaths) => {
  const candidates = listFixtureFiles(fixturesDir)
    .filter(fitsSeed)
    .map((file) => [selectorFor(toPosix(relative(fixturesDir, file)), selectorPaths), file])
    .filter(([selector]) => selector >= 0);
  const picked = new Map();
  for (const [selector, file] of candidates) {
    const list = [...(picked.get(selector) ?? []), file];
    picked.set(selector, list.slice(0, SEEDS_PER_SELECTOR));
  }
  return picked;
};

/** Replaces the fixture seeds in `corpusDir` with the picked files. */
export const writeSeeds = (corpusDir, picked) => {
  mkdirSync(corpusDir, { recursive: true });
  readdirSync(corpusDir)
    .filter((name) => name.startsWith(SEED_PREFIX))
    .forEach((name) => rmSync(join(corpusDir, name)));
  const seeds = [...picked].flatMap(([selector, files]) =>
    files.map((file) => Buffer.concat([Buffer.from([selector]), readFileSync(file)])),
  );
  for (const seed of seeds) {
    const hash = createHash("sha256").update(seed).digest("hex").slice(0, 16);
    writeFileSync(join(corpusDir, `${SEED_PREFIX}${hash}.bin`), seed);
  }
  return seeds.length;
};

const main = () => {
  const pathsFile = join(ROOT, "fuzz", "fuzz_targets", "common", "extract_paths.rs");
  const selectorPaths = readSelectorPaths(readFileSync(pathsFile, "utf8"));
  const picked = pickFixtures(join(ROOT, "tests", "fixtures"), selectorPaths);
  for (const target of TARGETS) {
    const count = writeSeeds(join(ROOT, "fuzz", "corpus", target), picked);
    console.log(`wrote ${count} seeds for ${target}`);
  }
};

if (process.argv[1] === fileURLToPath(import.meta.url)) main();
