import { build } from "esbuild";
import { expect, test } from "vitest";

test("the plugin output keeps packages external", async () => {
  const result = await build({
    stdin: { contents: "export {}" },
    bundle: true,
    packages: "external",
    write: false,
  });
  expect(result.errors).toEqual([]);
});
