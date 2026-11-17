import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";
import { gunzipSync, gzipSync } from "node:zlib";

import {
  BUNDLE_PLATFORMS,
  LAUNCHER,
  assetName,
  buildBundle,
  checkIntegrity,
  readTar,
  writeTar,
} from "./type-aware-bundle.mjs";

const npmTarball = (files) =>
  gzipSync(
    writeTar(
      files.map(([path, data, executable]) => ({ path, data: Buffer.from(data), executable })),
    ),
  );

const integrity = (data) => `sha512-${createHash("sha512").update(data).digest("base64")}`;

const fixture = (t, { tamper = false, platform = { platform: "linux", arch: "x64" } } = {}) => {
  const dir = mkdtempSync(join(tmpdir(), "fallow-type-aware-bundle-"));
  t.after(() => rmSync(dir, { recursive: true, force: true }));
  const sidecar = join(dir, "sidecar");
  mkdirSync(join(sidecar, "src"), { recursive: true });
  writeFileSync(join(sidecar, "fallow-type-aware.mjs"), "#!/usr/bin/env node\n");
  writeFileSync(join(sidecar, "src", "cli.mjs"), "export const run = () => {};\n");
  writeFileSync(join(sidecar, "LICENSE"), "MIT\n");
  writeFileSync(
    join(sidecar, "package.json"),
    JSON.stringify({
      name: "fallow-type-aware",
      version: "9.8.7",
      type: "module",
      files: ["fallow-type-aware.mjs", "src"],
      bin: { "fallow-type-aware": "fallow-type-aware.mjs" },
      dependencies: { typescript: "7.0.2" },
      devDependencies: { tinybench: "1.0.0" },
    }),
  );
  const typescript = npmTarball([
    ["package/package.json", '{"name":"typescript"}'],
    ["package/LICENSE", "Apache-2.0"],
  ]);
  const native = npmTarball([
    ["package/lib/tsc", "binary", true],
    ["package/LICENSE", "Apache-2.0"],
    ["package/NOTICE.txt", "notices"],
  ]);
  const urls = {
    "https://registry.example/typescript.tgz": typescript,
    "https://registry.example/native.tgz": tamper ? Buffer.from("changed") : native,
  };
  writeFileSync(
    join(sidecar, "package-lock.json"),
    JSON.stringify({
      packages: {
        "node_modules/typescript": {
          version: "7.0.2",
          resolved: "https://registry.example/typescript.tgz",
          integrity: integrity(typescript),
        },
        [`node_modules/@typescript/typescript-${platform.platform}-${platform.arch}`]: {
          version: "7.0.2",
          resolved: "https://registry.example/native.tgz",
          integrity: integrity(native),
        },
      },
    }),
  );
  const protocolPath = join(dir, "protocol.json");
  writeFileSync(protocolPath, JSON.stringify({ wire_protocol_version: 42 }));
  const download = async (url) => {
    assert.ok(url in urls, `unexpected download ${url}`);
    return urls[url];
  };
  return { sidecarDir: sidecar, protocolPath, download };
};

test("a bundle holds the launcher, the manifest, the companion and both TypeScript packages", async (t) => {
  const bundle = await buildBundle({ platform: "linux", arch: "x64", ...fixture(t) });
  assert.equal(bundle.name, "fallow-type-aware-linux-x64.tar.gz");
  const files = new Map(readTar(gunzipSync(bundle.archive)).map((file) => [file.path, file]));
  assert.deepEqual([...files.keys()].toSorted(), [
    "THIRD_PARTY_NOTICES.md",
    "bundle.json",
    "fallow-type-aware",
    "lib/LICENSE",
    "lib/fallow-type-aware.mjs",
    "lib/node_modules/@typescript/typescript-linux-x64/LICENSE",
    "lib/node_modules/@typescript/typescript-linux-x64/NOTICE.txt",
    "lib/node_modules/@typescript/typescript-linux-x64/lib/tsc",
    "lib/node_modules/typescript/LICENSE",
    "lib/node_modules/typescript/package.json",
    "lib/package.json",
    "lib/src/cli.mjs",
    "platform-check.cjs",
  ]);
  assert.equal(files.get("fallow-type-aware").data.toString(), LAUNCHER);
  assert.ok(files.get("fallow-type-aware").executable);
  assert.ok(files.get("lib/node_modules/@typescript/typescript-linux-x64/lib/tsc").executable);
  assert.deepEqual(JSON.parse(files.get("bundle.json").data), {
    schema_version: 1,
    fallow_version: "9.8.7",
    wire_protocol_version: 42,
    platform: "linux",
    arch: "x64",
    typescript_version: "7.0.2",
  });
  const runtime = JSON.parse(files.get("lib/package.json").data);
  assert.equal(runtime.devDependencies, undefined);
  assert.equal(runtime.type, "module");
  assert.match(files.get("THIRD_PARTY_NOTICES.md").data.toString(), /Apache License 2\.0/u);
});

