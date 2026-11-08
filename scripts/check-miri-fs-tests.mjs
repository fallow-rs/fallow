#!/usr/bin/env node
// Miri isolation blocks file system calls, so a unit test that makes a
// temporary directory or reads a file fails under Miri. The Miri CI job is
// path-filtered and slow, so this check finds such a test without a
// `cfg_attr(miri, ignore ...)` mark before the change reaches CI. A file
// system call inside a helper that a test calls needs a call graph, so this
// text check does not find it.
import { existsSync, readdirSync, readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

import { runCliMain } from "./cli-main.mjs";

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const CI_WORKFLOW = ".github/workflows/ci.yml";
const MIRI_PACKAGE_PATTERN = /\bmiri test -p ([\w-]+)/g;
const TEMPDIR_ATTRIBUTE = '#[cfg_attr(miri, ignore = "tempdir is blocked by Miri isolation")]';
const FS_ATTRIBUTE = '#[cfg_attr(miri, ignore = "the filesystem is blocked by Miri isolation")]';

/**
 * The calls that Miri isolation blocks, in report priority order, with the
 * attribute that a test needs when its body contains one.
 */
const FS_PATTERNS = [
  { pattern: "tempdir(", regex: /\btempdir\s*\(/, attribute: TEMPDIR_ATTRIBUTE },
  { pattern: "tempfile::", regex: /\btempfile\s*::/, attribute: TEMPDIR_ATTRIBUTE },
  { pattern: "TempDir", regex: /\bTempDir\b/, attribute: TEMPDIR_ATTRIBUTE },
  { pattern: "std::fs::", regex: /\bstd\s*::\s*fs\s*::/, attribute: FS_ATTRIBUTE },
];
const MOD_ITEM = /^mod\s+([A-Za-z_]\w*)\s*([;{])/;
const FN_ITEM = /^fn\s+([A-Za-z_]\w*)/;
const TEST_ATTRIBUTE = /^#\[\s*test\s*\]$/;
const IGNORE_ATTRIBUTE = /^#\[\s*(?:ignore\b|cfg_attr\s*\(\s*miri\s*,\s*ignore\b)/;
const PATH_ATTRIBUTE = /^#\[\s*path\s*=\s*"([^"]+)"\s*\]$/;
const WORD_CHAR = /\w/;

/**
 * Returns the package names that the Miri job in the CI workflow tests.
 */
export const miriPackages = (workflow) => [
  ...new Set([...workflow.matchAll(MIRI_PACKAGE_PATTERN)].map((match) => match[1])),
];

const skipQuoted = (source, start, quote) => {
  let index = start + 1;
  while (index < source.length && source[index] !== quote) {
    index += source[index] === "\\" ? 2 : 1;
  }
  return index + 1;
};

const blank = (text) => text.replace(/[^\n]/g, " ");

const commentEnd = (source, index) => {
  if (source[index] !== "/") return null;
  if (source[index + 1] === "/") {
    const end = source.indexOf("\n", index);
    return end === -1 ? source.length : end;
  }
  if (source[index + 1] !== "*") return null;
  const end = source.indexOf("*/", index + 2);
  return end === -1 ? source.length : end + 2;
};

const stringEnd = (source, index) =>
  source[index] === '"' ? skipQuoted(source, index, '"') : null;

const RAW_STRING_START = /^b?r(#*)"/;

const prefixedStringEnd = (source, index) => {
  if (WORD_CHAR.test(source[index - 1] ?? "")) return null;
  if (source.startsWith('b"', index)) return skipQuoted(source, index + 1, '"');
  const raw = RAW_STRING_START.exec(source.slice(index, index + 260));
  if (!raw) return null;
  const end = source.indexOf(`"${raw[1]}`, index + raw[0].length);
  return end === -1 ? source.length : end + raw[1].length + 1;
};

const CHAR_LITERAL = /^'(?:\\(?:x[0-9a-fA-F]{2}|u\{[0-9a-fA-F]+\}|.)|[^\\'\n])'/u;

const charEnd = (source, index) => {
  if (source[index] !== "'") return null;
  const literal = CHAR_LITERAL.exec(source.slice(index, index + 12));
  return literal ? index + literal[0].length : null;
};

const LITERAL_SCANNERS = [commentEnd, stringEnd, prefixedStringEnd, charEnd];

const literalEnd = (source, index) => {
  for (const scan of LITERAL_SCANNERS) {
    const end = scan(source, index);
    if (end !== null) return end;
  }
  return null;
};

/**
 * Replaces comments, string literals and char literals with spaces. The
 * result has the same length and line breaks as the input, so an offset in
 * the result is the same offset in the source.
 */
export const stripLiterals = (source) => {
  let output = "";
  let index = 0;
  while (index < source.length) {
    const end = literalEnd(source, index);
    if (end === null) {
      output += source[index];
      index += 1;
    } else {
      output += blank(source.slice(index, end));
      index = end;
    }
  }
  return output;
};

const excludesMiri = (attribute) =>
  /\bcfg\s*\(/.test(attribute) && /\bnot\s*\(\s*miri\s*\)/.test(attribute);

const normalize = (attribute) => attribute.replace(/\s+/g, " ").trim();

const attributeEnd = (code, start) => {
  let depth = 0;
  for (let index = start; index < code.length; index += 1) {
    if (code[index] === "[") depth += 1;
    if (code[index] === "]") {
      depth -= 1;
      if (depth === 0) return index + 1;
    }
  }
  return code.length;
};

const firstFsPattern = (body) => FS_PATTERNS.find(({ regex }) => regex.test(body)) ?? null;

const hasMiriExclusion = (attributes) =>
  attributes.some((attribute) => excludesMiri(attribute.text));

const lineIndex = (code) => {
  const starts = [0];
  for (let index = 0; index < code.length; index += 1) {
    if (code[index] === "\n") starts.push(index + 1);
  }
  return (offset) => {
    let low = 0;
    let high = starts.length - 1;
    while (low < high) {
      const middle = Math.ceil((low + high) / 2);
      if (starts[middle] <= offset) low = middle;
      else high = middle - 1;
    }
    return low + 1;
  };
};

const scopeGated = (state) => state.fileGated || state.scopes.some((scope) => scope.gated);

const readAttribute = (state, index) => {
  const end = attributeEnd(state.code, index);
  const text = normalize(state.code.slice(index, end));
  if (state.code[index + 1] !== "!") {
    state.pending.push({ text, raw: normalize(state.source.slice(index, end)) });
  } else if (state.scopes.length === 0 && excludesMiri(text)) {
    state.fileGated = true;
  }
  return end;
};

const pathAttribute = (attributes) =>
  attributes.map((attribute) => PATH_ATTRIBUTE.exec(attribute.raw)?.[1]).find(Boolean) ?? null;

const readModule = (state, index, item) => {
  const gated = scopeGated(state) || hasMiriExclusion(state.pending);
  if (item[2] === "{") {
    state.armedModule = { name: item[1], gated };
    return index + item[0].length - 1;
  }
  if (!gated) {
    const inlinePath = state.scopes.filter((scope) => scope.module).map((scope) => scope.module);
    state.children.push({ name: item[1], path: pathAttribute(state.pending), inlinePath });
  }
  state.pending = [];
  return index + item[0].length;
};

const readWord = (state, index) => {
  const rest = state.code.slice(index, index + 200);
  const moduleItem = MOD_ITEM.exec(rest);
  if (moduleItem) return readModule(state, index, moduleItem);
  const fnItem = FN_ITEM.exec(rest);
  if (!fnItem) return index + /^\w+/.exec(rest)[0].length;
  state.armedFn = { name: fnItem[1], line: state.lineOf(index), attributes: state.pending };
  state.pending = [];
  return index + fnItem[0].length;
};

const newScope = (state, index) => {
  if (state.armedModule) return { module: state.armedModule.name, gated: state.armedModule.gated };
  if (!state.armedFn) return { gated: hasMiriExclusion(state.pending) };
  const gated = scopeGated(state) || hasMiriExclusion(state.armedFn.attributes);
  return { fn: state.armedFn, start: index, gated };
};

const openScope = (state, index) => {
  state.scopes.push(newScope(state, index));
  state.armedModule = null;
  state.armedFn = null;
  state.pending = [];
};

const testEntry = (state, scope, end) => {
  const match = firstFsPattern(state.code.slice(scope.start, end));
  const gated = scope.gated || state.fileGated;
  const ignored = scope.fn.attributes.some((attribute) => IGNORE_ATTRIBUTE.test(attribute.text));
  return {
    name: scope.fn.name,
    line: scope.fn.line,
    pattern: match?.pattern ?? null,
    attribute: match?.attribute ?? null,
    gated,
    missingIgnore: match !== null && !ignored && !gated,
  };
};

const closeScope = (state, index) => {
  const scope = state.scopes.pop();
  if (!scope?.fn?.attributes.some((attribute) => TEST_ATTRIBUTE.test(attribute.text))) return;
  state.tests.push(testEntry(state, scope, index));
};

const endItem = (state) => {
  state.pending = [];
  state.armedFn = null;
};

const PUNCTUATION = { "{": openScope, "}": closeScope, ";": endItem };
const ATTRIBUTE_START = /^#!?\[/;

const step = (state, index) => {
  const char = state.code[index];
  if (char === "#" && ATTRIBUTE_START.test(state.code.slice(index, index + 3))) {
    return readAttribute(state, index);
  }
  if (WORD_CHAR.test(char) && !WORD_CHAR.test(state.code[index - 1] ?? "")) {
    return readWord(state, index);
  }
  PUNCTUATION[char]?.(state, index);
  return index + 1;
};

/**
 * Scans one Rust source file. Returns the `#[test]` functions with the first
 * file system pattern of each body, the `mod name;` declarations that Miri
 * compiles, and whether an inner attribute removes the file from the Miri
 * build.
 */
export const scanFile = (source) => {
  const code = stripLiterals(source);
  const state = {
    source,
    code,
    lineOf: lineIndex(code),
    tests: [],
    children: [],
    scopes: [],
    fileGated: false,
    pending: [],
    armedFn: null,
    armedModule: null,
  };
  let index = 0;
  while (index < code.length) index = step(state, index);
  return { tests: state.tests, children: state.children, fileGated: state.fileGated };
};

/**
 * Finds the `#[test]` functions that call the file system and have no Miri
 * ignore mark. `files` holds `{ path, source }` entries.
 */
export const findMissingMiriIgnores = (files) =>
  files.flatMap((file) =>
    scanFile(file.source)
      .tests.filter((entry) => entry.missingIgnore)
      .map((entry) => ({
        path: file.path,
        line: entry.line,
        name: entry.name,
        pattern: entry.pattern,
        message: `${file.path}:${entry.line}: test \`${entry.name}\` calls \`${entry.pattern}\`, which Miri isolation blocks. Add ${entry.attribute} to the test.`,
      })),
  );

const moduleDirectory = (file, isRoot) => {
  const base = path.basename(file);
  if (isRoot || base === "mod.rs" || base === "lib.rs" || base === "main.rs") {
    return path.dirname(file);
  }
  return file.slice(0, -".rs".length);
};

const childFile = (file, isRoot, child) => {
  if (child.path !== null) {
    const base =
      child.inlinePath.length === 0
        ? path.dirname(file)
        : path.join(moduleDirectory(file, isRoot), ...child.inlinePath);
    return path.join(base, child.path);
  }
  const base = path.join(moduleDirectory(file, isRoot), ...child.inlinePath, child.name);
  return (
    [`${base}.rs`, path.join(base, "mod.rs")].find((candidate) => existsSync(candidate)) ?? null
  );
};

const crateRoots = (crateDirectory) => {
  const roots = ["src/lib.rs", "src/main.rs"]
    .map((relative) => path.join(crateDirectory, relative))
    .filter((file) => existsSync(file));
  const testsDirectory = path.join(crateDirectory, "tests");
  if (!existsSync(testsDirectory)) return roots;
  const testRoots = readdirSync(testsDirectory, { withFileTypes: true })
    .filter((entry) => entry.isFile() && entry.name.endsWith(".rs"))
    .map((entry) => path.join(testsDirectory, entry.name));
  return [...roots, ...testRoots];
};

/**
 * Loads each source file that the Miri test build of one crate compiles. The
 * walk starts at the library root and the integration test roots, follows
 * `mod name;` declarations, and leaves out modules declared under `not(miri)`.
 */
export const loadMiriSources = (root, crateDirectory) => {
  const files = [];
  const seen = new Set();
  const visit = (file, isRoot) => {
    if (seen.has(file) || !existsSync(file)) return;
    seen.add(file);
    const source = readFileSync(file, "utf8");
    files.push({ path: path.relative(root, file).split(path.sep).join("/"), source });
    for (const child of scanFile(source).children) {
      const target = childFile(file, isRoot, child);
      if (target !== null) visit(target, false);
    }
  };
  for (const file of crateRoots(path.join(root, crateDirectory))) visit(file, true);
  return files;
};

const crateDirectories = (root) => {
  const cratesDirectory = path.join(root, "crates");
  if (!existsSync(cratesDirectory)) return new Map();
  return new Map(
    readdirSync(cratesDirectory, { withFileTypes: true })
      .filter((entry) => entry.isDirectory())
      .flatMap((entry) => {
        const manifestPath = path.join(cratesDirectory, entry.name, "Cargo.toml");
        if (!existsSync(manifestPath)) return [];
        const name = /^\s*name\s*=\s*"([^"]+)"/m.exec(readFileSync(manifestPath, "utf8"))?.[1];
        return name ? [[name, path.join("crates", entry.name)]] : [];
      }),
  );
};

export const main = (_args = process.argv.slice(2), root = ROOT) => {
  const packages = miriPackages(readFileSync(path.join(root, CI_WORKFLOW), "utf8"));
  if (packages.length === 0) {
    console.error(`no \`miri test -p <crate>\` step found in ${CI_WORKFLOW}`);
    return 1;
  }
  const directories = crateDirectories(root);
  const findings = [];
  for (const name of packages) {
    const directory = directories.get(name);
    if (!directory) {
      console.error(
        `${CI_WORKFLOW} runs Miri for ${name}, but no crate under crates/ has that name`,
      );
      return 1;
    }
    findings.push(...findMissingMiriIgnores(loadMiriSources(root, directory)));
  }
  if (findings.length === 0) {
    console.log(`miri file system test check passed (${packages.join(", ")})`);
    return 0;
  }
  for (const finding of findings) console.error(`miri-fs-tests: ${finding.message}`);
  return 1;
};

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  runCliMain(main);
}
