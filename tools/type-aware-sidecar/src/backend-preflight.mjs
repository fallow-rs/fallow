import { createRequire } from "node:module";
import path from "node:path";

import { selectBackend } from "./backend-selection.mjs";
import { BACKEND_VERSION, DEFAULT_BACKEND_FAMILY } from "./generated-protocol.mjs";

const REQUIRED_MAJOR = Number.parseInt(BACKEND_VERSION, 10);
const INSTALL_HINT =
  "run `npm ci` in tools/type-aware-sidecar so the sidecar resolves its own typescript install";

/**
 * Verify that module resolution hands the sidecar a typescript install that
 * provides the `typescript/unstable/sync` backend entry point.
 *
 * The semantic modules import that entry point statically, so an incompatible
 * resolution (for example a typescript 6 hoisted into an ancestor
 * `node_modules`) fails at link time with a bare module path instead of the
 * version conflict that caused it. This preflight runs before those imports
 * and names the resolved version, its location, and the fix. Exact
 * backend-version parity stays enforced in `protocol.mjs`.
 *
 * A backend that `FALLOW_TYPE_AWARE_BACKEND` selects instead of the default
 * must resolve at its exact pinned version.
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
  if (backend.family !== DEFAULT_BACKEND_FAMILY) {
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
  const major = Number.parseInt(version, 10);
  if (Number.isFinite(major) && major >= REQUIRED_MAJOR) {
    return;
  }
  throw new Error(
    `resolved typescript ${version} from ${path.dirname(manifestPath)}, which lacks the ` +
      `typescript/unstable/sync backend; fallow-type-aware needs typescript ${BACKEND_VERSION}; ` +
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
  if (version === backend.version) return;
  throw new Error(
    `resolved ${backend.package} ${version} from ${path.dirname(manifestPath)}; ` +
      `the ${backend.family} backend needs exactly ${backend.version}; ${install}`,
  );
};
