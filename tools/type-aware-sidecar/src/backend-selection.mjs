import { BACKENDS, BACKEND_FAMILY } from "./generated-protocol.mjs";

const BACKEND_ENV = "FALLOW_TYPE_AWARE_BACKEND";

/**
 * Resolve the backend that `FALLOW_TYPE_AWARE_BACKEND` selects. An unset or
 * empty value selects the default backend.
 *
 * @param {NodeJS.ProcessEnv} env
 * @returns {{ family: string, package: string, version: string }}
 */
export const selectBackend = (env = process.env) => {
  const requested = env[BACKEND_ENV]?.trim() || BACKEND_FAMILY;
  const backend = BACKENDS.find((candidate) => candidate.family === requested);
  if (backend) return backend;
  const known = BACKENDS.map((candidate) => candidate.family).join(", ");
  throw new Error(`unknown ${BACKEND_ENV} value \`${requested}\`; expected one of: ${known}`);
};

const versionCore = (version) => {
  const match = /^(\d+)\.(\d+)\.(\d+)(?:[-+]|$)/u.exec(version);
  return match ? match.slice(1).map(Number) : undefined;
};

const compareCores = (left, right) =>
  left.map((part, index) => part - right[index]).find((difference) => difference !== 0) ?? 0;

/**
 * Whether a resolved backend package version is in the range that the
 * protocol manifest supports. Only `major.minor.patch` counts, so a tsrs build
 * tag such as `-ts7.1.0-beta` or a TypeScript prerelease stays in range.
 *
 * @param {{ minimum_version: string, below_version: string }} backend
 * @param {string} version
 */
export const supportsVersion = (backend, version) => {
  const core = versionCore(version);
  if (!core) return false;
  return (
    compareCores(core, versionCore(backend.minimum_version)) >= 0 &&
    compareCores(core, versionCore(backend.below_version)) < 0
  );
};

/** @param {{ minimum_version: string, below_version: string }} backend */
export const supportedRange = (backend) => `>=${backend.minimum_version} <${backend.below_version}`;
