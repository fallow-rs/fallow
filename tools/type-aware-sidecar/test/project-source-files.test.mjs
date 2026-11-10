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
  return { project: { program }, fetched };
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
