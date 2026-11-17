import assert from "node:assert/strict";
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { test } from "node:test";

import {
  MANIFEST_PATH,
  HASH_METHOD,
  PIN_PATH,
  basePinProblems,
  checkPin,
  readBasePin,
  contractHash,
  serdeItems,
  writePin,
} from "./type-aware-protocol-contract.mjs";

test("the repository pin matches the type-aware wire contract", () => {
  assert.deepEqual(checkPin(), []);
  const pin = JSON.parse(readFileSync(PIN_PATH, "utf8"));
  const manifest = JSON.parse(readFileSync(MANIFEST_PATH, "utf8"));
  assert.equal(pin.wire_protocol_version, manifest.wire_protocol_version);
  assert.match(pin.contract_sha256, /^[0-9a-f]{64}$/u);
});

const SOURCES = [
  { path: MANIFEST_PATH, kind: "json" },
  { path: "sidecar/protocol.mjs", kind: "module" },
  { path: "api/wire.rs", kind: "serde", exclude: ["PublicStatus"] },
];
const SHARED = ["types/semantic.rs"];

const WIRE = `use serde::{Deserialize, Serialize};

/// A request.
#[derive(Serialize)]
struct Request {
    protocol_version: u32,
    symbol: SemanticSymbol,
}

#[derive(Serialize)]
pub struct PublicStatus {
    pub available: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "kebab-case")]
enum Outcome {
    Complete,
    Abstained,
}

#[cfg(test)]
mod tests {
    #[derive(serde::Serialize)]
    struct Fixture {
        value: u32,
    }
}
`;

const SEMANTIC = `#[derive(Debug, Serialize)]
pub struct SemanticSymbol {
    pub path: String,
    pub namespace: SemanticNamespace,
}

#[derive(Debug, Serialize)]
pub enum SemanticNamespace {
    Value,
    Type,
}

#[derive(Debug, Serialize)]
pub struct Unrelated {
    pub field: u32,
}
`;

const fixture = (t) => {
  const root = mkdtempSync(join(tmpdir(), "fallow-protocol-contract-"));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  const write = (path, text) => {
    mkdirSync(dirname(join(root, path)), { recursive: true });
    writeFileSync(join(root, path), text);
  };
  write(MANIFEST_PATH, JSON.stringify({ wire_protocol_version: 7, status_operation: "status" }));
  write("sidecar/protocol.mjs", "// Request keys.\nexport const KEYS = ['id'];\n");
  write("api/wire.rs", WIRE);
  write("types/semantic.rs", SEMANTIC);
  write(
    PIN_PATH,
    JSON.stringify({
      hash_method: HASH_METHOD,
      wire_protocol_version: 7,
      contract_sha256: contractHash(root, SOURCES, SHARED),
    }),
  );
  const edit = (path, from, to) => {
    const text = readFileSync(join(root, path), "utf8");
    assert.ok(text.includes(from), `${path} has no ${from}`);
    write(path, text.replace(from, to));
  };
  const setVersion = (version) => {
    const manifest = JSON.parse(readFileSync(join(root, MANIFEST_PATH), "utf8"));
    write(MANIFEST_PATH, JSON.stringify({ ...manifest, wire_protocol_version: version }));
  };
  return { root, edit, setVersion, check: () => checkPin(root, SOURCES, SHARED) };
};

test("a wire change without a version bump fails and names the fix", (t) => {
  const { root, edit, setVersion, check } = fixture(t);
  assert.deepEqual(check(), []);

  edit(
    "api/wire.rs",
    "    symbol: SemanticSymbol,\n",
    "    symbol: SemanticSymbol,\n    files: Vec<String>,\n",
  );
  const [problem] = check();
  assert.match(problem, /wire contract changed/u);
  assert.match(problem, /Raise wire_protocol_version/u);
  assert.throws(() => writePin(root, SOURCES, SHARED), /Raise wire_protocol_version/u);

  setVersion(8);
  const pin = writePin(root, SOURCES, SHARED);
  assert.equal(pin.wire_protocol_version, 8);
  assert.deepEqual(check(), []);
});

test("each part of the contract changes the hash", (t) => {
  for (const [path, from, to] of [
    ["sidecar/protocol.mjs", "['id']", "['id', 'files']"],
    [MANIFEST_PATH, '"status"', '"status-v2"'],
    ["api/wire.rs", "    Abstained,\n", "    Abstained,\n    Skipped,\n"],
    ["types/semantic.rs", "    Type,\n", "    Type,\n    Both,\n"],
  ]) {
    const { edit, check } = fixture(t);
    edit(path, from, to);
    assert.equal(check().length, 1, `${path}: ${to}`);
  }
});

