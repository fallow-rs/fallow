import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";

const readJson = (path) => JSON.parse(readFileSync(path, "utf8"));

test("MCP Registry metadata matches the published npm package", () => {
  const card = readJson("server.json");
  const packageManifest = readJson("npm/fallow/package.json");
  const [npmPackage] = card.packages;

  assert.equal(
    card.$schema,
    "https://static.modelcontextprotocol.io/schemas/2025-12-11/server.schema.json",
  );
  assert.ok(card.description.length <= 100);
  assert.equal(card.name, packageManifest.mcpName);
  assert.equal(card.version, packageManifest.version);
  assert.equal(npmPackage.identifier, packageManifest.name);
  assert.equal(npmPackage.version, packageManifest.version);
  assert.deepEqual(npmPackage.transport, { type: "stdio" });
  assert.deepEqual(npmPackage.packageArguments, [{ type: "positional", value: "mcp-server" }]);
  assert.equal(card.repository.url, "https://github.com/fallow-rs/fallow");
});
