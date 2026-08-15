import assert from "node:assert/strict";
import {
  chmodSync,
  cpSync,
  existsSync,
  mkdirSync,
  mkdtempSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { delimiter, dirname, join, resolve } from "node:path";
import { spawnSync } from "node:child_process";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

const REPO_ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");

const installForeignCodegenDependency = (root) => {
  const directory = join(root, "node_modules", "json-schema-to-typescript");
  mkdirSync(directory, { recursive: true });
  writeFileSync(
    join(directory, "package.json"),
    `${JSON.stringify({ name: "json-schema-to-typescript", version: "0.0.0" })}\n`,
  );
};

const installCargoMarker = (root, marker) => {
  const directory = join(root, "fake-bin");
  mkdirSync(directory, { recursive: true });
  if (process.platform === "win32") {
    writeFileSync(join(directory, "cargo.cmd"), `@echo called>"${marker}"\r\n@exit /b 97\r\n`);
  } else {
    const binary = join(directory, "cargo");
    writeFileSync(binary, `#!/bin/sh\nprintf called > "${marker}"\nexit 97\n`);
    chmodSync(binary, 0o755);
  }
  return directory;
};

test("generation validates local codegen resolution before Cargo", () => {
  const outer = mkdtempSync(join(tmpdir(), "fallow-generate-preflight-"));
  const checkout = join(outer, ".git-worktrees", "nested");
  const marker = join(outer, "cargo-called");
  mkdirSync(checkout, { recursive: true });
  cpSync(join(REPO_ROOT, "scripts"), join(checkout, "scripts"), { recursive: true });
  writeFileSync(
    join(checkout, "package.json"),
    `${JSON.stringify({ private: true, type: "module" })}\n`,
  );
  installForeignCodegenDependency(outer);
  const fakeBin = installCargoMarker(outer, marker);
  const env = { ...process.env, PATH: `${fakeBin}${delimiter}${process.env.PATH ?? ""}` };

  try {
    const help = spawnSync(process.execPath, ["scripts/generate-all.mjs", "--help"], {
      cwd: checkout,
      encoding: "utf8",
      env,
    });
    assert.equal(help.status, 0, `${help.stdout}\n${help.stderr}`);

    const generated = spawnSync(process.execPath, ["scripts/generate-all.mjs", "--check"], {
      cwd: checkout,
      encoding: "utf8",
      env,
    });
    assert.equal(generated.status, 1, `${generated.stdout}\n${generated.stderr}`);
    assert.match(`${generated.stdout}\n${generated.stderr}`, /outside this checkout/u);
    assert.equal(existsSync(marker), false, "Cargo must not run before the codegen preflight");
  } finally {
    rmSync(outer, { recursive: true, force: true });
  }
});
