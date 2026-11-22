#!/usr/bin/env node
// `fallow_config::MergedTsconfig` is the one reader of tsconfig compiler
// options. It follows the `extends` chain with the merge rules of TypeScript.
// A direct read of the `compilerOptions` key sees only one file, so it misses
// a setting that the config inherits. This check fails on such a read outside
// the reader and outside test code.
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

import { runCliMain } from "./cli-main.mjs";
import { parseOptions } from "./lib/options.mjs";
import {
  allowedByMarker,
  inRegions,
  lineOf,
  productionRustSources,
  testRegions,
} from "./lib/rust-modules.mjs";
import { stripComments, stripLiterals } from "./lib/rust-source.mjs";

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const OWNER = "crates/config/src/workspace/tsconfig_merged.rs";
const SCAN_FOLDERS = ["crates", "fuzz/fuzz_targets"];
const ALLOW_MARKER = "tsconfig-reads: allow";
// The key as a Rust string literal: "compilerOptions", r"compilerOptions" or
// r#"compilerOptions"#.
const KEY_PATTERN = /(?<![\w"])(?:"compilerOptions"|r(#*)"compilerOptions"\1)/g;

/** True when offset `index` is inside a literal or a comment of `source`. */
const blankedAt = (source, code, index) =>
  index >= 0 && index < source.length && code[index] === " " && source[index] !== " ";

/**
 * True when `[start, end)` is one whole literal, and not a part of a larger
 * string such as a JSON fixture.
 */
const isWholeLiteral = (source, code, start, end) =>
  !blankedAt(source, code, start - 1) && !blankedAt(source, code, end);

/**
 * Returns each `compilerOptions` key read in `source` as `{ line, text }`.
 * The key inside a comment, inside a larger string such as a JSON fixture, or
 * inside a `#[cfg(test)]` item does not count. A line with a
 * `tsconfig-reads: allow` comment on it, or below a comment line with the
 * marker, does not count.
 */
export const findTsconfigReads = (source) => {
  const code = stripLiterals(source);
  const withoutComments = stripComments(source);
  const regions = testRegions(code);
  const lines = source.split("\n");
  const reads = [];
  for (const match of withoutComments.matchAll(KEY_PATTERN)) {
    const end = match.index + match[0].length;
    if (!isWholeLiteral(source, code, match.index, end) || inRegions(regions, match.index)) {
      continue;
    }
    const line = lineOf(source, match.index);
    if (!allowedByMarker(lines, line, ALLOW_MARKER)) reads.push({ line, text: match[0] });
  }
  return reads;
};

/**
 * Returns each `compilerOptions` key read under `<root>/crates` and
 * `<root>/fuzz/fuzz_targets` as `file:line: text`. Files in a `tests/`
 * folder, test-only modules and the reader are skipped.
 */
export const scanCrates = (root) =>
  [...productionRustSources(root, SCAN_FOLDERS)]
    .filter(([relative]) => relative !== OWNER)
    .flatMap(([relative, source]) =>
      findTsconfigReads(source).map(({ line, text }) => `${relative}:${line}: ${text}`),
    );

export const main = (args = process.argv.slice(2)) => {
  const { root } = parseOptions(
    args,
    { "--root": { key: "root", type: "string" } },
    { root: ROOT },
  );
  const findings = scanCrates(path.resolve(root));
  if (findings.length === 0) {
    console.log("tsconfig read check passed");
    return 0;
  }
  for (const finding of findings) console.error(`tsconfig-reads: ${finding}`);
  console.error(
    `Read tsconfig compiler options through fallow_config::MergedTsconfig (${OWNER}), which follows the extends chain.`,
  );
  return 1;
};

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  runCliMain(main);
}
