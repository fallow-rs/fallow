// Generated from tools/mutation-runner/src/runtime/child-coverage.mjs. Do not edit.
import { randomUUID } from "node:crypto";
import { mkdirSync, readFileSync, readdirSync, renameSync, rmSync, writeFileSync } from "node:fs";
import { join } from "node:path";

import { childHitsPath } from "./child.mjs";

/**
 * The coverage of the Node processes that a test starts.
 *
 * A Node child of a test loads the subprocess preload. In a dry run, the
 * preload URL carries the token of the test. At exit, the child writes its
 * hits and its loaded paths to one file in the child-hits directory, with
 * the token as the name prefix. After the test, the test process takes the
 * files of its token and adds them to the hits of the test. Without this
 * step, a test that asserts only on the output of a child has no hits, and
 * the API never selects it for a mutant.
 *
 * A child that a signal ends writes no file, so its hits are lost.
 */

const TOKEN = /^[0-9a-f-]{36}$/u;
const FILE_SUFFIX = ".json";

/**
 * Writes the hits of this child process when it exits. `collect()` gives
 * `{ hits, loaded }`. A child with no hit and no loaded path writes no file.
 * The file is written under a temporary name and then renamed, so the test
 * process never reads a part of a file.
 */
export const reportChildHitsAtExit = (overlayPath, token, collect) => {
  if (overlayPath === null || token === null || !TOKEN.test(token)) return;
  process.on("exit", () => {
    try {
      const { hits, loaded } = collect();
      if (hits.length === 0 && loaded.length === 0) return;
      const dir = childHitsPath(overlayPath);
      mkdirSync(dir, { recursive: true });
      const path = join(dir, `${token}.${randomUUID()}`);
      writeFileSync(`${path}.tmp`, JSON.stringify({ hits, loaded }));
      renameSync(`${path}.tmp`, `${path}${FILE_SUFFIX}`);
    } catch {
      // A hit that cannot be written is lost. The child result stays.
    }
  });
};

const readChildFile = (path) => {
  try {
    const { hits, loaded } = JSON.parse(readFileSync(path, "utf8"));
    return {
      hits: Array.isArray(hits) ? hits.filter((id) => typeof id === "string") : [],
      loaded: Array.isArray(loaded) ? loaded.filter((source) => typeof source === "string") : [],
    };
  } catch {
    return { hits: [], loaded: [] };
  } finally {
    rmSync(path, { force: true });
  }
};

const listTokenFiles = (dir, token) => {
  try {
    return readdirSync(dir).filter(
      (name) => name.startsWith(`${token}.`) && name.endsWith(FILE_SUFFIX),
    );
  } catch {
    return [];
  }
};

/**
 * Creates the test-process side of the child coverage. `startTest()` gives
 * the token for the Node children of the test. Tests that run at the same
 * time share one token, as they share the hits of the test process.
 * `finishTest()` takes the hits and the loaded paths that the children of
 * the token wrote so far.
 */
export const createChildCoverage = (overlayPath) => {
  const dir = childHitsPath(overlayPath);
  let running = 0;
  let token = null;
  return {
    startTest: () => {
      if (running === 0) token = randomUUID();
      running += 1;
      return token;
    },
    finishTest: () => {
      running = Math.max(0, running - 1);
      const files = token === null ? [] : listTokenFiles(dir, token);
      const reports = files.map((name) => readChildFile(join(dir, name)));
      return {
        hits: reports.flatMap((report) => report.hits),
        loaded: reports.flatMap((report) => report.loaded),
      };
    },
  };
};
