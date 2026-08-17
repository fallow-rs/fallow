const MEBIBYTE = 1024 * 1024;

export const VSIX_EXECUTABLE_MODE = 0o755;
export const VSIX_REGULAR_FILE_MODE = 0o644;

export const TYPE_AWARE_SIDECAR_FILES = Object.freeze([
  Object.freeze({
    path: "dist/type-aware/fallow-type-aware.mjs",
    mode: VSIX_EXECUTABLE_MODE,
  }),
  Object.freeze({
    path: "dist/type-aware/src/windows-child-process.mjs",
    mode: VSIX_REGULAR_FILE_MODE,
  }),
]);

const defineBackend = ({ target, packageName, os, cpu, executable }) =>
  Object.freeze({ target, packageName, os, cpu, executable });

export const TYPE_AWARE_BACKENDS = Object.freeze([
  defineBackend({
    target: "darwin-arm64",
    packageName: "@typescript/typescript-darwin-arm64",
    os: "darwin",
    cpu: "arm64",
    executable: "tsc",
  }),
  defineBackend({
    target: "darwin-x64",
    packageName: "@typescript/typescript-darwin-x64",
    os: "darwin",
    cpu: "x64",
    executable: "tsc",
  }),
  defineBackend({
    target: "linux-arm64",
    packageName: "@typescript/typescript-linux-arm64",
    os: "linux",
    cpu: "arm64",
    executable: "tsc",
  }),
  defineBackend({
    target: "linux-x64",
    packageName: "@typescript/typescript-linux-x64",
    os: "linux",
    cpu: "x64",
    executable: "tsc",
  }),
  defineBackend({
    target: "win32-arm64",
    packageName: "@typescript/typescript-win32-arm64",
    os: "win32",
    cpu: "arm64",
    executable: "tsc.exe",
  }),
  defineBackend({
    target: "win32-x64",
    packageName: "@typescript/typescript-win32-x64",
    os: "win32",
    cpu: "x64",
    executable: "tsc.exe",
  }),
]);

const defineVariant = ({ target, targetPlatform, maxBytes, backends }) =>
  Object.freeze({ target, targetPlatform, maxBytes, backends: Object.freeze([...backends]) });

export const UNIVERSAL_VSIX_TARGET = "universal";

export const UNIVERSAL_VSIX_VARIANT = defineVariant({
  target: UNIVERSAL_VSIX_TARGET,
  targetPlatform: null,
  maxBytes: 70 * MEBIBYTE,
  backends: TYPE_AWARE_BACKENDS,
});

export const TARGETED_VSIX_VARIANTS = Object.freeze(
  TYPE_AWARE_BACKENDS.map((backend) =>
    defineVariant({
      target: backend.target,
      targetPlatform: backend.target,
      maxBytes: 15 * MEBIBYTE,
      backends: [backend],
    }),
  ),
);

export const VSIX_VARIANTS = Object.freeze([UNIVERSAL_VSIX_VARIANT, ...TARGETED_VSIX_VARIANTS]);

const variantsByTarget = new Map(VSIX_VARIANTS.map((variant) => [variant.target, variant]));

if (variantsByTarget.size !== VSIX_VARIANTS.length) {
  throw new Error("VSIX target catalog contains duplicate targets");
}

export const getVsixVariant = (target = UNIVERSAL_VSIX_TARGET) => {
  const variant = variantsByTarget.get(target);
  if (!variant) {
    throw new Error(
      `unsupported VSIX target ${target}; expected one of ${VSIX_VARIANTS.map(({ target: value }) => value).join(", ")}`,
    );
  }
  return variant;
};

export const backendExecutableMode = ({ os }) =>
  os === "win32" ? VSIX_REGULAR_FILE_MODE : VSIX_EXECUTABLE_MODE;

export const vsixFilename = (version, target) => {
  getVsixVariant(target);
  if (!/^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?$/u.test(version)) {
    throw new Error(`invalid VSIX version ${version}`);
  }
  return `fallow-vscode-${version}-${target}.vsix`;
};
