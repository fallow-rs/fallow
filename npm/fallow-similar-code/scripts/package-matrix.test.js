"use strict";

const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const companion = require("../package.json");

test("every supported platform has an exact-version native package", () => {
  const dependencies = companion.optionalDependencies;
  assert.deepEqual(Object.keys(dependencies).toSorted(), [
    "@fallow-cli/fallow-similar-code-darwin-arm64",
    "@fallow-cli/fallow-similar-code-darwin-x64",
    "@fallow-cli/fallow-similar-code-linux-arm64-gnu",
    "@fallow-cli/fallow-similar-code-linux-arm64-musl",
    "@fallow-cli/fallow-similar-code-linux-x64-gnu",
    "@fallow-cli/fallow-similar-code-linux-x64-musl",
    "@fallow-cli/fallow-similar-code-win32-arm64-msvc",
    "@fallow-cli/fallow-similar-code-win32-x64-msvc",
  ]);
  for (const [name, version] of Object.entries(dependencies)) {
    assert.equal(version, companion.version);
    const directory = name.replace("@fallow-cli/fallow-similar-code-", "fallow-similar-code-");
    const manifestPath = path.join(__dirname, "..", "..", directory, "package.json");
    const native = JSON.parse(fs.readFileSync(manifestPath, "utf8"));
    assert.equal(native.name, name);
    assert.equal(native.version, companion.version);
  }
});
