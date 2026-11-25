import { existsSync, realpathSync } from "node:fs";
import { createRequire } from "node:module";
import path from "node:path";

import { selectBackend, supportedRange, supportsVersion } from "./backend-selection.mjs";
import { createReadScope } from "./read-scope.mjs";

const SELECTED = selectBackend();
const { version: resolvedVersion } = await import(SELECTED.package);
if (!supportsVersion(SELECTED, resolvedVersion)) {
  throw new Error(
    `${SELECTED.package} backend version ${resolvedVersion} is outside the supported range ${supportedRange(SELECTED)}`,
  );
}

/**
 * The selected backend. `version` is the resolved package version, which can
 * differ from the tested version in the protocol manifest.
 */
export const BACKEND = Object.freeze({ ...SELECTED, version: resolvedVersion });
// TypeScript 7.1 serves the API at `typescript/sync`. TypeScript 7.0 and its
// 7.1 dev builds serve it at `typescript/unstable/sync`.
const importApi = async (subpath) => {
  try {
    return await import(`${BACKEND.package}/${subpath}`);
  } catch (error) {
    if (error?.code !== "ERR_PACKAGE_PATH_NOT_EXPORTED") throw error;
    return import(`${BACKEND.package}/unstable/${subpath}`);
  }
};
export const sync = await importApi("sync");
export const ast = await importApi("ast/is");
export const { SyntaxKind, NodeFlags } = await importApi("ast");
const { serverFS } = await importApi("fs");

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
 * The `node_modules` directory that holds the backend package. The platform
 * package of the compiler, with its library files, is installed next to it.
 */
const COMPILER_DIRS = (() => {
  try {
    const manifest = createRequire(import.meta.url).resolve(`${BACKEND.package}/package.json`);
    const packageDir = path.dirname(realpathSync(manifest));
    return [packageDir, path.dirname(packageDir), path.dirname(path.dirname(packageDir))].filter(
      (dir) => path.basename(dir) === "node_modules" || dir === packageDir,
    );
  } catch {
    return [];
  }
})();

/**
 * The answer of each scoped file system callback, besides `readFile`, for a
 * path outside the read scope: a directory lists no entries.
 *
 * Each scoped callback costs one round trip from the checker for each call.
 * So the callbacks that give no file content (`fileExists`,
 * `directoryExists`, `stat` and `realpath`) go to the checker host. A link
 * that `realpath` resolves to a path outside the scope stays unreadable,
 * because `readFile` checks the resolved path of each file itself.
 */
const REFUSED = {
  getAccessibleEntries: () => ({ files: [], directories: [] }),
};

/**
 * Give `fs` callbacks that keep the checker inside the read scope of `root`
 * (see {@link createReadScope}). A refused file reads as missing. A path in
 * scope goes to the callback of the caller, or to the file system.
 */
export const withReadScope = (scope, fs = {}) => {
  const scoped = { ...fs };
  const readFile = fs.readFile ?? (() => READ_FROM_DISK);
  scoped.readFile = (fileName) => (scope.readable(fileName) ? readFile(fileName) : null);
  for (const [name, refused] of Object.entries(REFUSED)) {
    const inner = fs[name] ?? (() => FS_CALLBACK_DEFAULTS[name]);
    scoped[name] = (fileName) => (scope.inScope(fileName) ? inner(fileName) : refused(fileName));
  }
  return scoped;
};

/** Drop the path cache of `scope` before each snapshot update of `api`. */
const resettingScope = (api, scope) =>
  new Proxy(api, {
    get(target, property) {
      const value = Reflect.get(target, property, target);
      if (property === "updateSnapshot") {
        return (...args) => {
          scope.reset();
          return value.apply(target, args);
        };
      }
      return typeof value === "function" ? value.bind(target) : value;
    },
  });

/**
 * Create a backend API with the TypeScript 7.0 snapshot contract. With a
 * `cwd`, the checker touches only the paths in the read scope of that root.
 * `scopeRoot` overrides the scope root that fallow sets in the environment.
 *
 * @param {{ cwd?: string, scopeRoot?: string, fs?: Record<string, unknown> }} options
 */
export const createApi = (options = {}) => {
  const { scopeRoot, ...apiOptions } = options;
  const scope = options.cwd ? createReadScope(options.cwd, COMPILER_DIRS, scopeRoot) : null;
  const scoped = scope ? withReadScope(scope, options.fs) : options.fs;
  const fs = scoped ? { ...FS_CALLBACK_DEFAULTS, ...scoped } : undefined;
  const api = new sync.API({ ...apiOptions, ...(fs ? { fs } : {}) });
  const linear = typeof api.createSnapshot === "function" ? withLinearSnapshots(api) : api;
  return scope ? resettingScope(linear, scope) : linear;
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

/**
 * Name node of a heritage clause type. TypeScript 7.0 wraps it in an
 * ExpressionWithTypeArguments (`expression`). TypeScript 7.1 uses a
 * TypeReference (`typeName`) in `implements` and interface `extends` clauses.
 */
export const heritageTypeName = (heritageType) => heritageType.expression ?? heritageType.typeName;

/**
 * Compiler options of a project. TypeScript 7.1 beta moves them from
 * `compilerOptions` to `parsedCommandLine.options`.
 */
export const projectCompilerOptions = (project) =>
  project.compilerOptions ?? project.parsedCommandLine?.options ?? {};
