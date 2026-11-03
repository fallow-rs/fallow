import { execFileSync } from "node:child_process";

import { expect, test } from "vitest";

import { add } from "../src/math.ts";

const moduleUrl = new URL("../src/math.ts", import.meta.url).href;

test("adds in a Node child process", () => {
  expect(Number.isFinite(add(2, 2))).toBe(true);
  const output = execFileSync(
    process.execPath,
    [
      "--input-type=module",
      "--eval",
      `import { add } from ${JSON.stringify(moduleUrl)}; console.log(add(2, 2));`,
    ],
    { encoding: "utf8" },
  );
  expect(output.trim()).toBe("4");
});

test("keeps one subprocess preload in NODE_OPTIONS", () => {
  const preloads = (process.env.NODE_OPTIONS ?? "").match(/node-subprocess-preload\.mjs/gu) ?? [];
  expect(preloads).toHaveLength(1);
});
