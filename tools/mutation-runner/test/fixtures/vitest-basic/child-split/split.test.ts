import { execFileSync } from "node:child_process";

import { expect, test } from "vitest";

import { add } from "../src/math.ts";

const moduleUrl = new URL("../src/math.ts", import.meta.url).href;

test("calls add in the test process", () => {
  expect(Number.isFinite(add(2, 2))).toBe(true);
});

test("asserts only in a Node child process", () => {
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

test("starts no child process", () => {
  expect(1 + 1).toBe(2);
});