test("comments, tests, public API items and unused shared types do not change the hash", (t) => {
  for (const [path, from, to] of [
    ["sidecar/protocol.mjs", "// Request keys.", "// The keys of a request."],
    ["api/wire.rs", "/// A request.", "/// One request of the client."],
    ["api/wire.rs", "        value: u32,", "        value: u64,"],
    ["api/wire.rs", "    pub available: bool,", "    pub available: bool,\n    pub path: String,"],
    ["types/semantic.rs", "    pub field: u32,", "    pub field: u64,"],
  ]) {
    const { edit, check } = fixture(t);
    edit(path, from, to);
    assert.deepEqual(check(), [], `${path}: ${to}`);
  }
});

test("a version bump with an unchanged contract also needs a new pin", (t) => {
  const { setVersion, check } = fixture(t);
  setVersion(8);
  const [problem] = check();
  assert.match(problem, /pins 7 for an unchanged contract/u);
});

test("the serde reader finds every derived item outside the test module", () => {
  assert.deepEqual(
    serdeItems(WIRE).map((item) => item.name),
    ["Request", "PublicStatus", "Outcome"],
  );
  assert.match(contractHash(), /^[0-9a-f]{64}$/u);
});

test("the writer refuses a version below the pinned version", (t) => {
  const { root, edit, setVersion } = fixture(t);
  edit("api/wire.rs", "    Abstained,\n", "    Abstained,\n    Skipped,\n");
  setVersion(9);
  writePin(root, SOURCES, SHARED);
  setVersion(8);
  assert.throws(() => writePin(root, SOURCES, SHARED), /below the pinned version 9/u);
});

test("a contract change needs a version above the pin of the base branch", () => {
  const base = {
    hash_method: HASH_METHOD,
    wire_protocol_version: 8,
    contract_sha256: "a".repeat(64),
  };
  const same = { ...base };
  assert.deepEqual(basePinProblems(same, base), []);
  const handEdited = { ...base, contract_sha256: "b".repeat(64) };
  assert.match(basePinProblems(handEdited, base)[0], /above 8, the version on the base branch/u);
  const lowered = { ...base, wire_protocol_version: 7 };
  assert.match(basePinProblems(lowered, base)[0], /below 8/u);
  const raised = { ...base, wire_protocol_version: 9, contract_sha256: "b".repeat(64) };
  assert.deepEqual(basePinProblems(raised, base), []);
  const newMethod = { ...handEdited, hash_method: HASH_METHOD + 1 };
  assert.deepEqual(basePinProblems(newMethod, base), [], "hashes of two methods do not compare");
  assert.deepEqual(basePinProblems(handEdited, undefined), []);
});

test("the repository pin is above the base branch pin when the contract changed", (t) => {
  const base = readBasePin();
  if (!base) {
    t.skip("origin/main has no pin to compare with");
    return;
  }
  const current = JSON.parse(readFileSync(PIN_PATH, "utf8"));
  assert.deepEqual(basePinProblems(current, base), []);
});

test("formatting and comments inside code do not change the hash", (t) => {
  for (const [path, from, to] of [
    ["api/wire.rs", "    protocol_version: u32,", "    protocol_version: u32, // the version"],
    ["api/wire.rs", '#[serde(rename_all = "kebab-case")]', '#[serde( rename_all = "kebab-case" )]'],
    ["api/wire.rs", "/// A request.", "/* A request,\n   over two lines. */"],
    ["sidecar/protocol.mjs", "['id'];", "[ 'id' ]; // keys"],
    ["sidecar/protocol.mjs", "// Request keys.", "/*\n Request keys.\n*/"],
  ]) {
    const { edit, check } = fixture(t);
    edit(path, from, to);
    assert.deepEqual(check(), [], `${path}: ${to}`);
  }
  const { edit, check } = fixture(t);
  edit("sidecar/protocol.mjs", "['id']", "['id // not a comment']");
  assert.equal(check().length, 1, "a // inside a string is code");
});

test("only an inline test module ends the contract part of a file", () => {
  const source = `#[path = "wire_tests.rs"]
mod tests;

#[derive(Serialize)]
struct Late {
    value: u32,
}
`;
  assert.deepEqual(
    serdeItems(source).map((item) => item.name),
    ["Late"],
  );
});

test("a wire type that no listed source defines stops the hash", (t) => {
  const { root, edit } = fixture(t);
  edit(
    "api/wire.rs",
    "    symbol: SemanticSymbol,\n",
    "    symbol: SemanticSymbol,\n    extra: MissingType,\n",
  );
  assert.throws(() => contractHash(root, SOURCES, SHARED), /MissingType/u);
});

test("a new hash method needs a new pin, which the writer records at the same version", (t) => {
  const { root, check } = fixture(t);
  const pinFile = join(root, PIN_PATH);
  const pin = JSON.parse(readFileSync(pinFile, "utf8"));
  writeFileSync(
    pinFile,
    JSON.stringify({ ...pin, hash_method: HASH_METHOD - 1, contract_sha256: "c".repeat(64) }),
  );
  assert.match(check()[0], /hash method/u);
  assert.equal(writePin(root, SOURCES, SHARED).wire_protocol_version, 7);
  assert.deepEqual(check(), []);
});
