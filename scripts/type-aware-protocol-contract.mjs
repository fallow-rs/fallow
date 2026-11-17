// Pin the wire contract of the type-aware companion to its protocol version.
//
// Fallow accepts a companion of another release when it speaks the same
// `wire_protocol_version`. That is safe only when every change of the wire
// format raises the version. This script hashes the sources that define the
// format and compares the hash with `crates/api/type-aware-protocol-contract.json`.
//
// Usage:
//   node scripts/type-aware-protocol-contract.mjs           check the pin
//   node scripts/type-aware-protocol-contract.mjs --write   record a new pin
//
// `--write` refuses a changed contract until `wire_protocol_version` in
// `crates/api/type-aware-protocol.json` is higher than the pinned version, and
// it refuses a version below the pinned version. The check also compares the
// pin with the pin of `origin/main` when that ref exists, so a hand edit of
// the pin file cannot keep a changed contract at the base version.
//
// The hash covers the request and response modules of the sidecar and the
// Rust types that serialize and parse each message. The response values that
// other sidecar modules build (for example `call-evidence.mjs`) count through
// the Rust types that parse them: a response that a new Fallow cannot parse
// changes those types, and so the hash.

import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { readFileSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");
export const MANIFEST_PATH = "crates/api/type-aware-protocol.json";
export const PIN_PATH = "crates/api/type-aware-protocol-contract.json";
/**
 * Version of the hash computation. Raise it when this script changes what the
 * hash covers or how it normalizes text. A pin of another method is not
 * comparable, so `--write` may record the new hash at the same protocol
 * version.
 */
export const HASH_METHOD = 2;
const methodOf = (pin) => pin.hash_method ?? 1;

/**
 * The sources that define the wire format.
 *
 * - `json`: the protocol manifest without `wire_protocol_version`.
 * - `module`: a whole JavaScript module of the companion, without comments.
 * - `serde`: every item with a serde derive in a Rust file, without comments
 *   and without the test module. `exclude` names public API items that never
 *   cross the wire.
 */
const CONTRACT_SOURCES = [
  { path: MANIFEST_PATH, kind: "json" },
  { path: "tools/type-aware-sidecar/src/protocol.mjs", kind: "module" },
  { path: "tools/type-aware-sidecar/src/cli.mjs", kind: "module" },
  { path: "tools/type-aware-sidecar/src/response-normalization.mjs", kind: "module" },
  {
    path: "crates/api/src/type_aware/transport/mod.rs",
    kind: "serde",
    exclude: ["TypeAwareStatus"],
  },
  { path: "crates/api/src/type_aware/transport/session.rs", kind: "serde" },
  { path: "crates/api/src/type_aware/client/mod.rs", kind: "serde" },
  { path: "crates/api/src/type_aware/client/call_evidence.rs", kind: "serde" },
  { path: "crates/api/src/type_aware/mutant_typecheck.rs", kind: "serde" },
];

/** Shared type files. Their serde items count when a contract item uses them,
 * directly or through another shared item. */
const SHARED_TYPE_SOURCES = [
  "crates/types/src/semantic.rs",
  "crates/types/src/envelope.rs",
  "crates/output/src/mutation.rs",
];

/** The index after the comment that starts at `index`, or -1. A line
 * comment ends before its newline. */
const commentEnd = (text, index) => {
  if (text.startsWith("//", index)) {
    const end = text.indexOf("\n", index);
    return end === -1 ? text.length : end;
  }
  if (text.startsWith("/*", index)) {
    const end = text.indexOf("*/", index + 2);
    return end === -1 ? text.length : end + 2;
  }
  return -1;
};

/** The index after the string literal that opens at `index`. */
const stringEnd = (text, index) => {
  const quote = text[index];
  let cursor = index + 1;
  while (cursor < text.length && text[cursor] !== quote) {
    cursor += text[cursor] === "\\" ? 2 : 1;
  }
  return Math.min(cursor + 1, text.length);
};

/**
 * Remove comments outside string literals. Rust has only `"` strings here
 * (a `'` starts a lifetime); JavaScript also has `'` and template strings.
 */
const stripComments = (text, language) => {
  const quotes = language === "rust" ? '"' : "\"'`";
  let out = "";
  let index = 0;
  while (index < text.length) {
    const end = commentEnd(text, index);
    if (end !== -1) {
      out += " ";
      index = end;
      continue;
    }
    const next = quotes.includes(text[index]) ? stringEnd(text, index) : index + 1;
    out += text.slice(index, next);
    index = next;
  }
  return out;
};

/** Collapse whitespace to one space, and drop it next to punctuation, so
 * formatting does not change the hash. */
const normalizeSpace = (text) =>
  text
    .replace(/\s+/gu, " ")
    .replace(/ ?([^\w\s"'`]) ?/gu, "$1")
    .trim();

const SERDE_DERIVE = /^\s*#\[derive\([^\]]*\b(?:Serialize|Deserialize)\b/u;
const ITEM_NAME = /^\s*(?:pub(?:\([^)]*\))?\s+)?(?:struct|enum)\s+([A-Za-z0-9_]+)/u;
/** An inline test module. `mod tests;` loads another file and ends nothing. */
const TEST_MODULE = /^(?:pub(?:\([^)]*\))?\s+)?mod tests\s*\{/u;

/** The last line of the item that starts at `start`, and its name. */
const itemEnd = (lines, start) => {
  const indent = lines[start].match(/^\s*/u)[0];
  let name;
  for (let end = start; end < lines.length; end += 1) {
    const line = lines[end];
    name ??= line.match(ITEM_NAME)?.[1];
    if (name === undefined) {
      continue;
    }
    const closesBlock = line === `${indent}}` || line === `${indent}};`;
    const endsOneLineItem = line.startsWith(indent) && /;\s*$/u.test(line) && !line.includes("{");
    if (closesBlock || endsOneLineItem) {
      return { end, name };
    }
  }
  throw new Error(`a serde derive at line ${start + 1} has no struct or enum`);
};

/** Each item with a serde derive: its name, its normalized text, and the
 * type names of its fields. The inline test module does not count. */
export const serdeItems = (source) => {
  const all = stripComments(source.replace(/\r\n/gu, "\n"), "rust").split("\n");
  const testModule = all.findIndex((line) => TEST_MODULE.test(line));
  const lines = testModule === -1 ? all : all.slice(0, testModule);
  const items = [];
  for (let index = 0; index < lines.length; index += 1) {
    if (!SERDE_DERIVE.test(lines[index])) {
      continue;
    }
    const { end, name } = itemEnd(lines, index);
    const body = lines.slice(index, end + 1);
    items.push({ name, text: normalizeSpace(body.join("\n")), uses: fieldTypes(body) });
    index = end;
  }
  return items;
};

const FIELD = /^\s*(?:pub(?:\([^)]*\))?\s+)?(?:r#)?[a-z_][A-Za-z0-9_]*\s*:\s*(.+?)\s*,?\s*$/u;
const TUPLE =
  /^\s*(?:pub(?:\([^)]*\))?\s+)?(?:struct\s+)?[A-Z][A-Za-z0-9_]*\s*\((.+)\)\s*[,;]?\s*$/u;
const TYPE_NAME = /\b[A-Z][A-Za-z0-9_]*\b/gu;

/** The type names that the fields and tuple variants of an item use. */
const fieldTypes = (lines) => {
  const names = new Set();
  for (const line of lines) {
    if (/^\s*#/u.test(line)) {
      continue;
    }
    const type = line.match(FIELD)?.[1] ?? line.match(TUPLE)?.[1];
    for (const name of type?.match(TYPE_NAME) ?? []) {
      names.add(name);
    }
  }
  return [...names];
};

/** Types of the standard library and of dependencies that the wire types may
 * use. Any other type must come from a listed source. */
const EXTERNAL_TYPES = new Set([
  "Arc",
  "BTreeMap",
  "BTreeSet",
  "Box",
  "Cow",
  "FxHashMap",
  "FxHashSet",
  "HashMap",
  "HashSet",
  "Option",
  "PathBuf",
  "Self",
  "String",
  "Value",
  "Vec",
]);

const readSource = (root, path) => readFileSync(join(root, path), "utf8").replace(/\r\n/gu, "\n");

const sourceText = (root, source) => {
  const text = readSource(root, source.path);
  if (source.kind === "json") {
    const { wire_protocol_version: _version, ...rest } = JSON.parse(text);
    return { text: JSON.stringify(rest), items: [] };
  }
  if (source.kind === "module") {
    return { text: normalizeSpace(stripComments(text, "js")), items: [] };
  }
  const items = serdeItems(text).filter((item) => !(source.exclude ?? []).includes(item.name));
  return { text: items.map((item) => item.text).join("\n"), items };
};

/** The shared items that the contract items use, transitively. A type that
 * neither a contract source, a shared source, nor the external list defines
 * is an error, so a new wire type cannot drop out of the hash. */
const sharedItems = (root, contractItems, sharedSources) => {
  const pool = new Map();
  for (const path of sharedSources) {
    for (const item of serdeItems(readSource(root, path))) {
      pool.set(item.name, { ...item, path });
    }
  }
  const local = new Set(contractItems.map((item) => item.name));
  const selected = new Map();
  const pending = contractItems.flatMap((item) => item.uses.map((name) => [name, item.name]));
  while (pending.length > 0) {
    const [name, user] = pending.pop();
    if (local.has(name) || selected.has(name) || EXTERNAL_TYPES.has(name)) {
      continue;
    }
    const item = pool.get(name);
    if (!item) {
      throw new Error(
        `wire type ${name} (used by ${user}) is not defined in a listed source. ` +
          "Add its file to SHARED_TYPE_SOURCES in scripts/type-aware-protocol-contract.mjs, " +
          "or add an external type to EXTERNAL_TYPES.",
      );
    }
    selected.set(name, item);
    pending.push(...item.uses.map((used) => [used, name]));
  }
  return [...selected.values()].toSorted((left, right) =>
    `${left.path}:${left.name}`.localeCompare(`${right.path}:${right.name}`),
  );
};

/** SHA-256 of the contract sources, in their listed order. */
export const contractHash = (
  root = ROOT,
  sources = CONTRACT_SOURCES,
  sharedSources = SHARED_TYPE_SOURCES,
) => {
  const hash = createHash("sha256");
  const contractItems = [];
  for (const source of sources) {
    const { text, items } = sourceText(root, source);
    contractItems.push(...items);
    hash.update(`${source.path}\n${text}\n\0\n`);
  }
  for (const item of sharedItems(root, contractItems, sharedSources)) {
    hash.update(`${item.path}:${item.name}\n${item.text}\n\0\n`);
  }
  return hash.digest("hex");
};

const readJson = (root, path) => JSON.parse(readFileSync(join(root, path), "utf8"));

const pinState = (root, sources, sharedSources) => ({
  version: readJson(root, MANIFEST_PATH).wire_protocol_version,
  pin: readJson(root, PIN_PATH),
  hash: contractHash(root, sources, sharedSources),
});

/** Compare the pin with the sources. Returns a list of problems. */
export const checkPin = (
  root = ROOT,
  sources = CONTRACT_SOURCES,
  sharedSources = SHARED_TYPE_SOURCES,
) => {
  const { version, pin, hash } = pinState(root, sources, sharedSources);
  const problems = [];
  if (methodOf(pin) !== HASH_METHOD) {
    return [
      `${PIN_PATH} uses hash method ${methodOf(pin)}, but this script uses ${HASH_METHOD}. ` +
        "Run `node scripts/type-aware-protocol-contract.mjs --write`.",
    ];
  }
  if (pin.contract_sha256 !== hash) {
    problems.push(
      pin.wire_protocol_version === version
        ? `The type-aware wire contract changed, but wire_protocol_version is still ${version}. ` +
            `A companion of an older release would pass the version check and then fail. ` +
            `Raise wire_protocol_version in ${MANIFEST_PATH}, run \`npm run generate:contracts\`, ` +
            `then run \`node scripts/type-aware-protocol-contract.mjs --write\`.`
        : `wire_protocol_version is ${version}, but ${PIN_PATH} pins ${pin.wire_protocol_version}. ` +
            `Run \`node scripts/type-aware-protocol-contract.mjs --write\`.`,
    );
  } else if (pin.wire_protocol_version !== version) {
    problems.push(
      `wire_protocol_version is ${version}, but ${PIN_PATH} pins ${pin.wire_protocol_version} for an unchanged contract. ` +
        `Run \`node scripts/type-aware-protocol-contract.mjs --write\`.`,
    );
  }
  return problems;
};

/** Record the pin. Refuses a changed contract without a higher version. */
export const writePin = (
  root = ROOT,
  sources = CONTRACT_SOURCES,
  sharedSources = SHARED_TYPE_SOURCES,
) => {
  const { version, pin, hash } = pinState(root, sources, sharedSources);
  if (version < pin.wire_protocol_version) {
    throw new Error(
      `wire_protocol_version ${version} is below the pinned version ${pin.wire_protocol_version}. ` +
        "A version never goes down: raise it above the pinned version.",
    );
  }
  const sameMethod = methodOf(pin) === HASH_METHOD;
  if (sameMethod && pin.contract_sha256 !== hash && version <= pin.wire_protocol_version) {
    throw new Error(
      `The type-aware wire contract changed. Raise wire_protocol_version in ${MANIFEST_PATH} ` +
        `above ${pin.wire_protocol_version} before you record a new pin.`,
    );
  }
  const next = {
    description:
      "SHA-256 of the type-aware wire contract sources. scripts/type-aware-protocol-contract.mjs owns this file.",
    hash_method: HASH_METHOD,
    wire_protocol_version: version,
    contract_sha256: hash,
  };
  writeFileSync(join(root, PIN_PATH), `${JSON.stringify(next, null, 2)}\n`);
  return next;
};

/**
 * Compare a pin with the pin of the base branch. A changed contract needs a
 * version above the base version, and a version never goes below it.
 * Returns no problems when there is no base pin.
 */
export const basePinProblems = (pin, base) => {
  if (!base) {
    return [];
  }
  if (pin.wire_protocol_version < base.wire_protocol_version) {
    return [
      `${PIN_PATH} pins version ${pin.wire_protocol_version}, below ${base.wire_protocol_version} on the base branch.`,
    ];
  }
  if (
    methodOf(pin) === methodOf(base) &&
    pin.contract_sha256 !== base.contract_sha256 &&
    pin.wire_protocol_version <= base.wire_protocol_version
  ) {
    return [
      `The type-aware wire contract differs from the base branch, so wire_protocol_version must be above ` +
        `${base.wire_protocol_version}, the version on the base branch. Raise it in ${MANIFEST_PATH}, ` +
        "then run `node scripts/type-aware-protocol-contract.mjs --write`.",
    ];
  }
  return [];
};

/** The pin of `ref`, or `undefined` when git or the ref has none. */
export const readBasePin = (ref = "origin/main", root = ROOT) => {
  const result = spawnSync("git", ["show", `${ref}:${PIN_PATH}`], { cwd: root, encoding: "utf8" });
  return result.status === 0 ? JSON.parse(result.stdout) : undefined;
};

const isDirectInvocation =
  process.argv[1] !== undefined && resolve(process.argv[1]) === fileURLToPath(import.meta.url);

if (isDirectInvocation) {
  if (process.argv[2] === "--write") {
    const pin = writePin();
    console.log(`pinned wire protocol ${pin.wire_protocol_version}: ${pin.contract_sha256}`);
  } else {
    const problems = [
      ...checkPin(),
      ...basePinProblems(JSON.parse(readFileSync(join(ROOT, PIN_PATH), "utf8")), readBasePin()),
    ];
    if (problems.length > 0) {
      for (const problem of problems) {
        console.error(problem);
      }
      process.exit(1);
    }
    console.log("type-aware wire contract matches its protocol version");
  }
}
