import assert from "node:assert/strict";
import { test } from "node:test";
import { build } from "esbuild";

test("the plugin output keeps packages external", async () => {
  const result = await build({
    stdin: { contents: "export {}" },
    bundle: true,
    packages: "external",
    write: false,
  });
  assert.deepEqual(result.errors, []);
});
