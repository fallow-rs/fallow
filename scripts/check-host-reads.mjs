#!/usr/bin/env node
// `fallow_process::HostPaths` is the one reader of the home folder, the
// per-user config and data folders, and the `PATH` search list. A direct read
// in production code makes a unit test that reaches it depend on the machine
// that runs it. This check fails on such a read outside the owner and outside
// test code. Clippy `disallowed-methods` cannot match on the argument of
// `std::env::var_os`, so a text check does it.
import { existsSync, readdirSync, readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

import { runCliMain } from "./cli-main.mjs";
import { parseOptions } from "./lib/options.mjs";
import { stripLiterals } from "./lib/rust-source.mjs";

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const OWNER = "crates/process/src/host_paths.rs";
const SCAN_FOLDERS = ["crates", "fuzz/fuzz_targets"];
const SKIPPED_FOLDERS = new Set(["target", "node_modules", "tests", "benches", "fixtures"]);
const CRATE_ROOTS = new Set(["lib.rs", "main.rs", "mod.rs", "build.rs"]);
const HOST_VARIABLES = "HOME|PATH|USERPROFILE|APPDATA|LOCALAPPDATA|XDG_[A-Z_]+";
// A string key: "HOME", r"HOME" or r#"HOME"#.
const HOST_KEY = String.raw`(?:"(?:${HOST_VARIABLES})"|r(#*)"(?:${HOST_VARIABLES})"\1)`;
const TURBOFISH = String.raw`(?:\s*::\s*<[^>()]*>)?`;
const ALLOW_MARKER = "host-reads: allow";
const ALIAS_PATTERN = /\b(?:var|var_os)\s+as\s+([A-Za-z_]\w*)/g;
const CFG_ATTRIBUTE = /#\[\s*cfg\s*\(([^\]]*)\)\s*\]/g;
const TEST_ITEM =
  /^\s*(?:#\[[^\]]*\]\s*)*(?:pub(?:\([^)]*\))?\s+)?(?:(?:async|const|unsafe)\s+)*(?:mod|fn|impl)\b/;
const MOD_DECLARATION = /((?:#\[[^\]]*\]\s*)*)(?:pub(?:\([^)]*\))?\s+)?mod\s+([A-Za-z_]\w*)\s*;/g;
const PATH_ATTRIBUTE = /#\[\s*path\s*=\s*"([^"]+)"\s*\]/;

const readPatterns = (aliases) => [
  new RegExp(String.raw`\bvar(?:_os)?${TURBOFISH}\s*\(\s*${HOST_KEY}\s*\)`, "g"),
  new RegExp(String.raw`\b(?:option_)?env!\s*\(\s*${HOST_KEY}\s*\)`, "g"),
  /\bvars_os\s*\(\s*\)|\benv\s*::\s*vars\s*\(\s*\)/g,
  /\bhome_dir\s*\(/g,
  ...aliases.map(
    (alias) => new RegExp(String.raw`\b${alias}${TURBOFISH}\s*\(\s*${HOST_KEY}\s*\)`, "g"),
  ),
];

const splitArguments = (text) => {
  const parts = [];
  let depth = 0;
  let start = 0;
  for (let index = 0; index < text.length; index += 1) {
    if (text[index] === "(") depth += 1;
    if (text[index] === ")") depth -= 1;
    if (text[index] === "," && depth === 0) {
      parts.push(text.slice(start, index));
      start = index + 1;
    }
  }
  parts.push(text.slice(start));
  return parts.map((part) => part.trim()).filter(Boolean);
};

const CALL = /^(all|any|not)\s*\(([\s\S]*)\)$/;
const FEATURE = /^feature\s*=/;

/**
 * Returns true when a `cfg(...)` predicate holds only in a test build, for
 * example `test`, `all(test, not(miri))` or `any(test, feature = "testing")`.
 * A `not(test)` item is production code.
 */
export const isTestPredicate = (predicate) => {
  const text = predicate.trim();
  if (text === "test") return true;
  const call = CALL.exec(text);
  if (!call) return false;
  const args = splitArguments(call[2]);
  if (call[1] === "all") return args.some(isTestPredicate);
  if (call[1] === "any") {
    return (
      args.some(isTestPredicate) && args.every((arg) => isTestPredicate(arg) || FEATURE.test(arg))
    );
  }
  return false;
};

const hasTestCfg = (attributes) =>
  [...attributes.matchAll(CFG_ATTRIBUTE)].some((attribute) => isTestPredicate(attribute[1]));

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

const inRegions = (regions, offset) =>
  regions.some(([start, end]) => offset >= start && offset < end);

const lineOf = (source, offset) => source.slice(0, offset).split("\n").length;

const allowed = (lines, line) => {
  const above = lines[line - 2]?.trim() ?? "";
  return (
    lines[line - 1].includes(ALLOW_MARKER) ||
    (above.startsWith("//") && above.includes(ALLOW_MARKER))
  );
};

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
      if (!allowed(lines, line)) reads.push({ line, text: match[0] });
    }
  }
  return reads.toSorted((left, right) => left.line - right.line);
};

/**
 * Returns the `mod name;` declarations of `source` as `{ name, path,
 * testOnly }`. `path` is the value of a `#[path = "..."]` attribute or null.
 * A declaration is test-only when its attributes have a test `cfg`, or when
 * it is inside a `#[cfg(test)]` item.
 */
export const moduleDeclarations = (source) => {
  const code = stripLiterals(source);
  const regions = testRegions(code);
  return [...code.matchAll(MOD_DECLARATION)].map((match) => {
    const attributes = source.slice(match.index, match.index + match[1].length);
    return {
      name: match[2],
      path: PATH_ATTRIBUTE.exec(attributes)?.[1] ?? null,
      testOnly: hasTestCfg(attributes) || inRegions(regions, match.index),
    };
  });
};

const isModuleRoot = (relative) =>
  CRATE_ROOTS.has(path.posix.basename(relative)) ||
  path.posix.basename(path.posix.dirname(relative)) === "bin" ||
  relative.startsWith("fuzz/fuzz_targets/");

/** Returns the files that a declaration in `relative` can name. */
const childCandidates = (relative, declaration) => {
  const folder = path.posix.dirname(relative);
  if (declaration.path) return [path.posix.normalize(`${folder}/${declaration.path}`)];
  const base = isModuleRoot(relative)
    ? folder
    : `${folder}/${path.posix.basename(relative, ".rs")}`;
  return [`${base}/${declaration.name}.rs`, `${base}/${declaration.name}/mod.rs`];
};

/**
 * Returns the set of files that only a test build compiles: each file that a
 * parent declares under a test `cfg`, and each file below such a file.
 */
export const testOnlyFiles = (sources) => {
  const children = childModules(sources);
  const queue = [...children.values()]
    .flat()
    .filter((entry) => entry.testOnly)
    .map((entry) => entry.child);
  return reachableFiles(children, queue);
};

/** Maps each file to the files that its `mod name;` declarations name. */
const childModules = (sources) => {
  const children = new Map();
  for (const [relative, source] of sources) {
    const entries = moduleDeclarations(source).flatMap((declaration) =>
      childCandidates(relative, declaration)
        .filter((child) => sources.has(child))
        .map((child) => ({ child, testOnly: declaration.testOnly })),
    );
    if (entries.length > 0) children.set(relative, entries);
  }
  return children;
};

/** Returns the files in `queue` and every file below them in `children`. */
const reachableFiles = (children, queue) => {
  const reached = new Set();
  while (queue.length > 0) {
    const file = queue.pop();
    if (reached.has(file)) continue;
    reached.add(file);
    for (const entry of children.get(file) ?? []) queue.push(entry.child);
  }
  return reached;
};

const rustFiles = (root, folder) => {
  if (!existsSync(path.join(root, folder))) return [];
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

/**
 * Returns each host read under `<root>/crates` and `<root>/fuzz/fuzz_targets`
 * as `file:line: text`. Files in a `tests/` folder, test-only modules and the
 * owner are skipped.
 */
export const scanCrates = (root) => {
  const sources = new Map(
    SCAN_FOLDERS.flatMap((folder) => rustFiles(root, folder)).map((relative) => [
      relative,
      readFileSync(path.join(root, relative), "utf8"),
    ]),
  );
  const testOnly = testOnlyFiles(sources);
  return [...sources]
    .filter(([relative]) => relative !== OWNER && !testOnly.has(relative))
    .flatMap(([relative, source]) =>
      findHostReads(source).map(({ line, text }) => `${relative}:${line}: ${text}`),
    );
};

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
