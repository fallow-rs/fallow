import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import {
  packageGroup,
  platformLabel,
  platformPackageDirs,
  preparePlatformPackageDocs,
  renderReadme,
} from "./prepare-platform-package-docs.mjs";

const LONG_DASHES = [0x2013, 0x2014].map((code) => String.fromCodePoint(code));

const fixtureRoot = (t) => {
  const root = mkdtempSync(join(tmpdir(), "fallow-platform-docs-"));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  writeFileSync(join(root, "LICENSE"), "MIT License\n");
  const add = (rel, manifest) => {
    mkdirSync(join(root, rel), { recursive: true });
    writeFileSync(join(root, rel, "package.json"), JSON.stringify(manifest));
  };
  add("npm/fallow", { name: "fallow" });
  add("npm/linux-x64-musl", {
    name: "@fallow-cli/linux-x64-musl",
    os: ["linux"],
    cpu: ["x64"],
    libc: ["musl"],
  });
  add("npm/fallow-similar-code-darwin-arm64", {
    name: "@fallow-cli/fallow-similar-code-darwin-arm64",
    os: ["darwin"],
    cpu: ["arm64"],
  });
  add("crates/napi", { name: "@fallow-cli/fallow-node" });
  add("crates/napi/npm/win32-x64-msvc", {
    name: "@fallow-cli/fallow-node-win32-x64-msvc",
    os: ["win32"],
    cpu: ["x64"],
  });
  return root;
};

test("labels name the operating system, the CPU and the C library", () => {
  assert.equal(
    platformLabel({ name: "x", os: ["darwin"], cpu: ["arm64"] }),
    "macOS ARM64 (Apple Silicon)",
  );
  assert.equal(platformLabel({ name: "x", os: ["darwin"], cpu: ["x64"] }), "macOS x64 (Intel)");
  assert.equal(
    platformLabel({ name: "x", os: ["linux"], cpu: ["x64"], libc: ["glibc"] }),
    "Linux x64 (glibc)",
  );
  assert.equal(
    platformLabel({ name: "@fallow-cli/linux-x64-musl", os: ["linux"], cpu: ["x64"] }),
    "Linux x64 (musl)",
  );
  assert.equal(
    platformLabel({ name: "@fallow-cli/linux-arm64-gnu", os: ["linux"], cpu: ["arm64"] }),
    "Linux ARM64 (glibc)",
  );
  assert.equal(platformLabel({ name: "x", os: ["win32"], cpu: ["arm64"] }), "Windows ARM64");
  assert.throws(
    () => platformLabel({ name: "x", os: ["freebsd"], cpu: ["x64"] }),
    /unsupported os\/cpu/,
  );
});

test("only platform packages get a group", () => {
  assert.equal(packageGroup("@fallow-cli/darwin-arm64"), "cli");
  assert.equal(packageGroup("@fallow-cli/fallow-similar-code-linux-x64-gnu"), "similar-code");
  assert.equal(packageGroup("@fallow-cli/fallow-node-win32-x64-msvc"), "napi");
  assert.equal(packageGroup("@fallow-cli/fallow-node"), null);
  assert.equal(packageGroup("fallow"), null);
  assert.equal(packageGroup("fallow-similar-code"), null);
});

test("a README claims verification only where a verifier exists", () => {
  const cli = renderReadme({
    name: "@fallow-cli/linux-x64-gnu",
    os: ["linux"],
    cpu: ["x64"],
    libc: ["glibc"],
  });
  assert.match(cli, /^# @fallow-cli\/linux-x64-gnu\n/);
  assert.match(cli, /Linux x64 \(glibc\) binaries/);
  assert.match(cli, /## Verification/);
  assert.match(cli, /npm install --save-dev fallow\n/);

  const napi = renderReadme({
    name: "@fallow-cli/fallow-node-darwin-x64",
    os: ["darwin"],
    cpu: ["x64"],
  });
  assert.doesNotMatch(napi, /## Verification/);
  assert.match(napi, /npm install @fallow-cli\/fallow-node\n/);
  for (const dash of LONG_DASHES) {
    assert.ok(!(cli + napi).includes(dash), "no long dashes in generated READMEs");
  }
});

test("write mode creates READMEs and the license copy covers every platform package and the addon root", (t) => {
  const root = fixtureRoot(t);
  const { dirs } = preparePlatformPackageDocs({ root, license: true });
  assert.equal(dirs.length, 3);
  for (const dir of dirs) {
    assert.ok(existsSync(join(dir, "README.md")), `${dir} has a README`);
    assert.equal(readFileSync(join(dir, "LICENSE"), "utf8"), "MIT License\n");
  }
  assert.ok(existsSync(join(root, "crates/napi/LICENSE")));
  assert.ok(
    !existsSync(join(root, "npm/fallow/README.md")),
    "the wrapper package keeps its own README",
  );
});

test("check mode reports a missing or edited README and writes nothing", (t) => {
  const root = fixtureRoot(t);
  preparePlatformPackageDocs({ root });
  writeFileSync(join(root, "npm/linux-x64-musl/README.md"), "edited\n");
  const { drift } = preparePlatformPackageDocs({ root, check: true });
  assert.deepEqual(drift, [join(root, "npm/linux-x64-musl/README.md")]);
  assert.equal(readFileSync(join(root, "npm/linux-x64-musl/README.md"), "utf8"), "edited\n");
});

test("the script starts as a process and checks the tracked packages", () => {
  const script = new URL("./prepare-platform-package-docs.mjs", import.meta.url);
  const result = spawnSync(process.execPath, [script.pathname, "--check"], { encoding: "utf8" });
  assert.equal(result.status, 0, result.stderr);
  assert.match(result.stdout, /^checked \d+ platform package\(s\)\n$/);
});

test("the tracked platform package READMEs match the generator", () => {
  const { dirs, drift } = preparePlatformPackageDocs({ check: true });
  assert.ok(platformPackageDirs().length >= 16, "found the CLI and similar-code platform packages");
  assert.ok(dirs.length > 0);
  assert.deepEqual(drift, [], "run node .github/scripts/prepare-platform-package-docs.mjs");
});
