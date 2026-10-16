#!/usr/bin/env node
/**
 * Keep the version strings fallow emits about itself in lockstep with the
 * workspace version.
 *
 * Fallow stamps its own version into JSON output under `version` and
 * `fallow_version`. Those strings are copied into published examples (the npm
 * skill contract) and into the MCP Registry server card. Nothing recompiles
 * them, so they rot silently: a released package can claim a version several
 * releases behind the binary it ships with.
 *
 * Detection targets fallow's own output shape and nothing else. A `version` or
 * `fallow_version` string counts only when the JSON object that directly holds
 * it also carries `schema_version`, which every fallow payload does and no
 * third-party manifest example does. A dependency version nested inside a
 * fallow payload sits in its own object and is left alone.
 *
 * Usage:
 *   node scripts/sync-emitted-versions.mjs                  rewrite to the workspace version
 *   node scripts/sync-emitted-versions.mjs --check          fail when a string is stale
 *   node scripts/sync-emitted-versions.mjs --version 3.22.0 rewrite to an explicit version
 *   node scripts/sync-emitted-versions.mjs --check docs     add a markdown tree
 *
 * `scripts/sync-npm-versions.sh` calls the rewrite mode during a release, so
 * the release commit and the gate share one definition of the surface set.
 */

import { readFileSync, readdirSync, statSync, writeFileSync } from "node:fs";
import { dirname, join, relative } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

import { runCliMain } from "./cli-main.mjs";

const REPO_ROOT = join(dirname(fileURLToPath(import.meta.url)), "..");

const VERSION_KEYS = new Set(["version", "fallow_version"]);
const OUTPUT_ANCHOR_KEY = "schema_version";
const SEMVER = /^\d+\.\d+\.\d+(?:[-+][0-9A-Za-z.-]+)?$/u;
const JSON_LANGUAGES = new Set(["json", "jsonc", "json5"]);

/** Documentation trees whose fenced JSON examples are fallow output. */
export const MARKDOWN_SURFACES = ["npm/fallow/skills"];

/** JSON documents whose version fields describe fallow itself. */
export const JSON_SURFACES = ["server.json"];

const readJsonString = (text, start) => {
  let index = start + 1;
  let value = "";
  while (index < text.length) {
    const character = text[index];
    if (character === "\\") {
      value += text[index + 1] ?? "";
      index += 2;
      continue;
    }
    if (character === '"') {
      return { value, end: index + 1, valueStart: start + 1, valueEnd: index };
    }
    if (character === "\n") {
      return null;
    }
    value += character;
    index += 1;
  }
  return null;
};

const skipWhitespace = (text, index) => {
  let cursor = index;
  while (cursor < text.length && /\s/u.test(text[cursor])) {
    cursor += 1;
  }
  return cursor;
};

/**
 * Collect the fallow-emitted version strings in one JSON example body.
 *
 * Objects are tracked by brace depth so an anchor applies to its own object
 * only. A truncated example never closes its braces, so open scopes are
 * flushed at the end rather than dropped.
 */
const scanJsonExample = (text, offset) => {
  const emitted = [];
  const scopes = [];
  const closeScope = () => {
    const scope = scopes.pop();
    if (scope?.anchored) {
      emitted.push(...scope.candidates);
    }
  };

  let index = 0;
  while (index < text.length) {
    const character = text[index];
    // JSON_LANGUAGES admits `jsonc` and `json5`, so a fenced block may carry
    // comments. Skip them outright rather than scanning their text: a comment
    // that merely quotes `"version":` would otherwise register as a real key,
    // and a commented-out third-party pin inside a genuine fallow envelope
    // would be rewritten to the workspace version. Both were reproduced before
    // this guard existed.
    if (character === "/" && text[index + 1] === "/") {
      const lineEnd = text.indexOf("\n", index);
      index = lineEnd === -1 ? text.length : lineEnd + 1;
      continue;
    }
    if (character === "/" && text[index + 1] === "*") {
      const blockEnd = text.indexOf("*/", index + 2);
      index = blockEnd === -1 ? text.length : blockEnd + 2;
      continue;
    }
    if (character === "{") {
      scopes.push({ anchored: false, candidates: [] });
      index += 1;
      continue;
    }
    if (character === "}") {
      closeScope();
      index += 1;
      continue;
    }
    if (character !== '"') {
      index += 1;
      continue;
    }

    const key = readJsonString(text, index);
    if (!key) {
      index += 1;
      continue;
    }
    const colon = skipWhitespace(text, key.end);
    if (text[colon] !== ":") {
      index = key.end;
      continue;
    }

    const scope = scopes.at(-1);
    const valueStart = skipWhitespace(text, colon + 1);
    if (scope && key.value === OUTPUT_ANCHOR_KEY) {
      scope.anchored = true;
    }
    if (scope && VERSION_KEYS.has(key.value) && text[valueStart] === '"') {
      const value = readJsonString(text, valueStart);
      if (value && SEMVER.test(value.value)) {
        scope.candidates.push({
          key: key.value,
          version: value.value,
          start: offset + value.valueStart,
          end: offset + value.valueEnd,
        });
      }
    }
    index = valueStart;
  }

  while (scopes.length > 0) {
    closeScope();
  }
  return emitted;
};

