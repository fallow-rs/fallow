#!/usr/bin/env node
// Miri builds each crate test target with `cfg(miri)` set. A module declared
// under `not(miri)` does not exist in that build, so any path into it from code
// that Miri still compiles breaks the whole crate. The Miri CI job is
// path-filtered, so this check runs on every change instead.
import { readdirSync, readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

import { runCliMain } from "./cli-main.mjs";

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const CI_WORKFLOW = ".github/workflows/ci.yml";
const MIRI_PACKAGE_PATTERN = /\bmiri test -p ([\w-]+)/g;
const MOD_DECLARATION = /^\s*(?:pub(?:\([^)]*\))?\s+)?mod\s+([a-z_][a-z0-9_]*)\s*([;{])/;
const PATH_REFERENCE = /\b(crate|super|self)((?:::[A-Za-z_][A-Za-z0-9_]*)+)/g;

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

const skipRawString = (source, start) => {
  const hashes = /^r(#*)"/.exec(source.slice(start, start + 260))?.[1] ?? null;
  if (hashes === null) return null;
  const end = source.indexOf(`"${hashes}`, start + hashes.length + 2);
  return end === -1 ? source.length : end + hashes.length + 1;
};

const blank = (text) => text.replace(/[^\n]/g, " ");

/**
 * Replaces comments, string literals and char literals with spaces. Line
 * breaks stay, so line numbers and brace depth stay correct.
 */
export const stripLiterals = (source) => {
  let output = "";
  let index = 0;
  while (index < source.length) {
    const char = source[index];
    const next = source[index + 1];
    let end = null;
    if (char === "/" && next === "/") {
      end = source.indexOf("\n", index);
      if (end === -1) end = source.length;
    } else if (char === "/" && next === "*") {
      end = source.indexOf("*/", index + 2);
      end = end === -1 ? source.length : end + 2;
    } else if (char === '"') {
      end = skipQuoted(source, index, '"');
    } else if ((char === "r" || char === "b") && !/[\w]/.test(source[index - 1] ?? "")) {
      const rawStart = char === "b" && next === "r" ? index + 1 : index;
      end = skipRawString(source, rawStart);
      if (end === null && char === "b" && next === '"') end = skipQuoted(source, index + 1, '"');
    } else if (char === "'") {
      const literal = /^'(?:\\(?:x[0-9a-fA-F]{2}|u\{[0-9a-fA-F]+\}|.)|[^\\'\n])'/u.exec(
        source.slice(index, index + 12),
      );
      if (literal) end = index + literal[0].length;
    }
    if (end === null) {
      output += char;
      index += 1;
    } else {
      output += blank(source.slice(index, end));
      index = end;
    }
  }
  return output;
};

const bracketBalance = (text) =>
  (text.match(/\[/g)?.length ?? 0) - (text.match(/\]/g)?.length ?? 0);

