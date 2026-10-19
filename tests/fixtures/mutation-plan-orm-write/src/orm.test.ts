import { expect, test } from "vitest";

import { evict } from "./cache";
import { cjsSave } from "./cjs.cjs";
import { lazySave } from "./lazy";
import { addRecord } from "./mongo";
import { addPerson } from "./person";
import { clearAll, runAll } from "./prisma";
import { removeRecord } from "./remove";
import { clamp, save } from "./save";
import { insertRow } from "./typed";

test("pure helpers", () => {
  expect(evict(new Map([[1, "a"]]), 1)).toBe(true);
  expect(clamp(-1)).toBe(0);
});

test("writers exist", () => {
  for (const writer of [save, addPerson, cjsSave, lazySave, addRecord, removeRecord, runAll, clearAll, insertRow]) {
    expect(typeof writer).toBe("function");
  }
});
