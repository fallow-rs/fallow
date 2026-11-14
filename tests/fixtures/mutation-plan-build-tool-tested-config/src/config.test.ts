import { expect, test } from "vitest";
import config from "../vite.config";

test("the config has one plugin", () => {
  expect(config.plugins?.length).toBe(1);
});
