import {
  copyFileSync,
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  readdirSync,
  renameSync,
  rmSync,
  statSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, relative, resolve, sep } from "node:path";

const normalizedRelativePath = (path) => path.split(sep).join("/");

const assertSurfacePaths = (surfacePaths) => {
  const unique = new Set();
  for (const path of surfacePaths) {
    const resolved = resolve("/surface-root", path);
    const relativePath = normalizedRelativePath(relative("/surface-root", resolved));
    if (
      path !== relativePath ||
      path === "" ||
      path.includes("*") ||
      path.includes("?") ||
      relativePath.startsWith("../")
    ) {
      throw new Error(`generated surface path must be a concrete relative path: ${path}`);
    }
    if (unique.has(path)) {
      throw new Error(`duplicate generated surface path: ${path}`);
    }
    unique.add(path);
  }
  return [...unique].toSorted();
};

const listFiles = (root, directory = root) => {
  const files = [];
  for (const entry of readdirSync(directory, { withFileTypes: true })) {
    const path = join(directory, entry.name);
    if (entry.isDirectory()) {
      files.push(...listFiles(root, path));
    } else if (entry.isFile()) {
      files.push(normalizedRelativePath(relative(root, path)));
    } else {
      throw new Error(`staging contains an unsupported file type: ${path}`);
    }
  }
  return files.toSorted();
};

const validateStagedSurfaceSet = (stagingRoot, surfacePaths) => {
  const expected = assertSurfacePaths(surfacePaths);
  const actual = listFiles(stagingRoot);
  const actualSet = new Set(actual);
  const expectedSet = new Set(expected);
  const missing = expected.filter((path) => !actualSet.has(path));
  const undeclared = actual.filter((path) => !expectedSet.has(path));

  if (missing.length === 0 && undeclared.length === 0) {
    return;
  }

  const problems = [];
  if (missing.length > 0) {
    problems.push(`missing staged surfaces: ${missing.join(", ")}`);
  }
  if (undeclared.length > 0) {
    problems.push(`undeclared staged surfaces: ${undeclared.join(", ")}`);
  }
  throw new Error(problems.join("\n"));
};

const hasSameContents = (left, right) => {
  if (!existsSync(left) || !statSync(left).isFile()) {
    return false;
  }
  return readFileSync(left).equals(readFileSync(right));
};

const driftedSurfacePaths = (repoRoot, stagingRoot, surfacePaths) =>
  surfacePaths.filter((path) => !hasSameContents(join(repoRoot, path), join(stagingRoot, path)));

const replaceFile = (source, destination, suffix) => {
  mkdirSync(dirname(destination), { recursive: true });
  const temporary = `${destination}.fallow-contract-${process.pid}-${suffix}`;
  try {
    copyFileSync(source, temporary);
    renameSync(temporary, destination);
  } finally {
    rmSync(temporary, { force: true });
  }
};

const restoreDestinations = ({ backupRoot, repoRoot, paths }) => {
  const failures = [];
  paths.forEach((path, index) => {
    const backup = join(backupRoot, path);
    const destination = join(repoRoot, path);
    try {
      if (existsSync(backup)) {
        replaceFile(backup, destination, `rollback-${index}`);
      } else {
        rmSync(destination, { force: true });
      }
    } catch (error) {
      failures.push(`${path}: ${error.message}`);
    }
  });
  return failures;
};

const promoteStagedSurfaces = ({ backupRoot, beforePromote, paths, repoRoot, stagingRoot }) => {
  for (const path of paths) {
    const destination = join(repoRoot, path);
    if (existsSync(destination)) {
      const backup = join(backupRoot, path);
      mkdirSync(dirname(backup), { recursive: true });
      copyFileSync(destination, backup);
    }
  }

  try {
    paths.forEach((path, index) => {
      beforePromote?.(path, index);
      replaceFile(join(stagingRoot, path), join(repoRoot, path), index);
    });
  } catch (error) {
    const rollbackFailures = restoreDestinations({ backupRoot, repoRoot, paths });
    if (rollbackFailures.length > 0) {
      throw new Error(`${error.message}\nrollback failed: ${rollbackFailures.join(", ")}`, {
        cause: error,
      });
    }
    throw error;
  }
};

export const runGenerationTransaction = ({
  beforePromote,
  check = false,
  generate,
  repoRoot,
  stagingParent = tmpdir(),
  surfacePaths,
  validate,
}) => {
  const paths = assertSurfacePaths(surfacePaths);
  const transactionRoot = mkdtempSync(join(stagingParent, "fallow-generate-all-"));
  const stagingRoot = join(transactionRoot, "output");
  const backupRoot = join(transactionRoot, "backup");
  mkdirSync(stagingRoot);
  mkdirSync(backupRoot);

  try {
    generate(stagingRoot);
    validateStagedSurfaceSet(stagingRoot, paths);
    validate?.(stagingRoot);
    validateStagedSurfaceSet(stagingRoot, paths);
    const driftedPaths = driftedSurfacePaths(repoRoot, stagingRoot, paths);
    if (!check && driftedPaths.length > 0) {
      promoteStagedSurfaces({
        backupRoot,
        beforePromote,
        paths: driftedPaths,
        repoRoot,
        stagingRoot,
      });
    }
    return { driftedPaths };
  } finally {
    rmSync(transactionRoot, { force: true, recursive: true });
  }
};
