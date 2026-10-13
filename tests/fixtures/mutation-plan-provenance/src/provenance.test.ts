import { expect, test } from "vitest";

import { download, fetchNamed } from "./download";
import { get, getOr } from "./get";
import { legacyDownload } from "./legacy";
import { pick } from "./pick";

test("local get and pick", () => {
  expect(get({ a: 1 }, "a")).toBe(1);
  expect(getOr({}, "a", 2)).toBe(2);
  expect(pick({ a: 1 }, ["a"])).toEqual([1]);
});

test("network calls", () => {
  expect(typeof download).toBe("function");
  expect(typeof fetchNamed).toBe("function");
  expect(typeof legacyDownload).toBe("function");
});
