// Imports the package by its published name, as the README shows. Node
// resolves the name through the `exports` map of package.json, so this test
// covers the entry points that users load.
import { strict as assert } from "node:assert";
import { createRequire } from "node:module";

import fallow, * as namespace from "@fallow-cli/fallow-node";
import { computeHealth, detectDeadCode, detectSimilarCode } from "@fallow-cli/fallow-node";

const require = createRequire(import.meta.url);
const required = require("@fallow-cli/fallow-node");

assert.equal(typeof detectDeadCode, "function");
assert.equal(typeof computeHealth, "function");
assert.equal(fallow, required, "the default ESM export is the CommonJS export");
assert.equal(
  detectSimilarCode,
  required.detectSimilarCode,
  "the named export is the wrapped detectSimilarCode of the loader",
);

const bindingFunctions = Object.keys(required).filter(
  (name) => typeof required[name] === "function",
);
assert.ok(bindingFunctions.length > 0, "the binding exports functions");
const namedExports = new Map(Object.entries(namespace));
for (const name of bindingFunctions) {
  assert.equal(namedExports.get(name), required[name], `named ESM export ${name}`);
}

const report = await detectDeadCode({ root: process.cwd(), noCache: true });
assert.equal(typeof report, "object");
console.log("  [PASS] named ESM imports of the package name");
