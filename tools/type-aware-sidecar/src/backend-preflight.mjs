import { createRequire } from "node:module";
import path from "node:path";

import { selectBackend, supportedRange, supportsVersion } from "./backend-selection.mjs";
import { BACKEND_VERSION, BACKEND_FAMILY } from "./generated-protocol.mjs";

const INSTALL_HINT =
  "run `npm ci` in tools/type-aware-sidecar so the sidecar resolves its own typescript install";

/**
 * Verify that module resolution hands the sidecar a typescript install that
 * provides the `typescript` sync API backend entry point.
 *
 * The semantic modules import that entry point statically, so an incompatible
 * resolution (for example a typescript 6 hoisted into an ancestor
 * `node_modules`) fails at link time with a bare module path instead of the
 * version conflict that caused it. This preflight runs before those imports
 * and names the resolved version, its location, and the fix.
 *
 * A backend that `FALLOW_TYPE_AWARE_BACKEND` selects instead of the default
 * must resolve at a version in its supported range, so a project can reuse the
 * tsrs it already installs.
 *
 * @param {string | URL} resolveFrom Module URL or file path to resolve from;
 *   defaults to this module so the check mirrors the semantic imports.
 * @param {NodeJS.ProcessEnv} env Environment that selects the backend.
 */
export const assertTypescriptBackendResolvable = (
  resolveFrom = import.meta.url,
  env = process.env,
) => {
  const backend = selectBackend(env);
  const sidecarRequire = createRequire(resolveFrom);
  if (backend.family !== BACKEND_FAMILY) {
    assertPinnedBackendResolvable(sidecarRequire, backend);
    return;
  }
  let manifestPath;
  try {
    manifestPath = sidecarRequire.resolve("typescript/package.json");
  } catch {
    throw new Error(`typescript is not installed; ${INSTALL_HINT}`);
  }
  const { version } = sidecarRequire(manifestPath);
  if (supportsVersion(backend, version)) {
    return;
  }
  throw new Error(
    `resolved typescript ${version} from ${path.dirname(manifestPath)}, which the ` +
      `typescript sync API backend does not support; fallow-type-aware needs typescript ` +
      `${supportedRange(backend)} (tested with ${BACKEND_VERSION}); ` +
      INSTALL_HINT,
  );
};

const assertPinnedBackendResolvable = (sidecarRequire, backend) => {
  const install = `install ${backend.package}@${backend.version} where fallow-type-aware can resolve it`;
  let manifestPath;
  try {
    manifestPath = sidecarRequire.resolve(`${backend.package}/package.json`);
  } catch {
    throw new Error(`the ${backend.family} backend is not installed; ${install}`);
  }
  const { version } = sidecarRequire(manifestPath);
  if (supportsVersion(backend, version)) return;
  throw new Error(
    `resolved ${backend.package} ${version} from ${path.dirname(manifestPath)}; ` +
      `the ${backend.family} backend supports ${supportedRange(backend)}; ${install}`,
  );
};
