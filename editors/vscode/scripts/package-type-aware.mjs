import {
  chmodSync,
  cpSync,
  mkdirSync,
  readFileSync,
  realpathSync,
  rmSync,
  statSync,
} from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const extensionRoot = join(dirname(fileURLToPath(import.meta.url)), "..");
const repoRoot = join(extensionRoot, "..", "..");
const sourceRoot = join(repoRoot, "tools", "type-aware-sidecar");
const destination = join(extensionRoot, "dist", "type-aware");
const protocol = JSON.parse(
  readFileSync(join(repoRoot, "crates", "api", "type-aware-protocol.json"), "utf8"),
);
const backendVersion = protocol.backend.version;
const platformPackages = [
  { name: "@typescript/typescript-darwin-arm64", os: "darwin", cpu: "arm64", bin: "tsc" },
  { name: "@typescript/typescript-darwin-x64", os: "darwin", cpu: "x64", bin: "tsc" },
  { name: "@typescript/typescript-linux-arm64", os: "linux", cpu: "arm64", bin: "tsc" },
  { name: "@typescript/typescript-linux-x64", os: "linux", cpu: "x64", bin: "tsc" },
  {
    name: "@typescript/typescript-win32-arm64",
    os: "win32",
    cpu: "arm64",
    bin: "tsc.exe",
  },
  {
    name: "@typescript/typescript-win32-x64",
    os: "win32",
    cpu: "x64",
    bin: "tsc.exe",
  },
];

const readPackage = (directory) =>
  JSON.parse(readFileSync(join(directory, "package.json"), "utf8"));

const copyPlatformPackage = ({ name, os, cpu, bin }) => {
  const source = join(typescriptNodeModules, ...name.split("/"));
  const manifest = readPackage(source);
  if (
    manifest.name !== name ||
    manifest.version !== backendVersion ||
    !manifest.os?.includes(os) ||
    !manifest.cpu?.includes(cpu)
  ) {
    throw new Error(`invalid bundled TypeScript backend metadata for ${name}`);
  }
  const executable = join(source, "lib", bin);
  const mode = statSync(executable).mode;
  if (process.platform !== "win32" && os !== "win32" && (mode & 0o111) === 0) {
    throw new Error(`bundled TypeScript backend is not executable: ${name}`);
  }
  cpSync(source, join(destination, "node_modules", ...name.split("/")), {
    dereference: true,
    recursive: true,
  });
};

rmSync(destination, { recursive: true, force: true });
mkdirSync(join(destination, "node_modules"), { recursive: true });
cpSync(join(sourceRoot, "fallow-type-aware.mjs"), join(destination, "fallow-type-aware.mjs"));
cpSync(join(sourceRoot, "src"), join(destination, "src"), { recursive: true });
cpSync(join(sourceRoot, "package.json"), join(destination, "package.json"));
const typescriptSource = join(extensionRoot, "node_modules", "typescript");
const typescriptNodeModules = dirname(realpathSync(typescriptSource));
const typescriptManifest = readPackage(typescriptSource);
if (typescriptManifest.version !== backendVersion) {
  throw new Error(`expected TypeScript ${backendVersion}, found ${typescriptManifest.version}`);
}
for (const { name } of platformPackages) {
  if (typescriptManifest.optionalDependencies?.[name] !== backendVersion) {
    throw new Error(`TypeScript does not pin ${name} to ${backendVersion}`);
  }
}
cpSync(typescriptSource, join(destination, "node_modules", "typescript"), {
  dereference: true,
  recursive: true,
});
for (const platformPackage of platformPackages) {
  copyPlatformPackage(platformPackage);
}
chmodSync(join(destination, "fallow-type-aware.mjs"), 0o755);
