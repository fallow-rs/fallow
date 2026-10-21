import assert from "node:assert/strict";
import {
  chmodSync,
  existsSync,
  lstatSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  readdirSync,
  rmSync,
  symlinkSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import test from "node:test";

import { packageTypeAware } from "./package-type-aware.mjs";
import { packageVsixVariants } from "./package-vsix-variants.mjs";
import { normalizedPayload, parseVsixTargetPlatform, sha256File } from "./vsix-archive.mjs";
import {
  createInventory,
  INVENTORY_FILENAME,
  validateInventory,
  writeInventory,
} from "./vsix-inventory.mjs";
import {
  getVsixVariant,
  TYPE_AWARE_BACKENDS,
  TYPE_AWARE_SIDECAR_FILES,
  VSIX_VARIANTS,
  vsixFilename,
} from "./vsix-targets.mjs";
import {
  assertVsixTargetPlatform,
  verifyPackagedTypeAware,
} from "./verify-packaged-type-aware.mjs";

const VERSION = "1.2.3";
const BACKEND_VERSION = "7.0.2";

const write = (path, contents = "fixture\n") => {
  mkdirSync(dirname(path), { recursive: true });
  writeFileSync(path, contents);
};

const writeJson = (path, value) => write(path, `${JSON.stringify(value, null, 2)}\n`);

const crc32 = (contents) => {
  let crc = 0xffffffff;
  for (const byte of contents) {
    crc ^= byte;
    for (let bit = 0; bit < 8; bit += 1) {
      crc = (crc >>> 1) ^ (crc & 1 ? 0xedb88320 : 0);
    }
  }
  return (crc ^ 0xffffffff) >>> 0;
};

const writeStoredZip = (path, files) => {
  const localParts = [];
  const centralParts = [];
  let offset = 0;
  for (const file of files) {
    const name = Buffer.from(file.path, "utf8");
    const contents = Buffer.isBuffer(file.contents)
      ? file.contents
      : Buffer.from(file.contents, "utf8");
    const checksum = crc32(contents);
    const local = Buffer.alloc(30);
    local.writeUInt32LE(0x04034b50, 0);
    local.writeUInt16LE(20, 4);
    local.writeUInt16LE(0x800, 6);
    local.writeUInt16LE(0, 8);
    local.writeUInt16LE(0, 10);
    local.writeUInt16LE(0x21, 12);
    local.writeUInt32LE(checksum, 14);
    local.writeUInt32LE(contents.length, 18);
    local.writeUInt32LE(contents.length, 22);
    local.writeUInt16LE(name.length, 26);
    local.writeUInt16LE(0, 28);
    localParts.push(local, name, contents);

    const central = Buffer.alloc(46);
    central.writeUInt32LE(0x02014b50, 0);
    central.writeUInt16LE(0x0314, 4);
    central.writeUInt16LE(20, 6);
    central.writeUInt16LE(0x800, 8);
    central.writeUInt16LE(0, 10);
    central.writeUInt16LE(0, 12);
    central.writeUInt16LE(0x21, 14);
    central.writeUInt32LE(checksum, 16);
    central.writeUInt32LE(contents.length, 20);
    central.writeUInt32LE(contents.length, 24);
    central.writeUInt16LE(name.length, 28);
    central.writeUInt16LE(0, 30);
    central.writeUInt16LE(0, 32);
    central.writeUInt16LE(0, 34);
    central.writeUInt16LE(0, 36);
    central.writeUInt32LE(((0o100000 | file.mode) << 16) >>> 0, 38);
    central.writeUInt32LE(offset, 42);
    centralParts.push(central, name);
    offset += local.length + name.length + contents.length;
  }

  const centralDirectory = Buffer.concat(centralParts);
  const end = Buffer.alloc(22);
  end.writeUInt32LE(0x06054b50, 0);
  end.writeUInt16LE(0, 4);
  end.writeUInt16LE(0, 6);
  end.writeUInt16LE(files.length, 8);
  end.writeUInt16LE(files.length, 10);
  end.writeUInt32LE(centralDirectory.length, 12);
  end.writeUInt32LE(offset, 16);
  end.writeUInt16LE(0, 20);
  writeFileSync(path, Buffer.concat([...localParts, centralDirectory, end]));
};

const fixture = (context) => {
  const root = mkdtempSync(join(tmpdir(), "fallow-vsix-test-"));
  context.after(() => rmSync(root, { recursive: true, force: true }));
  const repoRoot = join(root, "repo");
  const extensionRoot = join(repoRoot, "editors", "vscode");
  const storeNodeModules = join(root, "store", "node_modules");
  const optionalDependencies = Object.fromEntries(
    TYPE_AWARE_BACKENDS.map(({ packageName }) => [packageName, BACKEND_VERSION]),
  );

  writeJson(join(repoRoot, "crates", "api", "type-aware-protocol.json"), {
    backend: { version: BACKEND_VERSION },
  });
  write(join(repoRoot, "tools", "type-aware-sidecar", "fallow-type-aware.mjs"));
  write(join(repoRoot, "tools", "type-aware-sidecar", "src", "windows-child-process.mjs"));
  writeJson(join(repoRoot, "tools", "type-aware-sidecar", "package.json"), {
    name: "fallow-type-aware",
    version: VERSION,
  });
  writeJson(join(storeNodeModules, "typescript", "package.json"), {
    name: "typescript",
    version: BACKEND_VERSION,
    optionalDependencies,
  });
  write(join(storeNodeModules, "typescript", "lib", "typescript.js"));
  for (const { packageName, os, cpu, executable } of TYPE_AWARE_BACKENDS) {
    const directory = join(storeNodeModules, ...packageName.split("/"));
    writeJson(join(directory, "package.json"), {
      name: packageName,
      version: BACKEND_VERSION,
      os: [os],
      cpu: [cpu],
    });
    const executablePath = join(directory, "lib", executable);
    write(executablePath, `${packageName}\n`);
    if (os !== "win32") {
      chmodSync(executablePath, 0o755);
    }
  }
  mkdirSync(join(extensionRoot, "node_modules"), { recursive: true });
  symlinkSync(
    join(storeNodeModules, "typescript"),
    join(extensionRoot, "node_modules", "typescript"),
  );
  writeJson(join(extensionRoot, "package.json"), { name: "fallow-vscode", version: VERSION });
  write(join(extensionRoot, ".vscodeignore"), "src/**\n");
  write(join(extensionRoot, "dist", "extension.js"));
  return { extensionRoot, repoRoot };
};

const packagedNames = (destination) =>
  readdirSync(join(destination, "node_modules", "@typescript")).sort();

const inventoryEntry = (variant) => ({
  file: vsixFilename(VERSION, variant.target),
  target: variant.target,
  targetPlatform: variant.targetPlatform,
  version: VERSION,
  bytes: 1024,
  sha256: "a".repeat(64),
  payload: { fileCount: 3, sha256: "b".repeat(64) },
  typescriptVersion: BACKEND_VERSION,
  backends: variant.backends.map(({ packageName, os, cpu, executable }) => ({
    package: packageName,
    os,
    cpu,
    executable: `dist/type-aware/node_modules/${packageName}/lib/${executable}`,
    bytes: 8,
    sha256: "c".repeat(64),
    mode: os === "win32" ? 0o644 : 0o755,
  })),
  sidecar: TYPE_AWARE_SIDECAR_FILES.map(({ path, mode }) => ({
    path,
    bytes: 8,
    sha256: "d".repeat(64),
    mode,
  })),
});

const archiveFiles = (extensionRoot, target, modeOverrides = {}) => {
  const typeAwareRoot = join(extensionRoot, "dist", "type-aware");
  const variant = getVsixVariant(target);
  const files = [
    {
      path: "extension/package.json",
      contents: readFileSync(join(extensionRoot, "package.json")),
      mode: 0o644,
    },
    {
      path: "extension.vsixmanifest",
      contents: `<PackageManifest><Metadata><Identity Id="fallow-vscode" Version="${VERSION}" TargetPlatform="${target}" /></Metadata></PackageManifest>`,
      mode: 0o644,
    },
    ...TYPE_AWARE_SIDECAR_FILES.map(({ path, mode }) => ({
      path: `extension/${path}`,
      contents: readFileSync(join(extensionRoot, ...path.split("/"))),
      mode: modeOverrides[path] ?? mode,
    })),
    ...variant.backends.map(({ packageName, os, executable }) => {
      const path = `dist/type-aware/node_modules/${packageName}/lib/${executable}`;
      return {
        path: `extension/${path}`,
        contents: readFileSync(
          join(typeAwareRoot, "node_modules", ...packageName.split("/"), "lib", executable),
        ),
        mode: modeOverrides[path] ?? (os === "win32" ? 0o644 : 0o755),
      };
    }),
  ];
  return files;
};

test("target catalog is closed and deterministic", () => {
  assert.deepEqual(
    VSIX_VARIANTS.map(({ target }) => target),
    [
      "universal",
      "darwin-arm64",
      "darwin-x64",
      "linux-arm64",
      "linux-x64",
      "win32-arm64",
      "win32-x64",
    ],
  );
  assert.equal(vsixFilename(VERSION, "linux-x64"), "fallow-vscode-1.2.3-linux-x64.vsix");
  assert.throws(() => getVsixVariant("linux-armhf"), /unsupported VSIX target/u);
  assert.throws(() => vsixFilename("latest", "linux-x64"), /invalid VSIX version/u);
});

test("target and universal payloads are isolated and dereferenced", (context) => {
  const { extensionRoot, repoRoot } = fixture(context);
  const targetDestination = join(extensionRoot, "target-payload");
  const universalDestination = join(extensionRoot, "universal-payload");
  packageTypeAware({
    target: "linux-x64",
    extensionRoot,
    repoRoot,
    destination: targetDestination,
  });
  packageTypeAware({
    extensionRoot,
    repoRoot,
    destination: universalDestination,
  });
  assert.deepEqual(packagedNames(targetDestination), ["typescript-linux-x64"]);
  assert.equal(packagedNames(universalDestination).length, TYPE_AWARE_BACKENDS.length);
  assert.equal(
    lstatSync(join(targetDestination, "node_modules", "typescript")).isSymbolicLink(),
    false,
  );
});

test("verifier rejects closure, metadata, mode, and symlink defects", async (context) => {
  const { extensionRoot, repoRoot } = fixture(context);
  const destination = join(extensionRoot, "dist", "type-aware");
  packageTypeAware({ target: "linux-x64", extensionRoot, repoRoot, destination });
  await verifyPackagedTypeAware({ extensionRoot, target: "linux-x64", repoRoot });

  mkdirSync(join(destination, "node_modules", "@typescript", "typescript-extra"));
  await assert.rejects(
    verifyPackagedTypeAware({ extensionRoot, target: "linux-x64", repoRoot }),
    /unexpected packaged TypeScript backends/u,
  );
  rmSync(join(destination, "node_modules", "@typescript", "typescript-extra"), {
    recursive: true,
  });

  const manifestPath = join(
    destination,
    "node_modules",
    "@typescript",
    "typescript-linux-x64",
    "package.json",
  );
  const manifest = JSON.parse(readFileSync(manifestPath, "utf8"));
  writeJson(manifestPath, { ...manifest, cpu: ["arm64"] });
  await assert.rejects(
    verifyPackagedTypeAware({ extensionRoot, target: "linux-x64", repoRoot }),
    /invalid packaged TypeScript backend metadata/u,
  );
  writeJson(manifestPath, manifest);

  if (process.platform !== "win32") {
    const executable = join(dirname(manifestPath), "lib", "tsc");
    chmodSync(executable, 0o644);
    await assert.rejects(
      verifyPackagedTypeAware({ extensionRoot, target: "linux-x64", repoRoot }),
      /not executable/u,
    );
    chmodSync(executable, 0o755);

    const source = join(destination, "src", "windows-child-process.mjs");
    const link = join(destination, "src", "linked.mjs");
    symlinkSync(source, link);
    await assert.rejects(
      verifyPackagedTypeAware({ extensionRoot, target: "linux-x64", repoRoot }),
      /contains a symlink/u,
    );
  }
});

test("verifier reads required executable modes from VSIX central-directory metadata", async (context) => {
  const { extensionRoot, repoRoot } = fixture(context);
  const target = "linux-x64";
  packageTypeAware({
    target,
    extensionRoot,
    repoRoot,
    destination: join(extensionRoot, "dist", "type-aware"),
  });

  const goodVsix = join(dirname(extensionRoot), "good.vsix");
  writeStoredZip(goodVsix, archiveFiles(extensionRoot, target));
  const verified = await verifyPackagedTypeAware({
    extensionRoot,
    vsixPath: goodVsix,
    target,
    repoRoot,
  });
  assert.equal(verified.backends[0].mode, 0o755);
  assert.deepEqual(
    verified.sidecar.map(({ path, mode }) => ({ path, mode })),
    TYPE_AWARE_SIDECAR_FILES,
  );

  const entrypoint = TYPE_AWARE_SIDECAR_FILES[0].path;
  const badSidecarVsix = join(dirname(extensionRoot), "bad-sidecar-mode.vsix");
  writeStoredZip(badSidecarVsix, archiveFiles(extensionRoot, target, { [entrypoint]: 0o644 }));
  await assert.rejects(
    verifyPackagedTypeAware({ extensionRoot, vsixPath: badSidecarVsix, target, repoRoot }),
    /unexpected archived mode for dist\/type-aware\/fallow-type-aware\.mjs/u,
  );

  const backend = getVsixVariant(target).backends[0];
  const backendPath = `dist/type-aware/node_modules/${backend.packageName}/lib/${backend.executable}`;
  const badBackendVsix = join(dirname(extensionRoot), "bad-backend-mode.vsix");
  writeStoredZip(badBackendVsix, archiveFiles(extensionRoot, target, { [backendPath]: 0o644 }));
  await assert.rejects(
    verifyPackagedTypeAware({ extensionRoot, vsixPath: badBackendVsix, target, repoRoot }),
    /unexpected archived mode for @typescript\/typescript-linux-x64/u,
  );

  const sourcePath = `extension/${TYPE_AWARE_SIDECAR_FILES[1].path}`;
  const missingSourceVsix = join(dirname(extensionRoot), "missing-source.vsix");
  writeStoredZip(
    missingSourceVsix,
    archiveFiles(extensionRoot, target).filter(({ path }) => path !== sourcePath),
  );
  await assert.rejects(
    verifyPackagedTypeAware({ extensionRoot, vsixPath: missingSourceVsix, target, repoRoot }),
    /VSIX archive is missing extension\/dist\/type-aware\/src\/windows-child-process\.mjs/u,
  );

  const duplicateSourceVsix = join(dirname(extensionRoot), "duplicate-source.vsix");
  const files = archiveFiles(extensionRoot, target);
  writeStoredZip(duplicateSourceVsix, [...files, files.find(({ path }) => path === sourcePath)]);
  await assert.rejects(
    verifyPackagedTypeAware({ extensionRoot, vsixPath: duplicateSourceVsix, target, repoRoot }),
    /duplicate entry extension\/dist\/type-aware\/src\/windows-child-process\.mjs/u,
  );
});

test("TargetPlatform parsing distinguishes universal and targeted VSIX manifests", () => {
  assert.equal(
    parseVsixTargetPlatform(
      '<PackageManifest><Metadata><Identity Id="x" /></Metadata></PackageManifest>',
    ),
    null,
  );
  assert.equal(
    parseVsixTargetPlatform(
      '<PackageManifest><Metadata><Identity TargetPlatform="win32-x64" Id="x" /></Metadata></PackageManifest>',
    ),
    "win32-x64",
  );
  assert.throws(() => parseVsixTargetPlatform("<PackageManifest />"), /Identity element/u);
  assert.doesNotThrow(() => assertVsixTargetPlatform("linux-x64", "linux-x64"));
  assert.doesNotThrow(() => assertVsixTargetPlatform(null, "universal"));
  assert.throws(
    () => assertVsixTargetPlatform(null, "linux-x64"),
    /expected linux-x64, found absent/u,
  );
  assert.throws(
    () => assertVsixTargetPlatform("win32-x64", "linux-x64"),
    /expected linux-x64, found win32-x64/u,
  );
});

test("normalized payload hashes ignore entry order but detect content changes", () => {
  const first = normalizedPayload(
    new Map([
      ["extension/b.txt", Buffer.from("b")],
      ["extension/a.txt", Buffer.from("a")],
      ["extension.vsixmanifest", Buffer.from("ignored")],
    ]),
  );
  const reordered = normalizedPayload(
    new Map([
      ["extension/a.txt", Buffer.from("a")],
      ["extension/b.txt", Buffer.from("b")],
    ]),
  );
  const changed = normalizedPayload(
    new Map([
      ["extension/a.txt", Buffer.from("changed")],
      ["extension/b.txt", Buffer.from("b")],
    ]),
  );
  assert.equal(first.sha256, reordered.sha256);
  assert.notEqual(first.sha256, changed.sha256);
});

test("inventory rejects missing, duplicate, reordered, and malformed variants", (context) => {
  const entries = VSIX_VARIANTS.map(inventoryEntry);
  const inventory = createInventory(VERSION, entries);
  assert.equal(validateInventory(inventory), inventory);
  assert.throws(
    () => validateInventory({ ...inventory, entries: entries.slice(0, -1) }),
    /exactly 7 entries/u,
  );
  assert.throws(
    () =>
      validateInventory({ ...inventory, entries: [entries[0], entries[0], ...entries.slice(2)] }),
    /duplicate VSIX inventory target/u,
  );
  assert.throws(
    () =>
      validateInventory({ ...inventory, entries: [entries[1], entries[0], ...entries.slice(2)] }),
    /unexpected inventory filename for universal/u,
  );
  assert.throws(
    () =>
      validateInventory({
        ...inventory,
        entries: [{ ...entries[0], file: "wrong.vsix" }, ...entries.slice(1)],
      }),
    /unexpected inventory filename/u,
  );
  assert.throws(
    () =>
      validateInventory({
        ...inventory,
        entries: [
          entries[0],
          { ...entries[1], bytes: getVsixVariant(entries[1].target).maxBytes + 1 },
          ...entries.slice(2),
        ],
      }),
    /invalid inventory size/u,
  );
  assert.throws(
    () =>
      validateInventory({
        ...inventory,
        entries: [{ ...entries[0], sha256: "invalid" }, ...entries.slice(1)],
      }),
    /lowercase SHA-256 digest/u,
  );
  assert.throws(
    () =>
      validateInventory({
        ...inventory,
        entries: [
          {
            ...entries[0],
            backends: [
              { ...entries[0].backends[0], executable: "dist/type-aware/wrong" },
              ...entries[0].backends.slice(1),
            ],
          },
          ...entries.slice(1),
        ],
      }),
    /invalid backend inventory/u,
  );
  assert.throws(
    () =>
      validateInventory({
        ...inventory,
        entries: [
          {
            ...entries[0],
            backends: [{ ...entries[0].backends[0], mode: 0o644 }, ...entries[0].backends.slice(1)],
          },
          ...entries.slice(1),
        ],
      }),
    /invalid backend inventory/u,
  );
  assert.throws(
    () =>
      validateInventory({
        ...inventory,
        entries: [{ ...entries[0], sidecar: entries[0].sidecar.slice(0, 1) }, ...entries.slice(1)],
      }),
    /invalid sidecar inventory/u,
  );
  assert.throws(
    () =>
      validateInventory({
        ...inventory,
        entries: [
          { ...entries[0], sidecar: [entries[0].sidecar[0], entries[0].sidecar[0]] },
          ...entries.slice(1),
        ],
      }),
    /invalid sidecar inventory/u,
  );
  assert.throws(
    () =>
      validateInventory({
        ...inventory,
        entries: [
          {
            ...entries[0],
            sidecar: [{ ...entries[0].sidecar[0], mode: 0o644 }, entries[0].sidecar[1]],
          },
          ...entries.slice(1),
        ],
      }),
    /invalid sidecar inventory/u,
  );

  const output = mkdtempSync(join(tmpdir(), "fallow-vsix-inventory-"));
  context.after(() => rmSync(output, { recursive: true, force: true }));
  for (const entry of entries) {
    write(join(output, entry.file), entry.target);
    entry.sha256 = sha256File(join(output, entry.file));
  }
  const written = createInventory(VERSION, entries);
  writeInventory(output, written);
  const checksums = readFileSync(join(output, "SHA256SUMS"), "utf8");
  assert.match(
    checksums,
    new RegExp(`${sha256File(join(output, INVENTORY_FILENAME))}  inventory\\.json`, "u"),
  );
});

test("variant orchestration is ordered, isolated, cleaned, and deterministic", async (context) => {
  const { extensionRoot, repoRoot } = fixture(context);
  packageTypeAware({
    extensionRoot,
    repoRoot,
    destination: join(extensionRoot, "dist", "type-aware"),
  });
  const scratchRoot = dirname(repoRoot);

  const run = async (label) => {
    const outputDirectory = join(scratchRoot, `output-${label}`);
    const stagingRoot = join(scratchRoot, `staging-${label}`);
    const events = [];
    const createVsixCalls = [];
    const inventory = await packageVsixVariants({
      outputDirectory,
      extensionRoot,
      repoRoot,
      createStagingRoot: () => {
        mkdirSync(stagingRoot);
        return stagingRoot;
      },
      listPackageFiles: async () => ["package.json", "dist/extension.js"],
      packagePayload: ({ target, destination }) => {
        events.push(`payload:${target}`);
        write(join(destination, "variant.txt"), target);
      },
      createVsix: async (options) => {
        const target = options.target ?? "universal";
        events.push(`vsix:${target}`);
        createVsixCalls.push(options);
        assert.equal(
          readFileSync(join(options.cwd, "dist", "type-aware", "variant.txt"), "utf8"),
          target,
        );
        write(options.packagePath, target);
      },
      verifyPackage: async ({ extensionRoot: stageRoot, vsixPath, target }) => {
        events.push(`verify:${target}`);
        assert.equal(stageRoot, join(stagingRoot, target));
        const verified = inventoryEntry(getVsixVariant(target));
        delete verified.file;
        verified.bytes = readFileSync(vsixPath).byteLength;
        verified.sha256 = sha256File(vsixPath);
        return verified;
      },
      removeStagingRoot: (path) => {
        events.push("cleanup");
        rmSync(path, { recursive: true, force: true });
      },
    });
    assert.equal(existsSync(stagingRoot), false);
    assert.deepEqual(
      createVsixCalls.map(({ target }) => target),
      VSIX_VARIANTS.map(({ targetPlatform }) => targetPlatform ?? undefined),
    );
    assert.deepEqual(events, [
      ...VSIX_VARIANTS.flatMap(({ target }) => [
        `payload:${target}`,
        `vsix:${target}`,
        `verify:${target}`,
      ]),
      "cleanup",
    ]);
    return { inventory, outputDirectory };
  };

  const first = await run("first");
  const second = await run("second");
  assert.deepEqual(first.inventory, second.inventory);
  assert.deepEqual(readdirSync(first.outputDirectory), readdirSync(second.outputDirectory));
  for (const file of readdirSync(first.outputDirectory)) {
    assert.deepEqual(
      readFileSync(join(first.outputDirectory, file)),
      readFileSync(join(second.outputDirectory, file)),
    );
  }
});

test("variant orchestration removes isolated staging after a packaging failure", async (context) => {
  const { extensionRoot, repoRoot } = fixture(context);
  packageTypeAware({
    extensionRoot,
    repoRoot,
    destination: join(extensionRoot, "dist", "type-aware"),
  });
  const stagingRoot = join(dirname(repoRoot), "failed-staging");
  await assert.rejects(
    packageVsixVariants({
      outputDirectory: join(dirname(repoRoot), "failed-output"),
      extensionRoot,
      repoRoot,
      createStagingRoot: () => {
        mkdirSync(stagingRoot);
        return stagingRoot;
      },
      listPackageFiles: async () => ["package.json", "dist/extension.js"],
      packagePayload: ({ destination }) => write(join(destination, "variant.txt")),
      createVsix: async () => {
        throw new Error("fixture packaging failure");
      },
    }),
    /fixture packaging failure/u,
  );
  assert.equal(existsSync(stagingRoot), false);
});
