// Build the signed-release bundles of the type-aware companion.
//
// `fallow type-aware setup` downloads one bundle per Node platform from the
// GitHub release. A bundle holds the companion, the pinned `typescript`
// package and the TypeScript platform package of that Node platform. The
// release workflow signs each bundle in a separate job.
//
// Usage:
//   node scripts/type-aware-bundle.mjs --out <dir> [--platform <platform-arch>]...
//
// The script reads the tarball URL and the integrity of each TypeScript
// package from `tools/type-aware-sidecar/package-lock.json` and rejects a
// download that does not match. It never runs package code.

import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { mkdirSync, readFileSync, readdirSync, statSync, writeFileSync } from "node:fs";
import { dirname, join, relative, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";
import { gunzipSync, gzipSync } from "node:zlib";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const SIDECAR_DIR = join(ROOT, "tools", "type-aware-sidecar");
const PROTOCOL_PATH = join(ROOT, "crates", "api", "type-aware-protocol.json");
const BLOCK = 512;
const MANIFEST_SCHEMA_VERSION = 1;

/** Node platforms that get a bundle. Keep in sync with `BUNDLE_PLATFORMS` in
 * `crates/api/src/type_aware/setup_store.rs`. */
export const BUNDLE_PLATFORMS = [
  { platform: "darwin", arch: "arm64" },
  { platform: "darwin", arch: "x64" },
  { platform: "linux", arch: "arm64" },
  { platform: "linux", arch: "x64" },
];

export const assetName = ({ platform, arch }) => `fallow-type-aware-${platform}-${arch}.tar.gz`;

/** POSIX launcher at the root of the bundle. Fallow starts it with an
 * absolute path and a PATH from which project directories are removed. */
export const LAUNCHER = `#!/bin/sh
# Start the fallow-type-aware companion of this bundle with Node.js.
if ! command -v node >/dev/null 2>&1; then
  echo "fallow-type-aware: Node.js 20 or later must be on PATH. An editor can start Fallow with a shorter PATH than your shell. Add the directory of node to the PATH of that program." >&2
  exit 2
fi
here=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd -P) || exit 2
exec node --require "$here/platform-check.cjs" "$here/lib/fallow-type-aware.mjs" "$@"
`;

/** Loaded before the companion. The TypeScript platform package follows the
 * Node that setup asked, so a different \`node\` on PATH at run time (another
 * architecture after a Node switch) must stop with a clear message. */
export const PLATFORM_CHECK = `"use strict";
const { readFileSync } = require("node:fs");
const { join } = require("node:path");

const stop = (message) => {
  process.stderr.write(\`fallow-type-aware: \${message} Run \\\`fallow type-aware setup\\\` again.\\n\`);
  process.exit(2);
};

let bundle;
try {
  bundle = JSON.parse(readFileSync(join(__dirname, "bundle.json"), "utf8"));
} catch (error) {
  stop(\`cannot read bundle.json of this companion (\${error instanceof Error ? error.message : String(error)}).\`);
}
if (bundle.platform !== process.platform || bundle.arch !== process.arch) {
  stop(
    \`this companion is for Node on \${bundle.platform}-\${bundle.arch}, \` +
      \`but the node on PATH runs on \${process.platform}-\${process.arch}.\`,
  );
}
`;

const octal = (value, width) => value.toString(8).padStart(width - 1, "0") + "\0";

const splitName = (name) => {
  const bytes = Buffer.byteLength(name);
  if (bytes <= 100) {
    return { name, prefix: "" };
  }
  for (let index = name.indexOf("/"); index !== -1; index = name.indexOf("/", index + 1)) {
    const prefix = name.slice(0, index);
    const rest = name.slice(index + 1);
    if (Buffer.byteLength(prefix) <= 155 && Buffer.byteLength(rest) <= 100) {
      return { name: rest, prefix };
    }
  }
  throw new Error(`path is too long for a ustar header: ${name}`);
};

const header = ({ path, size, mode, type }) => {
  const block = Buffer.alloc(BLOCK);
  const { name, prefix } = splitName(path);
  block.write(name, 0, 100, "utf8");
  block.write(octal(mode, 8), 100, 8, "ascii");
  block.write(octal(0, 8), 108, 8, "ascii");
  block.write(octal(0, 8), 116, 8, "ascii");
  block.write(octal(size, 12), 124, 12, "ascii");
  block.write(octal(0, 12), 136, 12, "ascii");
  block.fill(0x20, 148, 156);
  block.write(type, 156, 1, "ascii");
  block.write("ustar\0", 257, 6, "ascii");
  block.write("00", 263, 2, "ascii");
  block.write(prefix, 345, 155, "utf8");
  let sum = 0;
  for (const byte of block) {
    sum += byte;
  }
  block.write(`${sum.toString(8).padStart(6, "0")}\0 `, 148, 8, "ascii");
  return block;
};

/** Write a deterministic ustar archive: sorted paths, zero times and owners,
 * mode 0755 or 0644, regular files and directories only. */
export const writeTar = (files) => {
  const sorted = [...files].toSorted((left, right) => (left.path < right.path ? -1 : 1));
  const directories = new Set();
  for (const file of sorted) {
    const parts = file.path.split("/");
    for (let depth = 1; depth < parts.length; depth += 1) {
      directories.add(parts.slice(0, depth).join("/"));
    }
  }
  const entries = [
    ...[...directories].map((path) => ({ path: `${path}/`, directory: true })),
    ...sorted,
  ].toSorted((left, right) => (left.path < right.path ? -1 : 1));
  const chunks = [];
  for (const entry of entries) {
    if (entry.directory) {
      chunks.push(header({ path: entry.path, size: 0, mode: 0o755, type: "5" }));
      continue;
    }
    chunks.push(
      header({
        path: entry.path,
        size: entry.data.length,
        mode: entry.executable ? 0o755 : 0o644,
        type: "0",
      }),
    );
    chunks.push(entry.data);
    const padding = (BLOCK - (entry.data.length % BLOCK)) % BLOCK;
    chunks.push(Buffer.alloc(padding));
  }
  chunks.push(Buffer.alloc(BLOCK * 2));
  return Buffer.concat(chunks);
};

const readString = (block, start, length) => {
  const slice = block.subarray(start, start + length);
  const end = slice.indexOf(0);
  return slice.subarray(0, end === -1 ? slice.length : end).toString("utf8");
};

const parsePax = (data) => {
  const values = {};
  let offset = 0;
  while (offset < data.length) {
    const space = data.indexOf(0x20, offset);
    const length = Number(data.subarray(offset, space).toString("ascii"));
    assert.ok(Number.isInteger(length) && length > 0, "invalid pax record");
    const record = data.subarray(space + 1, offset + length - 1).toString("utf8");
    const equals = record.indexOf("=");
    values[record.slice(0, equals)] = record.slice(equals + 1);
    offset += length;
  }
  return values;
};

/** Read the regular files of a ustar or pax archive. Every other entry type
 * is an error, so a package with a link cannot enter a bundle. */
export const readTar = (archive) => {
  const files = [];
  let offset = 0;
  let pax = {};
  while (offset + BLOCK <= archive.length) {
    const block = archive.subarray(offset, offset + BLOCK);
    if (block.every((byte) => byte === 0)) {
      break;
    }
    const size = Number.parseInt(readString(block, 124, 12).trim() || "0", 8);
    const type = String.fromCharCode(block[156] || 0x30);
    const prefix = readString(block, 345, 155);
    const name = readString(block, 0, 100);
    const mode = Number.parseInt(readString(block, 100, 8).trim() || "0", 8);
    const data = archive.subarray(offset + BLOCK, offset + BLOCK + size);
    offset += BLOCK + Math.ceil(size / BLOCK) * BLOCK;
    if (type === "x") {
      pax = parsePax(data);
      continue;
    }
    if (type === "g") {
      continue;
    }
    const path = pax.path ?? (prefix ? `${prefix}/${name}` : name);
    pax = {};
    if (type === "5") {
      continue;
    }
    assert.ok(type === "0", `unsupported tar entry type ${type} for ${path}`);
    files.push({ path, data: Buffer.from(data), executable: (mode & 0o111) !== 0 });
  }
  return files;
};

const integrityOf = (data, algorithm) =>
  `${algorithm}-${createHash(algorithm).update(data).digest("base64")}`;

/** Check a tarball against an npm `integrity` value. */
export const checkIntegrity = (data, integrity, name) => {
  const [algorithm] = integrity.split("-", 1);
  assert.ok(["sha512", "sha384", "sha256"].includes(algorithm), `${name}: weak integrity`);
  assert.equal(integrityOf(data, algorithm), integrity, `${name}: integrity mismatch`);
};

const lockedPackage = (lock, key) => {
  const entry = lock.packages?.[key];
  assert.ok(entry?.resolved && entry?.integrity, `package-lock.json has no ${key}`);
  return entry;
};

const fetchTarball = async (url) => {
  const response = await fetch(url);
  assert.ok(response.ok, `GET ${url} returned ${response.status}`);
  return Buffer.from(await response.arrayBuffer());
};

const packageFiles = (tarball, target) =>
  readTar(gunzipSync(tarball)).map((file) => {
    assert.ok(file.path.startsWith("package/"), `unexpected path ${file.path}`);
    return { ...file, path: `${target}/${file.path.slice("package/".length)}` };
  });

const listFiles = (dir, base = dir) =>
  readdirSync(dir, { withFileTypes: true }).flatMap((entry) => {
    const path = join(dir, entry.name);
    if (entry.isDirectory()) {
      return listFiles(path, base);
    }
    assert.ok(entry.isFile(), `not a regular file: ${path}`);
    return [
      {
        path: relative(base, path).split(sep).join("/"),
        data: readFileSync(path),
        executable: (statSync(path).mode & 0o111) !== 0,
      },
    ];
  });

const companionFiles = (sidecarDir, manifest) => {
  const files = [];
  for (const entry of manifest.files) {
    const path = join(sidecarDir, entry);
    if (statSync(path).isDirectory()) {
      files.push(
        ...listFiles(path).map((file) => ({ ...file, path: `lib/${entry}/${file.path}` })),
      );
    } else {
      files.push({ path: `lib/${entry}`, data: readFileSync(path), executable: false });
    }
  }
  files.push({ path: "lib/LICENSE", data: readFileSync(join(sidecarDir, "LICENSE")) });
  const runtimeManifest = {
    name: manifest.name,
    version: manifest.version,
    description: manifest.description,
    license: manifest.license,
    type: manifest.type,
    bin: manifest.bin,
    dependencies: manifest.dependencies,
    engines: manifest.engines,
  };
  files.push({
    path: "lib/package.json",
    data: Buffer.from(`${JSON.stringify(runtimeManifest, null, 2)}\n`),
  });
  return files;
};

const noticeText = ({ version, typescript, platformPackage }) =>
  [
    "# Third-party notices",
    "",
    `This bundle holds fallow-type-aware ${version} (MIT, see lib/LICENSE) and`,
    "two packages of Microsoft TypeScript under the Apache License 2.0:",
    "",
    `- typescript ${typescript}: lib/node_modules/typescript/LICENSE`,
    `- ${platformPackage} ${typescript}: lib/node_modules/${platformPackage}/LICENSE`,
    `  and lib/node_modules/${platformPackage}/NOTICE.txt`,
    "",
    "The packages are unchanged copies of the npm releases that",
    "tools/type-aware-sidecar/package-lock.json pins.",
    "",
  ].join("\n");

/**
 * Build one bundle in memory.
 *
 * `download(url)` returns the tarball bytes. Tests pass a local function.
 */
export const buildBundle = async ({
  platform,
  arch,
  sidecarDir = SIDECAR_DIR,
  protocolPath = PROTOCOL_PATH,
  download = fetchTarball,
}) => {
  assert.ok(
    BUNDLE_PLATFORMS.some((entry) => entry.platform === platform && entry.arch === arch),
    `no bundle for ${platform}-${arch}`,
  );
  const manifest = JSON.parse(readFileSync(join(sidecarDir, "package.json"), "utf8"));
  const lock = JSON.parse(readFileSync(join(sidecarDir, "package-lock.json"), "utf8"));
  const protocol = JSON.parse(readFileSync(protocolPath, "utf8"));
  const typescript = lockedPackage(lock, "node_modules/typescript");
  assert.equal(
    typescript.version,
    manifest.dependencies.typescript,
    "package-lock.json and package.json pin different typescript versions",
  );
  const platformPackage = `@typescript/typescript-${platform}-${arch}`;
  const native = lockedPackage(lock, `node_modules/${platformPackage}`);
  assert.equal(native.version, typescript.version, `${platformPackage} version differs`);

  const typescriptTarball = await download(typescript.resolved);
  checkIntegrity(typescriptTarball, typescript.integrity, "typescript");
  const nativeTarball = await download(native.resolved);
  checkIntegrity(nativeTarball, native.integrity, platformPackage);

  const bundleManifest = {
    schema_version: MANIFEST_SCHEMA_VERSION,
    fallow_version: manifest.version,
    wire_protocol_version: protocol.wire_protocol_version,
    platform,
    arch,
    typescript_version: typescript.version,
  };
  const files = [
    { path: "fallow-type-aware", data: Buffer.from(LAUNCHER), executable: true },
    { path: "platform-check.cjs", data: Buffer.from(PLATFORM_CHECK) },
    { path: "bundle.json", data: Buffer.from(`${JSON.stringify(bundleManifest, null, 2)}\n`) },
    {
      path: "THIRD_PARTY_NOTICES.md",
      data: Buffer.from(
        noticeText({ version: manifest.version, typescript: typescript.version, platformPackage }),
      ),
    },
    ...companionFiles(sidecarDir, manifest),
    ...packageFiles(typescriptTarball, "lib/node_modules/typescript"),
    ...packageFiles(nativeTarball, `lib/node_modules/${platformPackage}`),
  ];
  const seen = new Set();
  for (const file of files) {
    assert.ok(!seen.has(file.path), `duplicate bundle path ${file.path}`);
    seen.add(file.path);
  }
  return {
    name: assetName({ platform, arch }),
    manifest: bundleManifest,
    archive: gzipSync(writeTar(files), { level: 9 }),
  };
};

const parseArgs = (args) => {
  const options = { out: undefined, platforms: [] };
  for (let index = 0; index < args.length; index += 1) {
    const arg = args[index];
    const value = args[index + 1];
    if (arg === "--out" && value) {
      options.out = value;
      index += 1;
    } else if (arg === "--platform" && value) {
      const [platform, arch] = value.split("-");
      options.platforms.push({ platform, arch });
      index += 1;
    } else {
      throw new Error(`usage: type-aware-bundle.mjs --out <dir> [--platform <platform-arch>]...`);
    }
  }
  assert.ok(options.out, "--out is required");
  if (options.platforms.length === 0) {
    options.platforms = BUNDLE_PLATFORMS;
  }
  return options;
};

const isDirectInvocation =
  process.argv[1] !== undefined && resolve(process.argv[1]) === fileURLToPath(import.meta.url);

if (isDirectInvocation) {
  const options = parseArgs(process.argv.slice(2));
  mkdirSync(options.out, { recursive: true });
  for (const { platform, arch } of options.platforms) {
    const bundle = await buildBundle({ platform, arch });
    writeFileSync(join(options.out, bundle.name), bundle.archive);
    console.log(`wrote ${bundle.name} (${bundle.archive.length} bytes)`);
  }
}
