import assert from "node:assert/strict";
import test from "node:test";

import { projectSourceFiles } from "../src/semantic-identity.mjs";

const FILE_NAMES = [
  "/repo/src/service.ts",
  "/repo/src/view.tsx",
  "/repo/src/types.d.ts",
  "/repo/src/styles.d.css.ts",
  "/repo/src/data.ts",
  "/repo/node_modules/pkg/index.ts",
  "/repo/node_modules/@types/node/index.d.ts",
];

const stubProject = () => {
  const fetched = [];
  const program = {
    getSourceFileNames: () => FILE_NAMES,
    getSourceFile: (fileName) => {
      fetched.push(fileName);
      return { fileName, isDeclarationFile: false };
    },
    isSourceFileDefaultLibrary: () => false,
    isSourceFileFromExternalLibrary: () => false,
  };
  return {
    project: {
      id: "/repo/tsconfig.json",
      configFileName: "/repo/tsconfig.json",
      program,
      rootFiles: [],
    },
    fetched,
  };
};

test("project source files skip node_modules and declaration files before a transfer", () => {
  const { project, fetched } = stubProject();
  assert.deepEqual(
    projectSourceFiles(project).map(({ fileName }) => fileName),
    ["/repo/src/service.ts", "/repo/src/view.tsx", "/repo/src/data.ts"],
  );
  assert.deepEqual(fetched, ["/repo/src/service.ts", "/repo/src/view.tsx", "/repo/src/data.ts"]);
});

test("project source files are read one time for each program", () => {
  const first = stubProject();
  projectSourceFiles(first.project);
  projectSourceFiles(first.project);
  assert.equal(first.fetched.length, 3);

  const next = stubProject();
  projectSourceFiles(next.project);
  assert.equal(next.fetched.length, 3, "a new program reads its files again");
});

const stubFor = (fileNames, { configFileName = "/repo/tsconfig.json", rootFiles = [] } = {}) => {
  const fetched = [];
  const program = {
    getSourceFileNames: () => fileNames,
    getSourceFile: (fileName) => {
      fetched.push(fileName);
      return { fileName, isDeclarationFile: /\.d\.(?:[cm]?ts|[^./\\]+\.ts)$/u.test(fileName) };
    },
    isSourceFileDefaultLibrary: () => false,
    isSourceFileFromExternalLibrary: () => false,
  };
  return { project: { id: configFileName, program, configFileName, rootFiles }, fetched };
};

test("project source files keep files below node_modules that belong to the project", () => {
  const nested = stubFor(
    ["/x/node_modules/app/src/use.ts", "/x/node_modules/app/node_modules/dep/index.ts"],
    {
      configFileName: "/x/node_modules/app/tsconfig.json",
    },
  );
  assert.deepEqual(
    projectSourceFiles(nested.project).map(({ fileName }) => fileName),
    ["/x/node_modules/app/src/use.ts"],
  );
  const included = stubFor(
    ["/repo/node_modules/local/user.ts", "/repo/node_modules/dep/index.ts"],
    {
      rootFiles: ["/repo/node_modules/local/user.ts"],
    },
  );
  assert.deepEqual(
    projectSourceFiles(included.project).map(({ fileName }) => fileName),
    ["/repo/node_modules/local/user.ts"],
  );
});

test("project source files match declaration file names case-sensitively", () => {
  const { project } = stubFor([
    "/repo/src/Upper.D.ts",
    "/repo/src/low.D.mts",
    "/repo/src/lower.d.ts",
  ]);
  assert.deepEqual(
    projectSourceFiles(project).map(({ fileName }) => fileName),
    ["/repo/src/Upper.D.ts", "/repo/src/low.D.mts"],
  );
});
