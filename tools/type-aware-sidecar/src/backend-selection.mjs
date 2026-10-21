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
