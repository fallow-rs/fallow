import {
  accessSync,
  cpSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  readdirSync,
  rmSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { createVSIX, listFiles, PackageManager } from "@vscode/vsce";

import { packageTypeAware } from "./package-type-aware.mjs";
import { verifyPackagedTypeAware } from "./verify-packaged-type-aware.mjs";
import { createInventory, writeInventory } from "./vsix-inventory.mjs";
import { VSIX_VARIANTS, vsixFilename } from "./vsix-targets.mjs";

const scriptPath = fileURLToPath(import.meta.url);
const defaultExtensionRoot = join(dirname(scriptPath), "..");
const defaultRepoRoot = join(defaultExtensionRoot, "..", "..");
const DEFAULT_SOURCE_DATE_EPOCH = "315532800";

const prepareOutputDirectory = (outputDirectory) => {
  mkdirSync(outputDirectory, { recursive: true });
  const existing = readdirSync(outputDirectory);
  if (existing.length > 0) {
    throw new Error(`VSIX output directory must be empty: ${outputDirectory}`);
  }
};

const copyPackageFiles = (extensionRoot, stageRoot, packageFiles) => {
  for (const relativePath of packageFiles) {
    const normalizedPath = relativePath.replaceAll("\\", "/");
    if (
      normalizedPath.startsWith("dist/type-aware/") ||
      normalizedPath.endsWith("/.metadata_never_index")
    ) {
      continue;
    }
    const source = join(extensionRoot, relativePath);
    const destination = join(stageRoot, relativePath);
    mkdirSync(dirname(destination), { recursive: true });
    cpSync(source, destination, { dereference: true, recursive: true });
  }
  const ignoreSource = join(extensionRoot, ".vscodeignore");
  accessSync(ignoreSource);
  cpSync(ignoreSource, join(stageRoot, ".vscodeignore"));
};

export const packageVsixVariants = async ({
  outputDirectory,
  extensionRoot = defaultExtensionRoot,
  repoRoot = defaultRepoRoot,
  createVsix = createVSIX,
  createStagingRoot = () => mkdtempSync(join(tmpdir(), "fallow-vsix-variants-")),
  listPackageFiles = listFiles,
  packagePayload = packageTypeAware,
  removeStagingRoot = (path) => rmSync(path, { recursive: true, force: true }),
  verifyPackage = verifyPackagedTypeAware,
  writeOutputInventory = writeInventory,
} = {}) => {
  if (!outputDirectory) {
    throw new Error("packageVsixVariants requires outputDirectory");
  }
  const outputRoot = resolve(outputDirectory);
  const sourceRoot = resolve(extensionRoot);
  const repositoryRoot = resolve(repoRoot);
  accessSync(join(sourceRoot, "dist", "extension.js"));
  accessSync(join(sourceRoot, "dist", "type-aware", "fallow-type-aware.mjs"));
  prepareOutputDirectory(outputRoot);

  const manifest = JSON.parse(readFileSync(join(sourceRoot, "package.json"), "utf8"));
  const packageFiles = await listPackageFiles({
    cwd: sourceRoot,
    packageManager: PackageManager.None,
    packagedDependencies: [],
  });
  const stagingRoot = createStagingRoot();
  const previousEpoch = process.env.SOURCE_DATE_EPOCH;
  process.env.SOURCE_DATE_EPOCH = previousEpoch || DEFAULT_SOURCE_DATE_EPOCH;

  try {
    const entries = [];
    for (const variant of VSIX_VARIANTS) {
      const stageRoot = join(stagingRoot, variant.target);
      mkdirSync(stageRoot, { recursive: true });
      copyPackageFiles(sourceRoot, stageRoot, packageFiles);
      packagePayload({
        target: variant.target,
        extensionRoot: sourceRoot,
        repoRoot: repositoryRoot,
        destination: join(stageRoot, "dist", "type-aware"),
      });
      const file = vsixFilename(manifest.version, variant.target);
      const vsixPath = join(outputRoot, file);
      await createVsix({
        cwd: stageRoot,
        packagePath: vsixPath,
        target: variant.targetPlatform ?? undefined,
        dependencies: false,
      });
      const verified = await verifyPackage({
        extensionRoot: stageRoot,
        vsixPath,
        target: variant.target,
        expectedVersion: manifest.version,
        repoRoot: repositoryRoot,
      });
      entries.push({ file, ...verified });
    }
    const inventory = createInventory(manifest.version, entries);
    writeOutputInventory(outputRoot, inventory);
    return inventory;
  } finally {
    if (previousEpoch === undefined) {
      delete process.env.SOURCE_DATE_EPOCH;
    } else {
      process.env.SOURCE_DATE_EPOCH = previousEpoch;
    }
    removeStagingRoot(stagingRoot);
  }
};

const parseArguments = (args) => {
  let outputDirectory;
  for (let index = 0; index < args.length; index += 1) {
    if (args[index] === "--") {
      continue;
    }
    if (args[index] === "--output-dir") {
      outputDirectory = args[index + 1];
      index += 1;
    } else {
      throw new Error(`unknown package variants argument ${args[index]}`);
    }
  }
  if (!outputDirectory) {
    throw new Error("usage: package-vsix-variants.mjs --output-dir <directory>");
  }
  return { outputDirectory };
};

const isMain = process.argv[1] && resolve(process.argv[1]) === resolve(scriptPath);
if (isMain) {
  const options = parseArguments(process.argv.slice(2));
  const inventory = await packageVsixVariants(options);
  process.stdout.write(
    `Packaged ${inventory.entries.length} VSIX variants in ${resolve(options.outputDirectory)}.\n`,
  );
}
