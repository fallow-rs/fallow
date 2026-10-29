import { appendFileSync, readFileSync, realpathSync } from "node:fs";
import { resolve } from "node:path";

/**
 * The environment of a test process that a child-process adapter starts.
 * The preload reads each value and removes it, so the test code never sees
 * it.
 */
export const CHILD_ENV = Object.freeze({
  /** The path of the overlay file: `{ "<real source path>": { source, path, target } }`. */
  overlay: "FALLOW_MUTATION_OVERLAY",
  /** The id of the active mutant. Absent in a dry run. */
  active: "FALLOW_MUTATION_ACTIVE",
  /** The path of the file that receives one JSON line for each finished test. */
  hits: "FALLOW_MUTATION_HITS",
});

/** The prefix of the test diagnostic that carries the hits of a test. */
export const HITS_DIAGNOSTIC = "fallow-mutation-hits:";

/** The private integrity log shared by a test process and its Node children. */
export const overlayErrorsPath = (overlayPath) => `${overlayPath}.errors`;

/** Records incomplete instrumentation even when a test ignores its child's result. */
export const recordOverlayError = (overlayPath, message) => {
  appendFileSync(overlayErrorsPath(overlayPath), `${message}\n`);
};

/** Reads one variable of the child environment and removes it. */
export const takeChildEnv = (name) => {
  const value = process.env[name];
  delete process.env[name];
  return typeof value === "string" && value.length > 0 ? value : null;
};

export const realPath = (path) => {
  try {
    return realpathSync(path);
  } catch {
    return resolve(path);
  }
};

/**
 * Reads the overlay file as a map from the real path of a source file to
 * `{ source, path, target }`. No file gives an empty map.
 */
export const readOverlay = (path) =>
  path === null ? new Map() : new Map(Object.entries(JSON.parse(readFileSync(path, "utf8"))));

/** Escapes the regular expression characters of a text. */
export const escapeRegExp = (text) => text.replace(/[.*+?^${}()|[\]\\]/gu, "\\$&");
