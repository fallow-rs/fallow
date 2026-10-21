import { lstatSync, readFileSync, readdirSync, statSync } from "node:fs";
import { dirname, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { archiveFileRecord, inspectVsixArchive } from "./vsix-archive.mjs";
import {
  backendExecutableMode,
  getVsixVariant,
  TYPE_AWARE_BACKENDS,
  TYPE_AWARE_SIDECAR_FILES,
  UNIVERSAL_VSIX_TARGET,
} from "./vsix-targets.mjs";

const scriptPath = fileURLToPath(import.meta.url);
const defaultRepoRoot = join(dirname(scriptPath), "..", "..", "..");

const readPackage = (directory) =>
  JSON.parse(readFileSync(join(directory, "package.json"), "utf8"));

const assertNoSymlinks = (root) => {
  const visit = (path) => {
    const stat = lstatSync(path);
    if (stat.isSymbolicLink()) {
      throw new Error(`packaged type-aware payload contains a symlink: ${relative(root, path)}`);
    }
    if (!stat.isDirectory()) {
      return;
    }
    for (const entry of readdirSync(path)) {
      visit(join(path, entry));
    }
  };
  visit(root);
};

const fileMode = (path) => statSync(path).mode & 0o777;

const assertExecutable = (path, label) => {
  if (process.platform !== "win32" && (fileMode(path) & 0o111) === 0) {
    throw new Error(`packaged ${label} is not executable`);
  }
};

export const assertVsixTargetPlatform = (actualTargetPlatform, target) => {
  const variant = getVsixVariant(target);
  if (actualTargetPlatform !== variant.targetPlatform) {
    throw new Error(
      `unexpected TargetPlatform for ${target}: expected ${variant.targetPlatform ?? "absent"}, found ${actualTargetPlatform ?? "absent"}`,
    );
  }
};

const requireArchiveMode = (record, expectedMode, label) => {
  if (record.mode !== expectedMode) {
    throw new Error(
      `unexpected archived mode for ${label}: expected ${expectedMode.toString(8)}, found ${record.mode === null ? "absent" : record.mode.toString(8)}`,
    );
  }
};

const sidecarRecords = (archive) =>
  TYPE_AWARE_SIDECAR_FILES.map(({ path, mode }) => {
    const record = archiveFileRecord(archive, `extension/${path}`);
    requireArchiveMode(record, mode, path);
    return record;
  });

export const verifyPackagedTypeAware = async ({
  extensionRoot,
  vsixPath,
  target = UNIVERSAL_VSIX_TARGET,
  expectedVersion,
  repoRoot = defaultRepoRoot,
}) => {
  const root = resolve(extensionRoot);
  const variant = getVsixVariant(target);
  const protocol = JSON.parse(
    readFileSync(join(repoRoot, "crates", "api", "type-aware-protocol.json"), "utf8"),
  );
  const typeAwareRoot = join(root, "dist", "type-aware");
  const packageRoot = join(typeAwareRoot, "node_modules", "@typescript");
  const actualPackages = readdirSync(packageRoot).toSorted();
  const expectedPackages = variant.backends
    .map(({ packageName }) => packageName.slice("@typescript/".length))
    .toSorted();
  if (JSON.stringify(actualPackages) !== JSON.stringify(expectedPackages)) {
    throw new Error(
      `unexpected packaged TypeScript backends for ${target}: ${actualPackages.join(", ")}`,
    );
  }

  assertNoSymlinks(typeAwareRoot);
  const typescriptManifest = readPackage(join(typeAwareRoot, "node_modules", "typescript"));
  if (typescriptManifest.version !== protocol.backend.version) {
    throw new Error(
      `invalid packaged TypeScript version: expected ${protocol.backend.version}, found ${typescriptManifest.version}`,
    );
  }
  for (const { packageName } of TYPE_AWARE_BACKENDS) {
    if (typescriptManifest.optionalDependencies?.[packageName] !== protocol.backend.version) {
      throw new Error(
        `packaged TypeScript does not pin ${packageName} to ${protocol.backend.version}`,
      );
    }
  }

  const sidecarEntrypoint = join(typeAwareRoot, "fallow-type-aware.mjs");
  statSync(sidecarEntrypoint);
  statSync(join(typeAwareRoot, "src", "windows-child-process.mjs"));
  assertExecutable(sidecarEntrypoint, "type-aware sidecar entrypoint");

  const extensionManifest = readPackage(root);
  if (expectedVersion && extensionManifest.version !== expectedVersion) {
    throw new Error(
      `unexpected extension version: expected ${expectedVersion}, found ${extensionManifest.version}`,
    );
  }

  const backendPayloads = variant.backends.map(({ packageName, os, cpu, executable }) => {
    const packageDirectory = join(packageRoot, packageName.slice("@typescript/".length));
    const manifest = readPackage(packageDirectory);
    if (
      manifest.name !== packageName ||
      manifest.version !== protocol.backend.version ||
      !manifest.os?.includes(os) ||
      !manifest.cpu?.includes(cpu)
    ) {
      throw new Error(`invalid packaged TypeScript backend metadata for ${packageName}`);
    }
    const executablePath = join(packageDirectory, "lib", executable);
    statSync(executablePath);
    if (os !== "win32") {
      assertExecutable(executablePath, `TypeScript backend ${packageName}`);
    }
    return { packageName, os, cpu, executable, executablePath };
  });

  if (!vsixPath) {
    return {
      target,
      targetPlatform: variant.targetPlatform,
      version: extensionManifest.version,
      typescriptVersion: typescriptManifest.version,
    };
  }

  const archive = await inspectVsixArchive(resolve(vsixPath));
  assertVsixTargetPlatform(archive.targetPlatform, target);
  if (archive.version !== extensionManifest.version) {
    throw new Error(
      `VSIX version ${archive.version} does not match extracted extension ${extensionManifest.version}`,
    );
  }
  if (archive.bytes > variant.maxBytes) {
    throw new Error(`VSIX exceeds ${variant.maxBytes} bytes for ${target}: ${vsixPath}`);
  }

  const backends = backendPayloads.map(({ packageName, os, cpu, executable }) => {
    const archivePath = [
      "extension",
      "dist",
      "type-aware",
      "node_modules",
      ...packageName.split("/"),
      "lib",
      executable,
    ].join("/");
    const record = archiveFileRecord(archive, archivePath);
    requireArchiveMode(record, backendExecutableMode({ os }), packageName);
    return {
      package: packageName,
      os,
      cpu,
      executable: record.path,
      bytes: record.bytes,
      sha256: record.sha256,
      mode: record.mode,
    };
  });

  return {
    target,
    targetPlatform: archive.targetPlatform,
    version: archive.version,
    bytes: archive.bytes,
    sha256: archive.sha256,
    payload: {
      fileCount: archive.payload.fileCount,
      sha256: archive.payload.sha256,
    },
    typescriptVersion: typescriptManifest.version,
    backends,
    sidecar: sidecarRecords(archive),
  };
};

const parseArguments = (args) => {
  const [extensionRoot, vsixPath, ...options] = args;
  if (!extensionRoot) {
    throw new Error(
      "usage: verify-packaged-type-aware.mjs <extension-root> [vsix-path] [--target <target>] [--version <version>] [--json]",
    );
  }
  let target = UNIVERSAL_VSIX_TARGET;
  let expectedVersion;
  let json = false;
  for (let index = 0; index < options.length; index += 1) {
    const option = options[index];
    if (option === "--target") {
      target = options[index + 1];
      index += 1;
    } else if (option === "--version") {
      expectedVersion = options[index + 1];
      index += 1;
    } else if (option === "--json") {
      json = true;
    } else {
      throw new Error(`unknown verifier argument ${option}`);
    }
  }
  if (!target) {
    throw new Error("--target requires a value");
  }
  if (options.includes("--version") && !expectedVersion) {
    throw new Error("--version requires a value");
  }
  return { extensionRoot, vsixPath, target, expectedVersion, json };
};

const isMain = process.argv[1] && resolve(process.argv[1]) === resolve(scriptPath);
if (isMain) {
  const { json, ...options } = parseArguments(process.argv.slice(2));
  const result = await verifyPackagedTypeAware(options);
  process.stdout.write(
    json
      ? `${JSON.stringify(result, null, 2)}\n`
      : `Packaged type-aware VSIX payload is complete for ${result.target}.\n`,
  );
}
