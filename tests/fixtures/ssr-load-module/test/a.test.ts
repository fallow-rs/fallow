import { createServer } from "vite";
import { expect, test } from "vitest";

test("loads the module through the dev server", async () => {
  const vite = await createServer();
  const m = await vite.ssrLoadModule("/src/a.ts");
  expect(m.used).toBe(1);
});
