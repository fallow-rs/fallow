const fs = require("node:fs");
const path = require("node:path");
const { configureTypeAwareCommand } = require("./type-aware-command.js");

const packageVersion = require("./package.json").version;

const existingCompanion = (manifestPath) => {
  const companion = path.join(path.dirname(manifestPath), "fallow-type-aware.mjs");
  return fs.existsSync(companion) ? companion : null;
};

const resolveTypeAwareCompanion = () => {
  try {
    const manifestPath = require.resolve("fallow-type-aware/package.json");
    const manifest = JSON.parse(fs.readFileSync(manifestPath, "utf8"));
    return manifest.version === packageVersion ? existingCompanion(manifestPath) : null;
  } catch {
    return null;
  }
};

const configureTypeAwareCompanion = () => {
  if (process.env.FALLOW_TYPE_AWARE_BIN) return;
  const companion = resolveTypeAwareCompanion();
  if (companion) {
    configureTypeAwareCommand(companion);
  }
};

let similarCodeCompanion;

const resolveSimilarCodeCompanion = () => {
  if (similarCodeCompanion !== undefined) return similarCodeCompanion;
  try {
    const manifestPath = require.resolve("fallow-similar-code/package.json");
    const manifest = JSON.parse(fs.readFileSync(manifestPath, "utf8"));
    if (manifest.version !== packageVersion) {
      throw new Error(
        `similar-code companion version mismatch, expected ${packageVersion} but got ${manifest.version}`,
      );
    }
    const {
      resolveBinaryArtifact,
      resolvePlatformPackage,
    } = require("fallow-similar-code/scripts/run-binary.js");
    const { verifyBinary } = require("fallow-similar-code/scripts/verify-binary.js");
    const artifact = resolveBinaryArtifact(resolvePlatformPackage());
    const verification = verifyBinary(artifact);
    if (!verification.ok) {
      const error = new Error(
        `similar-code companion verification failed: ${verification.message}`,
      );
      error.code = verification.code;
      throw error;
    }
    similarCodeCompanion = { binary: artifact.binaryPath };
  } catch (error) {
    similarCodeCompanion = { error };
  }
  return similarCodeCompanion;
};

const similarCodeProviderError = (cause) => {
  const error = new Error(
    "The exact local similar-code companion is unavailable. Reinstall @fallow-cli/fallow-node, then run `fallow similar-code setup --local`.",
  );
  error.name = "FallowNodeError";
  error.code = "FALLOW_SIMILAR_CODE_PROVIDER_NOT_READY";
  error.exitCode = 3;
  error.help = "Model setup is an explicit CLI action and is never started by the Node API.";
  error.cause = cause;
  return error;
};

// Every export declares a Promise. The native functions convert and check
// their options before they create the promise, so an invalid option throws.
// This wrapper turns that throw into a rejection.
const rejectInsteadOfThrow =
  (fn) =>
  (...args) => {
    try {
      return fn(...args);
    } catch (error) {
      return Promise.reject(error);
    }
  };

configureTypeAwareCompanion();
const binding = require("./index.js");
const nativeDetectSimilarCode = binding.detectSimilarCode;
binding.detectSimilarCode = (options = {}) => {
  const companion = resolveSimilarCodeCompanion();
  if (companion.error) throw similarCodeProviderError(companion.error);
  return nativeDetectSimilarCode({ ...options, adapterProviderPath: companion.binary });
};
for (const [name, value] of Object.entries(binding)) {
  if (typeof value === "function") binding[name] = rejectInsteadOfThrow(value);
}
module.exports = binding;
