#!/usr/bin/env node
// `fallow_process::HostPaths` is the one reader of the home folder, the
// per-user config and data folders, and the `PATH` search list. A direct read
// in production code makes a unit test that reaches it depend on the machine
// that runs it. This check fails on such a read outside the owner and outside
// test code. Clippy `disallowed-methods` cannot match on the argument of
// `std::env::var_os`, so a text check does it.
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
import { stripLiterals } from "./lib/rust-source.mjs";

export { isTestPredicate, moduleDeclarations, testOnlyFiles } from "./lib/rust-modules.mjs";

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const OWNER = "crates/process/src/host_paths.rs";
const SCAN_FOLDERS = ["crates", "fuzz/fuzz_targets"];
const HOST_VARIABLES = "HOME|PATH|USERPROFILE|APPDATA|LOCALAPPDATA|XDG_[A-Z_]+";
// A string key: "HOME", r"HOME" or r#"HOME"#.
const HOST_KEY = String.raw`(?:"(?:${HOST_VARIABLES})"|r(#*)"(?:${HOST_VARIABLES})"\1)`;
const TURBOFISH = String.raw`(?:\s*::\s*<[^>()]*>)?`;
const ALLOW_MARKER = "host-reads: allow";
const ALIAS_PATTERN = /\b(?:var|var_os)\s+as\s+([A-Za-z_]\w*)/g;

const readPatterns = (aliases) => [
  new RegExp(String.raw`\bvar(?:_os)?${TURBOFISH}\s*\(\s*${HOST_KEY}\s*\)`, "g"),
  new RegExp(String.raw`\b(?:option_)?env!\s*\(\s*${HOST_KEY}\s*\)`, "g"),
  /\bvars_os\s*\(\s*\)|\benv\s*::\s*vars\s*\(\s*\)/g,
  /\bhome_dir\s*\(/g,
  ...aliases.map(
    (alias) => new RegExp(String.raw`\b${alias}${TURBOFISH}\s*\(\s*${HOST_KEY}\s*\)`, "g"),
  ),
];

/**
 * Returns each host read in `source` as `{ line, text }`. A read inside a
 * comment, a string or a `#[cfg(test)]` item does not count. A line with a
 * `host-reads: allow` comment on it, or below a comment line with the
 * marker, does not count.
 */
export const findHostReads = (source) => {
  const code = stripLiterals(source);
  const regions = testRegions(code);
  const lines = source.split("\n");
  const aliases = [...code.matchAll(ALIAS_PATTERN)].map((match) => match[1]);
  const reads = [];
  for (const pattern of readPatterns(aliases)) {
    for (const match of source.matchAll(pattern)) {
      if (code[match.index] === " " || inRegions(regions, match.index)) continue;
      const line = lineOf(source, match.index);
      if (!allowedByMarker(lines, line, ALLOW_MARKER)) reads.push({ line, text: match[0] });
    }
  }
  return reads.toSorted((left, right) => left.line - right.line);
};

/**
 * Returns each host read under `<root>/crates` and `<root>/fuzz/fuzz_targets`
 * as `file:line: text`. Files in a `tests/` folder, test-only modules and the
 * owner are skipped.
 */
export const scanCrates = (root) =>
  [...productionRustSources(root, SCAN_FOLDERS)]
    .filter(([relative]) => relative !== OWNER)
    .flatMap(([relative, source]) =>
      findHostReads(source).map(({ line, text }) => `${relative}:${line}: ${text}`),
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