test("a bundle is byte-for-byte reproducible", async (t) => {
  const inputs = fixture(t);
  const first = await buildBundle({ platform: "linux", arch: "x64", ...inputs });
  const second = await buildBundle({ platform: "linux", arch: "x64", ...inputs });
  assert.deepEqual(first.archive, second.archive);
});

test("a package that does not match the lockfile integrity stops the build", async (t) => {
  await assert.rejects(
    buildBundle({ platform: "linux", arch: "x64", ...fixture(t, { tamper: true }) }),
    /integrity mismatch/u,
  );
  assert.throws(() => checkIntegrity(Buffer.from("x"), "sha1-abc", "weak"), /weak integrity/u);
});

test("only the Node platforms of the setup command get a bundle", async (t) => {
  await assert.rejects(
    buildBundle({ platform: "win32", arch: "x64", ...fixture(t) }),
    /no bundle for win32-x64/u,
  );
  const rust = readFileSync(
    new URL("../crates/api/src/type_aware/setup_store.rs", import.meta.url),
    "utf8",
  );
  for (const platform of BUNDLE_PLATFORMS) {
    assert.match(
      rust,
      new RegExp(`platform: "${platform.platform}",\\s+arch: "${platform.arch}",`, "u"),
      `${assetName(platform)} must be a bundle platform in Rust too`,
    );
  }
});

test("the archive reader rejects links", () => {
  const tar = writeTar([{ path: "a", data: Buffer.from("x") }]);
  tar[156] = "2".charCodeAt(0);
  let sum = 0;
  tar.fill(0x20, 148, 156);
  for (const byte of tar.subarray(0, 512)) {
    sum += byte;
  }
  tar.write(`${sum.toString(8).padStart(6, "0")}\0 `, 148, 8, "ascii");
  assert.throws(() => readTar(tar), /unsupported tar entry type 2/u);
});

test("the script starts its build when run as a process", (t) => {
  const out = mkdtempSync(join(tmpdir(), "fallow-type-aware-bundle-out-"));
  t.after(() => rmSync(out, { recursive: true, force: true }));
  const result = spawnSync(
    process.execPath,
    [
      fileURLToPath(new URL("./type-aware-bundle.mjs", import.meta.url)),
      "--out",
      out,
      "--platform",
      "win32-x64",
    ],
    { encoding: "utf8" },
  );
  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /no bundle for win32-x64/u);
});

const unpackBundle = (t, archive) => {
  const dir = mkdtempSync(join(tmpdir(), "fallow-type-aware-unpacked-"));
  t.after(() => rmSync(dir, { recursive: true, force: true }));
  for (const file of readTar(gunzipSync(archive))) {
    mkdirSync(dirname(join(dir, file.path)), { recursive: true });
    writeFileSync(join(dir, file.path), file.data, { mode: file.executable ? 0o755 : 0o644 });
  }
  writeFileSync(
    join(dir, "lib", "fallow-type-aware.mjs"),
    'process.stdout.write("companion started");\n',
  );
  return dir;
};

test(
  "the launcher refuses a Node of another platform than the bundle",
  { skip: process.platform === "win32" },
  async (t) => {
    const platform = { platform: process.platform, arch: process.arch };
    if (
      !BUNDLE_PLATFORMS.some(
        (entry) => entry.platform === platform.platform && entry.arch === platform.arch,
      )
    ) {
      t.skip("no bundle for the platform of this runner");
      return;
    }
    const bundle = await buildBundle({ ...platform, ...fixture(t, { platform }) });
    const dir = unpackBundle(t, bundle.archive);
    const launch = () => spawnSync(join(dir, "fallow-type-aware"), [], { encoding: "utf8" });

    const same = launch();
    assert.equal(same.status, 0, same.stderr);
    assert.equal(same.stdout, "companion started");

    const manifestPath = join(dir, "bundle.json");
    const manifest = JSON.parse(readFileSync(manifestPath, "utf8"));
    const otherArch = manifest.arch === "x64" ? "arm64" : "x64";
    writeFileSync(manifestPath, JSON.stringify({ ...manifest, arch: otherArch }));
    const other = launch();
    assert.equal(other.status, 2);
    assert.equal(other.stdout, "");
    assert.match(other.stderr, new RegExp(`${manifest.platform}-${otherArch}`, "u"));
    assert.match(other.stderr, /the node on PATH/u);
    assert.match(other.stderr, /fallow type-aware setup/u);

    for (const broken of [undefined, "{not json"]) {
      if (broken === undefined) {
        rmSync(manifestPath);
      } else {
        writeFileSync(manifestPath, broken);
      }
      const unreadable = launch();
      assert.equal(unreadable.status, 2, unreadable.stderr);
      assert.match(unreadable.stderr, /cannot read bundle\.json/u);
      assert.match(unreadable.stderr, /fallow type-aware setup/u);
      assert.doesNotMatch(unreadable.stderr, /\n\s+at /u, "no stack trace");
    }
  },
);
