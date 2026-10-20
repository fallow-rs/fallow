import { existsSync } from "node:fs";
import path from "node:path";

import { selectBackend } from "./backend-selection.mjs";

export const BACKEND = selectBackend();

const { version: resolvedVersion } = await import(BACKEND.package);
export const sync = await import(`${BACKEND.package}/unstable/sync`);
export const ast = await import(`${BACKEND.package}/unstable/ast/is`);
const { serverFS } = await import(`${BACKEND.package}/unstable/fs`);

if (resolvedVersion !== BACKEND.version) {
  throw new Error(
    `${BACKEND.package} backend version ${resolvedVersion} does not match protocol ${BACKEND.version}`,
  );
}

/**
 * The value a `readFile` callback returns to read the file from disk.
 * TypeScript 7.0 reads from disk on `undefined`. TypeScript 7.1 treats
 * `undefined` as a missing file and needs the `serverFS.useOS` sentinel.
 */
export const READ_FROM_DISK = serverFS?.useOS;

const FS_CALLBACK_DEFAULTS = serverFS
  ? {
      readFile: serverFS.useOS,
      fileExists: serverFS.useOS,
      directoryExists: serverFS.useOS,
      getAccessibleEntries: serverFS.useOS,
      realpath: serverFS.useOS,
      stat: serverFS.useOS,
      writeFile: serverFS.useOS,
      removeFile: serverFS.useOS,
    }
  : {};

/**
 * TypeScript 7.0 finds a project by its config file name. TypeScript 7.1 keys
 * `getProject` by project id and finds a config file through
 * `getConfiguredProject`.
 */
const getProject = (snapshot, name) =>
  snapshot.getProject(name) ?? snapshot.getConfiguredProject(name);

/**
 * Give a TypeScript 7.1 API the TypeScript 7.0 `updateSnapshot(changes)`
 * contract: each call applies `changes` to the newest snapshot and returns
 * current programs.
 *
 * - 7.0 `fileChanges` become 7.1 `fileNotifications`, and `ensurePrograms`
 *   rebuilds the programs that those notifications make dirty.
 * - 7.1 derives a snapshot from a live base snapshot, so a snapshot that the
 *   caller disposes stays alive while it is the newest one. The caller's
 *   handle still acts as disposed, as it does on 7.0.
 * - 7.1 rejects the whole update when an `openFiles` entry does not exist.
 *   7.0 ignores that file, so the wrapper drops missing files first.
 */
const withLinearSnapshots = (api) => {
  const entries = new Set();
  let head;
  const disposeWhenUnused = (entry) => {
    if (entry === head || entry.open > 0) return;
    entries.delete(entry);
    if (!entry.snapshot.isDisposed()) entry.snapshot.dispose();
  };
  const handle = (entry) => {
    entry.open += 1;
    let released = false;
    const dispose = () => {
      if (released) return;
      released = true;
      entry.open -= 1;
      disposeWhenUnused(entry);
    };
    return new Proxy(entry.snapshot, {
      get(target, property) {
        if (property === "dispose" || property === Symbol.dispose) return dispose;
        if (property === "isDisposed") return () => released || target.isDisposed();
        if (released) throw new Error("Snapshot is disposed");
        if (property === "getProject") return getProject.bind(undefined, target);
        const value = Reflect.get(target, property, target);
        return typeof value === "function" ? value.bind(target) : value;
      },
    });
  };
  const updateSnapshot = ({ fileChanges, openFiles, ...rest } = {}) => {
    const changes = {
      ...rest,
      ...(openFiles ? { openFiles: openFiles.filter((file) => existsSync(file)) } : {}),
      ...(fileChanges ? { fileNotifications: fileChanges } : {}),
      ensurePrograms: true,
    };
    const previous = head;
    head = {
      snapshot: previous ? previous.snapshot.update(changes) : api.createSnapshot(changes),
      open: 0,
    };
    entries.add(head);
    if (previous) disposeWhenUnused(previous);
    return handle(head);
  };
  const close = () => {
    for (const entry of entries) {
      if (!entry.snapshot.isDisposed()) entry.snapshot.dispose();
    }
    entries.clear();
    head = undefined;
    api.close();
  };
  return new Proxy(api, {
    get(target, property) {
      if (property === "updateSnapshot") return updateSnapshot;
      if (property === "close") return close;
      const value = Reflect.get(target, property, target);
      return typeof value === "function" ? value.bind(target) : value;
    },
  });
};

/**
 * Create a backend API with the TypeScript 7.0 snapshot contract.
 *
 * @param {{ cwd?: string, fs?: Record<string, unknown> }} options
 */
export const createApi = (options = {}) => {
  const fs = options.fs ? { ...FS_CALLBACK_DEFAULTS, ...options.fs } : undefined;
  const api = new sync.API({ ...options, ...(fs ? { fs } : {}) });
  return typeof api.createSnapshot === "function" ? withLinearSnapshots(api) : api;
};

const INFERRED_PROJECT_ID = "/dev/null/inferred";

/**
 * Whether a project is the inferred project. Both versions give it the id
 * `/dev/null/inferred`. Only 7.0 also uses that id as its config file name.
 */
export const isInferredProject = (project) =>
  project.id.split(path.sep).join("/") === INFERRED_PROJECT_ID;

/** Config file name with the 7.0 value `/dev/null/inferred` for the inferred project. */
export const projectConfigFileName = (project) =>
  isInferredProject(project) ? INFERRED_PROJECT_ID : project.configFileName;
