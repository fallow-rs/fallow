import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";

import { formatSnapshot, memberKinds, topTypeFor } from "./effect-schema-members.mjs";

const SNAPSHOT_DIR = new URL("../crates/extract/tests/effect_schema_members/", import.meta.url);

test("memberKinds names each form that gives a schema, or none", () => {
  assert.deepEqual(memberKinds({ read: true, call: false, transform: false }), ["read"]);
  assert.deepEqual(memberKinds({ read: false, call: false, transform: false }), ["none"]);
});

test("memberKinds gives a dual member the arity of its data-first form", () => {
  assert.deepEqual(memberKinds({ read: false, call: true, transform: true, callArity: 2 }), [
    "call",
    "transform",
    "arity:2",
  ]);
});

test("topTypeFor picks the top type of the Effect major version", () => {
  assert.equal(topTypeFor(4), "S.Top | S.Constraint");
  assert.equal(topTypeFor(3), "S.Schema<any, any, any> | S.Schema.All");
});

test("formatSnapshot sorts the members and writes one member per line", () => {
  const text = formatSnapshot({
    effect: "4.0.0",
    typescript: "5.9.3",
    members: { optional: ["call"], String: ["read"], is: ["none"] },
  });
  assert.equal(
    text,
    '{\n  "effect": "4.0.0",\n  "typescript": "5.9.3",\n  "members": {\n    "is": ["none"],\n    "optional": ["call"],\n    "String": ["read"]\n  }\n}\n',
  );
  assert.deepEqual(JSON.parse(text).members.optional, ["call"]);
});

test("each committed snapshot is in the format of formatSnapshot", () => {
  for (const file of ["effect-3.22.5.json", "effect-4.1.5.json"]) {
    const text = readFileSync(new URL(file, SNAPSHOT_DIR), "utf8");
    assert.equal(formatSnapshot(JSON.parse(text)), text, file);
  }
});
