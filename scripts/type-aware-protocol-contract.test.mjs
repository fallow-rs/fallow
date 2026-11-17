import assert from "node:assert/strict";
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { test } from "node:test";

import {
  MANIFEST_PATH,
  PIN_PATH,
  checkPin,
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
