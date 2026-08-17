import {
  chmodSync,
  cpSync,
  mkdirSync,
  readFileSync,
  realpathSync,
  rmSync,
  statSync,
} from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { getVsixVariant, TYPE_AWARE_BACKENDS, UNIVERSAL_VSIX_TARGET } from "./vsix-targets.mjs";

const scriptPath = fileURLToPath(import.meta.url);
const defaultExtensionRoot = join(dirname(scriptPath), "..");
const defaultRepoRoot = join(defaultExtensionRoot, "..", "..");

const readPackage = (directory) =>
  JSON.parse(readFileSync(join(directory, "package.json"), "utf8"));

export const packageTypeAware = ({
  target = UNIVERSAL_VSIX_TARGET,
  extensionRoot = defaultExtensionRoot,
  repoRoot = defaultRepoRoot,
  destination = join(extensionRoot, "dist", "type-aware"),
} = {}) => {
  const variant = getVsixVariant(target);
  const sourceRoot = join(repoRoot, "tools", "type-aware-sidecar");
  const protocol = JSON.parse(
    readFileSync(join(repoRoot, "crates", "api", "type-aware-protocol.json"), "utf8"),
  );
  const backendVersion = protocol.backend.version;
  const typescriptSource = join(extensionRoot, "node_modules", "typescript");
  const typescriptNodeModules = dirname(realpathSync(typescriptSource));
  const typescriptManifest = readPackage(typescriptSource);

  if (typescriptManifest.version !== backendVersion) {
    throw new Error(`expected TypeScript ${backendVersion}, found ${typescriptManifest.version}`);
  }
  for (const { packageName } of TYPE_AWARE_BACKENDS) {
    if (typescriptManifest.optionalDependencies?.[packageName] !== backendVersion) {
      throw new Error(`TypeScript does not pin ${packageName} to ${backendVersion}`);
    }
  }

  rmSync(destination, { recursive: true, force: true });
  mkdirSync(join(destination, "node_modules"), { recursive: true });
  cpSync(join(sourceRoot, "fallow-type-aware.mjs"), join(destination, "fallow-type-aware.mjs"));
  cpSync(join(sourceRoot, "src"), join(destination, "src"), {
    dereference: true,
    recursive: true,
  });
  cpSync(join(sourceRoot, "package.json"), join(destination, "package.json"));
  cpSync(typescriptSource, join(destination, "node_modules", "typescript"), {
    dereference: true,
    recursive: true,
  });

  for (const { packageName, os, cpu, executable } of variant.backends) {
    const source = join(typescriptNodeModules, ...packageName.split("/"));
    const manifest = readPackage(source);
    if (
      manifest.name !== packageName ||
      manifest.version !== backendVersion ||
      !manifest.os?.includes(os) ||
      !manifest.cpu?.includes(cpu)
    ) {
      throw new Error(`invalid bundled TypeScript backend metadata for ${packageName}`);
    }
    const sourceExecutable = join(source, "lib", executable);
    if (
      process.platform !== "win32" &&
      os !== "win32" &&
      (statSync(sourceExecutable).mode & 0o111) === 0
    ) {
      throw new Error(`bundled TypeScript backend is not executable: ${packageName}`);
    }
    const targetDirectory = join(destination, "node_modules", ...packageName.split("/"));
    cpSync(source, targetDirectory, { dereference: true, recursive: true });
    const targetExecutable = join(targetDirectory, "lib", executable);
    if (
      process.platform !== "win32" &&
      os !== "win32" &&
      (statSync(targetExecutable).mode & 0o111) === 0
    ) {
      throw new Error(`copied TypeScript backend is not executable: ${packageName}`);
    }
  }

  chmodSync(join(destination, "fallow-type-aware.mjs"), 0o755);
  return { backendVersion, destination, variant };
};

const isMain = process.argv[1] && resolve(process.argv[1]) === resolve(scriptPath);
if (isMain) {
  const target = process.argv[2] ?? UNIVERSAL_VSIX_TARGET;
  const destination = process.argv[3] ? resolve(process.argv[3]) : undefined;
  packageTypeAware({ target, ...(destination ? { destination } : {}) });
}
