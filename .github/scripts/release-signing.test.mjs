import assert from "node:assert/strict";
import { generateKeyPairSync, verify } from "node:crypto";
import { spawnSync } from "node:child_process";
import {
  copyFileSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  readdirSync,
  rmSync,
  symlinkSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";

const ROOT = new URL("../../", import.meta.url);
const workflow = readFileSync(new URL(".github/workflows/release.yml", ROOT), "utf8");
const binaryNames = [
  "fallow",
  "fallow-multicall",
  "fallow-lsp",
  "fallow-mcp",
  "fallow-similar-code",
];

const workflowCommand = (name) => {
  const start = workflow.indexOf(`      - name: ${name}\n`);
  assert.notEqual(start, -1, `missing release step: ${name}`);
  const end = workflow.indexOf("\n      - ", start + 1);
  const step = workflow.slice(start, end === -1 ? undefined : end);
  const run = step.match(/        run: \|\n((?:          .*\n|\n)+)/u);
  assert.ok(run, `missing command for release step: ${name}`);
  return run[1]
    .split("\n")
    .map((line) => line.slice(10))
    .join("\n");
};

const prepare = (t, platform, ext) => {
  const cwd = mkdtempSync(join(tmpdir(), "fallow-release-signing-"));
  t.after(() => rmSync(cwd, { recursive: true, force: true }));
  mkdirSync(join(cwd, "unsigned-binaries"));
  mkdirSync(join(cwd, ".github/scripts"), { recursive: true });
  for (const script of ["sign-binary.mjs", "verify-binary.mjs"]) {
    copyFileSync(new URL(`.github/scripts/${script}`, ROOT), join(cwd, ".github/scripts", script));
  }
  const names = binaryNames.map((name) => `${name}-${platform}${ext}`);
  for (const name of names) {
    // Executable-looking data must only be read and signed, never executed.
    writeFileSync(join(cwd, "unsigned-binaries", name), `#!/bin/sh\ntouch EXECUTED\n# ${name}\n`);
  }
  const { privateKey, publicKey } = generateKeyPairSync("ed25519");
  const env = {
    ...process.env,
    FALLOW_NPM_DIR: platform,
    FALLOW_BINARY_EXT: ext,
    ED25519_BINARY_SIGNING_PRIVATE_KEY: privateKey
      .export({ format: "der", type: "pkcs8" })
      .subarray(-32)
      .toString("base64"),
    ED25519_BINARY_SIGNING_PUBLIC_KEY: publicKey
      .export({ format: "der", type: "spki" })
      .subarray(-32)
      .toString("base64"),
  };
  const run = (name) =>
    spawnSync("bash", ["-euo", "pipefail", "-c", workflowCommand(name)], {
      cwd,
      env,
      encoding: "utf8",
    });
  return { cwd, names, publicKey, env, run };
};

for (const [platform, ext] of [
  ["linux-x64-gnu", ""],
  ["win32-x64-msvc", ".exe"],
]) {
  test(`release signing preserves ${platform} bytes and npm signature names`, (t) => {
    const { cwd, names, publicKey, env, run } = prepare(t, platform, ext);
    const originals = names.map((name) => readFileSync(join(cwd, "unsigned-binaries", name)));
    for (const step of [
      "Validate unsigned binary inventory",
      "Sign raw binaries",
      "Stage npm platform payload (multicall binary as fallow)",
    ]) {
      const result = run(step);
      assert.equal(result.status, 0, result.stderr);
    }
    for (const [index, name] of names.entries()) {
      const binary = readFileSync(join(cwd, "unsigned-binaries", name));
      assert.deepEqual(binary, originals[index]);
      assert.ok(
        verify(
          null,
          binary,
          publicKey,
          readFileSync(join(cwd, "unsigned-binaries", `${name}.sig`)),
        ),
        name,
      );
    }
    const multicall = readFileSync(join(cwd, "npm-payload", `fallow${ext}`));
    assert.deepEqual(multicall, originals[1]);
    assert.ok(
      verify(
        null,
        multicall,
        publicKey,
        readFileSync(join(cwd, "npm-payload", `fallow${ext}.sig`)),
      ),
    );
    const similarCode = readFileSync(join(cwd, "npm-payload", `fallow-similar-code${ext}`));
    assert.deepEqual(similarCode, originals[4]);
    assert.ok(
      verify(
        null,
        similarCode,
        publicKey,
        readFileSync(join(cwd, "npm-payload", `fallow-similar-code${ext}.sig`)),
      ),
    );
    assert.deepEqual(
      readdirSync(join(cwd, "npm-payload")).toSorted(),
      [
        `fallow${ext}`,
        `fallow${ext}.sig`,
        `fallow-similar-code${ext}`,
        `fallow-similar-code${ext}.sig`,
      ].toSorted(),
    );
    assert.ok(!readdirSync(cwd).includes("EXECUTED"));
    const altered = Buffer.concat([multicall, Buffer.from("tampered")]);
    assert.equal(
      verify(null, altered, publicKey, readFileSync(join(cwd, "npm-payload", `fallow${ext}.sig`))),
      false,
    );
    const unrelated = generateKeyPairSync("ed25519").publicKey;
    env.ED25519_BINARY_SIGNING_PUBLIC_KEY = unrelated
      .export({ format: "der", type: "spki" })
      .subarray(-32)
      .toString("base64");
    const wrongKey = run("Sign raw binaries");
    assert.notEqual(wrongKey.status, 0);
    assert.match(wrongKey.stderr, /ed25519 verification FAILED/u);
  });
}

for (const mutation of ["missing", "extra", "empty", "symlink", "directory"]) {
  test(`unsigned release inventory rejects ${mutation} files before signing`, (t) => {
    const { cwd, names, run } = prepare(t, "linux-x64-gnu", "");
    assert.equal(run("Validate unsigned binary inventory").status, 0);
    const path = join(cwd, "unsigned-binaries", names[0]);
    if (mutation === "extra") {
      writeFileSync(join(cwd, "unsigned-binaries", "preinstall.mjs"), "process.exit(0)");
    } else if (mutation === "empty") {
      writeFileSync(path, "");
    } else {
      rmSync(path);
      if (mutation === "symlink") symlinkSync(names[1], path);
      if (mutation === "directory") mkdirSync(path);
    }
    const result = run("Validate unsigned binary inventory");
    assert.notEqual(result.status, 0);
    assert.match(
      result.stderr,
      /Unexpected unsigned binary inventory|Expected a nonempty regular binary/u,
    );
    assert.ok(readdirSync(join(cwd, "unsigned-binaries")).every((name) => !name.endsWith(".sig")));
  });
}
