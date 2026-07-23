import { chmodSync, cpSync, mkdirSync, rmSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const extensionRoot = join(dirname(fileURLToPath(import.meta.url)), "..");
const repoRoot = join(extensionRoot, "..", "..");
const sourceRoot = join(repoRoot, "tools", "type-aware-sidecar");
const destination = join(extensionRoot, "dist", "type-aware");

rmSync(destination, { recursive: true, force: true });
mkdirSync(join(destination, "node_modules"), { recursive: true });
cpSync(join(sourceRoot, "fallow-type-aware.mjs"), join(destination, "fallow-type-aware.mjs"));
cpSync(join(sourceRoot, "src"), join(destination, "src"), { recursive: true });
cpSync(join(sourceRoot, "package.json"), join(destination, "package.json"));
cpSync(join(extensionRoot, "node_modules", "typescript"), join(destination, "node_modules", "typescript"), {
  dereference: true,
  recursive: true,
});
chmodSync(join(destination, "fallow-type-aware.mjs"), 0o755);