const jsonExampleBlocks = (source) => {
  const blocks = [];
  let open = null;
  let offset = 0;

  const closeBlock = () => {
    if (JSON_LANGUAGES.has(open.language)) {
      blocks.push({ offset: open.offset, text: open.lines.join("\n") });
    }
    open = null;
  };

  for (const line of source.split("\n")) {
    const fence = line.match(/^\s*(`{3,})\s*(\S*)/u);
    if (open === null) {
      if (fence) {
        open = {
          language: fence[2].toLowerCase(),
          lines: [],
          marker: fence[1],
          offset: offset + line.length + 1,
        };
      }
    } else if (fence && fence[2] === "" && fence[1].length >= open.marker.length) {
      closeBlock();
    } else {
      open.lines.push(line);
    }
    offset += line.length + 1;
  }

  if (open !== null) {
    closeBlock();
  }
  return blocks;
};

const lineNumber = (source, offset) => source.slice(0, offset).split("\n").length;

/** Every fallow-emitted version string in a markdown document, in file order. */
export const findEmittedVersions = (source) =>
  jsonExampleBlocks(source)
    .flatMap((block) => scanJsonExample(block.text, block.offset))
    .map((entry) => ({ ...entry, line: lineNumber(source, entry.start) }))
    .toSorted((left, right) => left.start - right.start);

/** Rewrite every fallow-emitted version string in a markdown document. */
export const rewriteEmittedVersions = (source, version) => {
  let output = source;
  for (const entry of findEmittedVersions(source).toReversed()) {
    output = `${output.slice(0, entry.start)}${version}${output.slice(entry.end)}`;
  }
  return output;
};

/** The version fields of the MCP Registry server card, top level and per package. */
export const serverCardVersions = (card) => [
  { label: "version", version: card.version },
  ...(card.packages ?? []).map((entry, index) => ({
    label: `packages[${index}].version`,
    version: entry.version,
  })),
];

/** Rewrite the server card version fields without disturbing key order. */
export const rewriteServerCard = (card, version) =>
  card.packages
    ? { ...card, version, packages: card.packages.map((entry) => ({ ...entry, version })) }
    : { ...card, version };

export const workspaceVersion = (root = REPO_ROOT) => {
  const manifest = readFileSync(join(root, "Cargo.toml"), "utf8");
  const version = manifest.match(/^\[workspace\.package\]\nversion = "([^"]+)"/mu)?.[1];
  if (!version) {
    throw new Error("Cargo.toml must declare workspace.package.version");
  }
  return version;
};

const markdownFilesUnder = (root) => {
  if (statSync(root).isFile()) {
    return root.endsWith(".md") ? [root] : [];
  }
  return readdirSync(root, { withFileTypes: true })
    .filter((entry) => !entry.name.startsWith("."))
    .flatMap((entry) => markdownFilesUnder(join(root, entry.name)))
    .toSorted();
};

const isJsonSurface = (path) => path.endsWith(".json");

const surfaceSites = (root, surface) => {
  const absolute = join(root, surface);
  if (isJsonSurface(surface)) {
    const card = JSON.parse(readFileSync(absolute, "utf8"));
    return serverCardVersions(card).map((site) => ({ ...site, path: surface }));
  }
  return markdownFilesUnder(absolute).flatMap((file) => {
    const source = readFileSync(file, "utf8");
    const path = relative(root, file).split("\\").join("/");
    return findEmittedVersions(source).map((entry) => ({
      label: `${entry.key} (line ${entry.line})`,
      path,
      version: entry.version,
    }));
  });
};

/**
 * Compare every emitted version string against the expected version.
 *
 * `sites` is reported alongside `drift` so a caller can fail when the surface
 * list matches nothing at all, which would make the gate vacuous.
 */
export const auditEmittedVersions = ({ root = REPO_ROOT, surfaces = [], version } = {}) => {
  const expected = version ?? workspaceVersion(root);
  const sites = [...MARKDOWN_SURFACES, ...JSON_SURFACES, ...surfaces].flatMap((surface) =>
    surfaceSites(root, surface),
  );
  return {
    drift: sites.filter((site) => site.version !== expected),
    sites,
    version: expected,
  };
};

/** Rewrite every emitted version string; returns the paths that changed. */
export const writeEmittedVersions = ({ root = REPO_ROOT, surfaces = [], version } = {}) => {
  const target = version ?? workspaceVersion(root);
  const changed = [];

  for (const surface of [...MARKDOWN_SURFACES, ...surfaces]) {
    for (const file of markdownFilesUnder(join(root, surface))) {
      const source = readFileSync(file, "utf8");
      const updated = rewriteEmittedVersions(source, target);
      if (updated !== source) {
        writeFileSync(file, updated);
        changed.push(relative(root, file).split("\\").join("/"));
      }
    }
  }

  for (const surface of JSON_SURFACES) {
    const absolute = join(root, surface);
    const source = readFileSync(absolute, "utf8");
    const updated = `${JSON.stringify(rewriteServerCard(JSON.parse(source), target), null, 2)}\n`;
    if (updated !== source) {
      writeFileSync(absolute, updated);
      changed.push(surface);
    }
  }

  return changed;
};

const parseArguments = (args) => {
  const options = { check: false, surfaces: [], version: undefined };
  for (let index = 0; index < args.length; index += 1) {
    const argument = args[index];
    if (argument === "--check") {
      options.check = true;
    } else if (argument === "--version") {
      index += 1;
      // A trailing `--version` with nothing after it must not fall through to
      // the workspace version: the caller asked for a specific target and got
      // silently overridden.
      if (index >= args.length) {
        throw new Error("--version requires a value");
      }
      options.version = args[index];
    } else if (argument.startsWith("--version=")) {
      options.version = argument.slice("--version=".length);
    } else if (argument.startsWith("-")) {
      throw new Error(`unknown option: ${argument}`);
    } else if (isJsonSurface(argument)) {
      throw new Error(`extra surfaces must be markdown trees, got: ${argument}`);
    } else {
      options.surfaces.push(argument);
    }
  }
  if (options.version !== undefined && !SEMVER.test(options.version)) {
    throw new Error(`--version expects a semantic version, got: ${options.version ?? ""}`);
  }
  return options;
};

export const main = (args = process.argv.slice(2)) => {
  const options = parseArguments(args);

  if (!options.check) {
    const changed = writeEmittedVersions(options);
    const target = options.version ?? workspaceVersion();
    for (const path of changed) {
      console.log(`  Updated ${path} → ${target}`);
    }
    if (changed.length === 0) {
      console.log(`  Emitted version strings already at ${target}`);
    }
    return 0;
  }

  const result = auditEmittedVersions(options);
  if (result.sites.length === 0) {
    console.error(
      "no fallow-emitted version strings found: the surface list or the detector is wrong",
    );
    return 1;
  }
  if (result.drift.length > 0) {
    for (const site of result.drift) {
      console.error(`${site.path}: ${site.label} is ${site.version}, expected ${result.version}`);
    }
    console.error("re-run this command without --check to refresh these surfaces");
    return 1;
  }

  console.log(`emitted version strings match the workspace version ${result.version}`);
  return 0;
};

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  runCliMain(main);
}
