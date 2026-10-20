import { expect, test } from "vitest";

import { evict } from "./cache";
import { cjsSave } from "./cjs.cjs";
import { addComputed } from "./computed";
import { addMapped, findMapped, PrivateRepository, StaticRepository } from "./holders";
import { lazySave } from "./lazy";
import { clearValues, insertPrepared, insertValue } from "./lite";
import { addRecord } from "./mongo";
import { addPerson } from "./person";
import { parseId, parseStored, parseWord } from "./pattern";
import { addRow } from "./pg";
import { addUser, countUsers } from "./pgjs";
import { clearAll, runAll } from "./prisma";
import { removeRecord } from "./remove";
import { clearAssigned, Repository } from "./repository";
import { pageCount } from "./routes";
import { runTask } from "./runner";
import { clamp, save } from "./save";
import { addInline, addStored, countStored, findStored } from "./statements";
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
    addStored,
    addInline,
    findStored,
    countStored,
    pageCount,
    parseId,
    parseWord,
    parseStored,
    clearAssigned,
  ]) {
    expect(typeof writer).toBe("function");
  }
});

test("repository methods exist", () => {
  const repository = new Repository();
  for (const method of [repository.addField, repository.setField, repository.findField]) {
    expect(typeof method).toBe("function");
  }
});

test("statement holders exist", () => {
  const repository = new PrivateRepository();
  for (const method of [addMapped, findMapped, repository.addPrivate, StaticRepository.addStatic]) {
    expect(typeof method).toBe("function");
  }
});
