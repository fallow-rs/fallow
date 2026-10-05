import { createServer } from "vite";
import { expect, test } from "vitest";

test("renders the view", async () => {
  const server = await createServer();
  const { renderView } = await server.ssrLoadModule("/src/view.ts");
  expect(renderView()).toBe("view");
});
