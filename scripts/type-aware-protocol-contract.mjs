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
// `crates/api/type-aware-protocol.json` is higher than the pinned version.

import { createHash } from "node:crypto";
import { readFileSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");
export const MANIFEST_PATH = "crates/api/type-aware-protocol.json";
export const PIN_PATH = "crates/api/type-aware-protocol-contract.json";

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
  {
    path: "crates/api/src/type_aware/transport/session.rs",
    kind: "serde",
    exclude: ["TypeAwareFileChanges"],
  },
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

const withoutComments = (lines) =>
  lines
    .map((line) => line.replace(/\s+$/u, ""))
    .filter((line) => {
      const trimmed = line.trim();
      return (
        trimmed !== "" &&
        !trimmed.startsWith("//") &&
        !trimmed.startsWith("/*") &&
        !trimmed.startsWith("*")
      );
    });

const SERDE_DERIVE = /^\s*#\[derive\([^\]]*\b(?:Serialize|Deserialize)\b/u;
const ITEM_NAME = /^\s*(?:pub(?:\([^)]*\))?\s+)?(?:struct|enum)\s+([A-Za-z0-9_]+)/u;

const TEST_MODULE = /^(?:pub(?:\([^)]*\))?\s+)?mod tests\b/u;

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

/** Each item with a serde derive: its attributes and its body, by name.
 * The test module of the file is not part of the contract. */
export const serdeItems = (source) => {
  const all = source.replace(/\r\n/gu, "\n").split("\n");
  const testModule = all.findIndex((line) => TEST_MODULE.test(line));
  const lines = testModule === -1 ? all : all.slice(0, testModule);
  const items = [];
  for (let index = 0; index < lines.length; index += 1) {
    if (!SERDE_DERIVE.test(lines[index])) {
      continue;
    }
    const { end, name } = itemEnd(lines, index);
    items.push({ name, text: withoutComments(lines.slice(index, end + 1)).join("\n") });
    index = end;
  }
  return items;
};

const readSource = (root, path) => readFileSync(join(root, path), "utf8").replace(/\r\n/gu, "\n");

const sourceText = (root, source) => {
  const text = readSource(root, source.path);
  if (source.kind === "json") {
    const { wire_protocol_version: _version, ...rest } = JSON.parse(text);
    return JSON.stringify(rest);
  }
  if (source.kind === "module") {
    return withoutComments(text.split("\n")).join("\n");
  }
  return serdeItems(text)
    .filter((item) => !(source.exclude ?? []).includes(item.name))
    .map((item) => item.text)
    .join("\n\n");
};

const TYPE_NAME = /\b[A-Z][A-Za-z0-9]*\b/gu;

/** The shared items that the contract text uses, with their own uses. */
const sharedItems = (root, contractText, sharedSources) => {
  const pool = new Map();
  for (const path of sharedSources) {
    for (const item of serdeItems(readSource(root, path))) {
      pool.set(item.name, { ...item, path });
    }
  }
  const selected = new Map();
  const pending = [...new Set(contractText.match(TYPE_NAME) ?? [])];
  while (pending.length > 0) {
    const name = pending.pop();
    const item = pool.get(name);
    if (!item || selected.has(name)) {
      continue;
    }
    selected.set(name, item);
    pending.push(...new Set(item.text.match(TYPE_NAME) ?? []));
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
  let rustText = "";
  for (const source of sources) {
    const text = sourceText(root, source);
    if (source.kind === "serde") {
      rustText += `${text}\n`;
    }
    hash.update(`${source.path}\n${text}\n\0\n`);
  }
  for (const item of sharedItems(root, rustText, sharedSources)) {
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
  if (pin.contract_sha256 !== hash && version <= pin.wire_protocol_version) {
    throw new Error(
      `The type-aware wire contract changed. Raise wire_protocol_version in ${MANIFEST_PATH} ` +
        `above ${pin.wire_protocol_version} before you record a new pin.`,
    );
  }
  const next = {
    description:
      "SHA-256 of the type-aware wire contract sources. scripts/type-aware-protocol-contract.mjs owns this file.",
    wire_protocol_version: version,
    contract_sha256: hash,
  };
  writeFileSync(join(root, PIN_PATH), `${JSON.stringify(next, null, 2)}\n`);
  return next;
};

const isDirectInvocation =
  process.argv[1] !== undefined && resolve(process.argv[1]) === fileURLToPath(import.meta.url);

if (isDirectInvocation) {
  if (process.argv[2] === "--write") {
    const pin = writePin();
    console.log(`pinned wire protocol ${pin.wire_protocol_version}: ${pin.contract_sha256}`);
  } else {
    const problems = checkPin();
    if (problems.length > 0) {
      for (const problem of problems) {
        console.error(problem);
      }
      process.exit(1);
    }
    console.log("type-aware wire contract matches its protocol version");
  }
}
