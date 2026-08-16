import { accessSync, readFileSync, readdirSync, statSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const MAX_VSIX_BYTES = 70 * 1024 * 1024;
const extensionRoot = resolve(process.argv[2] ?? "");
const vsixPath = process.argv[3] ? resolve(process.argv[3]) : undefined;
const repoRoot = join(dirname(fileURLToPath(import.meta.url)), "..", "..", "..");
const protocol = JSON.parse(
  readFileSync(join(repoRoot, "crates", "api", "type-aware-protocol.json"), "utf8"),
);
const platformPackages = [
  { name: "typescript-darwin-arm64", os: "darwin", cpu: "arm64", bin: "tsc" },
  { name: "typescript-darwin-x64", os: "darwin", cpu: "x64", bin: "tsc" },
  { name: "typescript-linux-arm64", os: "linux", cpu: "arm64", bin: "tsc" },
  { name: "typescript-linux-x64", os: "linux", cpu: "x64", bin: "tsc" },
  { name: "typescript-win32-arm64", os: "win32", cpu: "arm64", bin: "tsc.exe" },
  { name: "typescript-win32-x64", os: "win32", cpu: "x64", bin: "tsc.exe" },
];

if (!process.argv[2]) {
  throw new Error("usage: verify-packaged-type-aware.mjs <extension-root> [vsix-path]");
}

const packageRoot = join(extensionRoot, "dist", "type-aware", "node_modules", "@typescript");
const actualPackages = readdirSync(packageRoot).sort();
const expectedPackages = platformPackages.map(({ name }) => name).sort();
if (JSON.stringify(actualPackages) !== JSON.stringify(expectedPackages)) {
  throw new Error(`unexpected packaged TypeScript backends: ${actualPackages.join(", ")}`);
}

for (const { name, os, cpu, bin } of platformPackages) {
  const directory = join(packageRoot, name);
  const manifest = JSON.parse(readFileSync(join(directory, "package.json"), "utf8"));
  if (
    manifest.name !== `@typescript/${name}` ||
    manifest.version !== protocol.backend.version ||
    !manifest.os?.includes(os) ||
    !manifest.cpu?.includes(cpu)
  ) {
    throw new Error(`invalid packaged TypeScript backend metadata for ${name}`);
  }
  const executable = join(directory, "lib", bin);
  accessSync(executable);
  if (process.platform !== "win32" && os !== "win32" && (statSync(executable).mode & 0o111) === 0) {
    throw new Error(`packaged TypeScript backend is not executable: ${name}`);
  }
}

accessSync(join(extensionRoot, "dist", "type-aware", "fallow-type-aware.mjs"));
accessSync(join(extensionRoot, "dist", "type-aware", "src", "windows-child-process.mjs"));
if (vsixPath && statSync(vsixPath).size > MAX_VSIX_BYTES) {
  throw new Error(`VSIX exceeds ${MAX_VSIX_BYTES} bytes: ${vsixPath}`);
}

process.stdout.write("Packaged type-aware VSIX payload is complete.\n");
