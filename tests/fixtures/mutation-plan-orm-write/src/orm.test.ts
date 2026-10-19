import { expect, test } from "vitest";

import { evict } from "./cache";
import { cjsSave } from "./cjs.cjs";
import { addComputed } from "./computed";
import { lazySave } from "./lazy";
import { clearValues, insertPrepared, insertValue } from "./lite";
import { addRecord } from "./mongo";
import { addPerson } from "./person";
import { addRow } from "./pg";
import { addUser, countUsers } from "./pgjs";
import { clearAll, runAll } from "./prisma";
import { removeRecord } from "./remove";
import { runTask } from "./runner";
import { clamp, save } from "./save";
import { insertRow } from "./typed";
import { addValue } from "./vercel";

test("pure helpers", () => {
  expect(evict(new Map([[1, "a"]]), 1)).toBe(true);
  expect(clamp(-1)).toBe(0);
});

test("writers exist", () => {
  for (const writer of [
    save,
    addPerson,
    cjsSave,
    lazySave,
    addRecord,
    removeRecord,
    runAll,
    clearAll,
    insertRow,
    addUser,
    countUsers,
    addValue,
    insertValue,
    clearValues,
    insertPrepared,
    runTask,
    addRow,
    addComputed,
  ]) {
    expect(typeof writer).toBe("function");
  }
});