const excludesMiri = (attribute) =>
  /\bcfg\s*\(/.test(attribute) && /\bnot\s*\(\s*miri\s*\)/.test(attribute);

/**
 * Maps a source file below `src/` to its module path.
 */
export const modulePathFor = (relativeToSrc) => {
  const segments = relativeToSrc.replace(/\.rs$/, "").split("/");
  const last = segments.at(-1);
  if (last === "mod" || (segments.length === 1 && (last === "lib" || last === "main"))) {
    segments.pop();
  }
  return segments;
};

/**
 * Scans one file. Returns the modules it declares under `not(miri)` and the
 * paths it names, each with the module path and Miri state of the site.
 */
export const scanFile = (source, modulePath) => {
  const lines = stripLiterals(source).split("\n");
  const gatedModules = [];
  const references = [];
  const fileGated = lines.some((line) => /^\s*#!\[/.test(line) && excludesMiri(line));
  const scopes = [];
  let depth = 0;
  let pendingAttributes = [];
  let pendingGatedItem = null;

  const currentPath = () => [
    ...modulePath,
    ...scopes.filter((scope) => scope.name).map((scope) => scope.name),
  ];
  const insideGated = () =>
    fileGated || pendingGatedItem !== null || scopes.some((scope) => scope.gated);

  let attribute = null;
  let armedModule = null;

  lines.forEach((line, lineIndex) => {
    const trimmed = line.trim();
    if (attribute === null && trimmed.startsWith("#[")) attribute = "";
    if (attribute !== null) {
      attribute += ` ${trimmed}`;
      if (bracketBalance(attribute) <= 0) {
        pendingAttributes.push(attribute);
        attribute = null;
      }
      return;
    }
    const isItemLine = trimmed !== "" && !trimmed.startsWith("#!");
    const attributeGated = isItemLine && pendingAttributes.some(excludesMiri);
    const declaration = isItemLine ? MOD_DECLARATION.exec(line) : null;
    if (declaration && attributeGated) {
      gatedModules.push([...currentPath(), declaration[1]]);
    }
    if (attributeGated && pendingGatedItem === null) {
      pendingGatedItem = { depth, braced: false };
    }
    armedModule =
      declaration && declaration[2] === "{"
        ? { name: declaration[1], gated: attributeGated }
        : null;

    for (const match of line.matchAll(PATH_REFERENCE)) {
      references.push({
        line: lineIndex + 1,
        root: match[1],
        segments: match[2].slice(2).split("::"),
        sitePath: currentPath(),
        siteGated: insideGated(),
      });
    }

    for (const char of line) {
      if (char === "{") {
        if (pendingGatedItem && !pendingGatedItem.braced && depth === pendingGatedItem.depth) {
          pendingGatedItem.braced = true;
        }
        scopes.push({ depth, name: armedModule?.name ?? null, gated: armedModule?.gated ?? false });
        armedModule = null;
        depth += 1;
      } else if (char === "}") {
        depth -= 1;
        scopes.pop();
        if (pendingGatedItem && pendingGatedItem.braced && depth === pendingGatedItem.depth) {
          pendingGatedItem = null;
        }
      } else if (
        char === ";" &&
        pendingGatedItem &&
        !pendingGatedItem.braced &&
        depth === pendingGatedItem.depth
      ) {
        pendingGatedItem = null;
      }
    }

    if (isItemLine) pendingAttributes = [];
  });

  return { gatedModules, references };
};

const resolveReference = ({ root, segments, sitePath }) => {
  if (root === "crate") return segments;
  const base = [...sitePath];
  let rest = [...segments];
  if (root === "super") base.pop();
  while (rest[0] === "super") {
    base.pop();
    rest = rest.slice(1);
  }
  return [...base, ...rest];
};

const hasPrefix = (target, prefix) =>
  prefix.length <= target.length && prefix.every((segment, index) => target[index] === segment);

/**
 * Finds paths from Miri-compiled code into modules that Miri does not compile.
 * `files` holds `{ path, modulePath, source }` for one crate.
 */
export const findMiriCfgViolations = (files) => {
  const scanned = files.map((file) => ({ ...file, ...scanFile(file.source, file.modulePath) }));
  const gated = scanned.flatMap((file) => file.gatedModules);
  return scanned.flatMap((file) =>
    file.references
      .filter((reference) => !reference.siteGated)
      .filter((reference) => !gated.some((prefix) => hasPrefix(reference.sitePath, prefix)))
      .flatMap((reference) => {
        const target = resolveReference(reference);
        const module = gated.find((prefix) => hasPrefix(target, prefix));
        if (!module) return [];
        return [
          {
            path: file.path,
            line: reference.line,
            module: module.join("::"),
            message: `${file.path}:${reference.line} names crate::${module.join("::")}, which is compiled only under not(miri); gate this code with not(miri) too`,
          },
        ];
      }),
  );
};

const rustFilesBelow = (directory) =>
  readdirSync(directory, { withFileTypes: true }).flatMap((entry) => {
    const entryPath = path.join(directory, entry.name);
    if (entry.isDirectory()) return rustFilesBelow(entryPath);
    return entry.isFile() && entry.name.endsWith(".rs") ? [entryPath] : [];
  });

const crateDirectories = (root) =>
  new Map(
    readdirSync(path.join(root, "crates"), { withFileTypes: true })
      .filter((entry) => entry.isDirectory())
      .flatMap((entry) => {
        try {
          const manifest = readFileSync(
            path.join(root, "crates", entry.name, "Cargo.toml"),
            "utf8",
          );
          const name = /^\s*name\s*=\s*"([^"]+)"/m.exec(manifest)?.[1];
          return name ? [[name, path.join("crates", entry.name)]] : [];
        } catch {
          return [];
        }
      }),
  );

/**
 * Loads the source files of one crate below its `src/` directory.
 */
export const loadCrateSources = (root, crateDirectory) => {
  const srcRoot = path.join(root, crateDirectory, "src");
  return rustFilesBelow(srcRoot).map((absolute) => ({
    path: path.relative(root, absolute).split(path.sep).join("/"),
    modulePath: modulePathFor(path.relative(srcRoot, absolute).split(path.sep).join("/")),
    source: readFileSync(absolute, "utf8"),
  }));
};

export const main = (_args = process.argv.slice(2), root = ROOT) => {
  const packages = miriPackages(readFileSync(path.join(root, CI_WORKFLOW), "utf8"));
  if (packages.length === 0) {
    console.error(`no \`miri test -p <crate>\` step found in ${CI_WORKFLOW}`);
    return 1;
  }
  const directories = crateDirectories(root);
  const violations = [];
  for (const name of packages) {
    const directory = directories.get(name);
    if (!directory) {
      console.error(
        `${CI_WORKFLOW} runs Miri for ${name}, but no crate under crates/ has that name`,
      );
      return 1;
    }
    violations.push(...findMiriCfgViolations(loadCrateSources(root, directory)));
  }
  if (violations.length === 0) {
    console.log(`miri cfg check passed (${packages.join(", ")})`);
    return 0;
  }
  for (const violation of violations) {
    console.error(`miri-cfg: ${violation.message}`);
  }
  return 1;
};

if (import.meta.url === `file://${process.argv[1]}`) {
  runCliMain(main);
}
