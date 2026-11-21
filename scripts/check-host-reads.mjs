#!/usr/bin/env node
// `fallow_process::HostPaths` is the one reader of the home folder, the
// per-user config and data folders, and the `PATH` search list. A direct read
// in production code makes a unit test that reaches it depend on the machine
// that runs it. This check fails on such a read outside the owner and outside
// test code. Clippy `disallowed-methods` cannot match on the argument of
// `std::env::var_os`, so a text check does it.
import { readdirSync, readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

import { runCliMain } from "./cli-main.mjs";
import { parseOptions } from "./lib/options.mjs";
import { stripLiterals } from "./lib/rust-source.mjs";

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const OWNER = "crates/process/src/host_paths.rs";
const HOST_VARIABLES = "HOME|PATH|USERPROFILE|APPDATA|LOCALAPPDATA|XDG_[A-Z_]+";
const READ_PATTERNS = [
  new RegExp(String.raw`\bvar(?:_os)?\s*\(\s*"(?:${HOST_VARIABLES})"\s*\)`, "g"),
  /\bhome_dir\s*\(/g,
];
const SKIPPED_FOLDERS = new Set(["target", "node_modules", "tests", "benches", "fixtures"]);
const TEST_FILE = /(?:^|[_/])tests?\.rs$|\/test_support(?:\/|\.rs$)/;
const CFG_ATTRIBUTE = /#\[\s*cfg\s*\(([^\]]*)\)\s*\]/g;
const TEST_ITEM =
  /^\s*(?:#\[[^\]]*\]\s*)*(?:pub(?:\([^)]*\))?\s+)?(?:(?:async|const|unsafe)\s+)*(?:mod|fn|impl)\b/;

/**
 * Returns true when a `cfg(...)` predicate names `test` and does not negate
 * anything, for example `test`, `all(unix, test)` or `any(test, feature =
 * "testing")`. A `not(test)` item is production code.
 */
const isTestPredicate = (predicate) => /\btest\b/.test(predicate) && !/\bnot\s*\(/.test(predicate);

/** Returns true when `relative` is a test-only file or the owner. */
export const isExempt = (relative) => relative === OWNER || TEST_FILE.test(relative);

const closingBrace = (code, open) => {
  let depth = 0;
  for (let index = open; index < code.length; index += 1) {
    if (code[index] === "{") depth += 1;
    if (code[index] === "}") {
      depth -= 1;
      if (depth === 0) return index + 1;
    }
  }
  return code.length;
};

/**
 * Returns the `[start, end)` offsets of each `#[cfg(test)]` module, function
 * or impl block in `code`, a source with comments and literals blanked.
 */
const testRegions = (code) => {
  const regions = [];
  for (const attribute of code.matchAll(CFG_ATTRIBUTE)) {
    if (!isTestPredicate(attribute[1])) continue;
    const after = attribute.index + attribute[0].length;
    if (!TEST_ITEM.test(code.slice(after, after + 300))) continue;
    const open = code.indexOf("{", after);
    const semicolon = code.indexOf(";", after);
    if (open === -1 || (semicolon !== -1 && semicolon < open)) continue;
    regions.push([attribute.index, closingBrace(code, open)]);
  }
  return regions;
};

const lineOf = (source, offset) => source.slice(0, offset).split("\n").length;

/**
 * Returns each host read in `source` as `{ line, text }`. A read inside a
 * comment, a string or a `#[cfg(test)]` item does not count.
 */
export const findHostReads = (source) => {
  const code = stripLiterals(source);
  const regions = testRegions(code);
  const inTest = (offset) => regions.some(([start, end]) => offset >= start && offset < end);
  const reads = [];
  for (const pattern of READ_PATTERNS) {
    for (const match of source.matchAll(pattern)) {
      if (code[match.index] === " " || inTest(match.index)) continue;
      reads.push({ line: lineOf(source, match.index), text: match[0] });
    }
  }
  return reads.toSorted((left, right) => left.line - right.line);
};

const rustFiles = (root, folder) => {
  const files = [];
  for (const entry of readdirSync(path.join(root, folder), { withFileTypes: true })) {
    const relative = `${folder}/${entry.name}`;
    if (entry.isDirectory()) {
      if (!SKIPPED_FOLDERS.has(entry.name)) files.push(...rustFiles(root, relative));
    } else if (entry.name.endsWith(".rs")) {
      files.push(relative);
    }
  }
  return files;
};

/** Returns each host read under `<root>/crates` as `file:line: text`. */
export const scanCrates = (root) =>
  rustFiles(root, "crates")
    .filter((relative) => !isExempt(relative))
    .flatMap((relative) =>
      findHostReads(readFileSync(path.join(root, relative), "utf8")).map(
        ({ line, text }) => `${relative}:${line}: ${text}`,
      ),
    );

export const main = (args = process.argv.slice(2)) => {
  const { root } = parseOptions(
    args,
    { "--root": { key: "root", type: "string" } },
    { root: ROOT },
  );
  const findings = scanCrates(path.resolve(root));
  if (findings.length === 0) {
    console.log("host read check passed");
    return 0;
  }
  for (const finding of findings) console.error(`host-reads: ${finding}`);
  console.error(
    `Read the host paths through fallow_process::HostPaths (${OWNER}), and pass them to the code that needs them.`,
  );
  return 1;
};

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  runCliMain(main);
}
