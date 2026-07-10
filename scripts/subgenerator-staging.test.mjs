import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { existsSync, mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";

const temporaryRoot = (name) => mkdtempSync(join(tmpdir(), `${name}-`));
const hasVscodeCodegenDependencies = existsSync(
  join("editors", "vscode", "node_modules", "json-schema-to-typescript"),
);

test("NAPI type generation writes only under the requested output root", () => {
  const outputRoot = temporaryRoot("fallow-napi-stage");
  const committedPath = "crates/napi/index.d.ts";
  const before = readFileSync(committedPath);

  try {
    execFileSync("node", ["crates/napi/scripts/write-dts.mjs"], {
      env: { ...process.env, FALLOW_GENERATION_OUTPUT_ROOT: outputRoot },
      stdio: "pipe",
    });

    assert.deepEqual(readFileSync(committedPath), before);
    assert.deepEqual(readFileSync(join(outputRoot, committedPath)), before);
  } finally {
    rmSync(outputRoot, { force: true, recursive: true });
  }
});

test(
  "VS Code contract generation writes every artifact under the requested output root",
  {
    skip: hasVscodeCodegenDependencies ? false : "requires the VS Code codegen dependencies",
  },
  () => {
    const outputRoot = temporaryRoot("fallow-vscode-stage");
    const generatedPaths = [
      "editors/vscode/package.json",
      "editors/vscode/src/generated/issue-types.ts",
      "editors/vscode/src/generated/lsp-initialization-options.d.ts",
      "editors/vscode/src/generated/output-contract.d.ts",
      "npm/fallow/types/output-contract.d.ts",
    ];
    const before = new Map(generatedPaths.map((path) => [path, readFileSync(path)]));

    try {
      execFileSync("node", ["editors/vscode/scripts/codegen-contracts.mjs"], {
        env: {
          ...process.env,
          FALLOW_CODEGEN_CAPABILITY_SCHEMA: "npm/fallow/capabilities.json",
          FALLOW_CODEGEN_OUTPUT_SCHEMA: "docs/output-schema.json",
          FALLOW_GENERATION_OUTPUT_ROOT: outputRoot,
        },
        stdio: "pipe",
      });

      for (const path of generatedPaths) {
        assert.deepEqual(readFileSync(path), before.get(path), path);
        assert.deepEqual(readFileSync(join(outputRoot, path)), before.get(path), path);
      }
    } finally {
      rmSync(outputRoot, { force: true, recursive: true });
    }
  },
);
