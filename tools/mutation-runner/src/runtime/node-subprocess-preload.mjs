import { readOverlay, recordOverlayError } from "./child.mjs";
import { installFallowMutation } from "./global.mjs";
import { registerOverlayHook } from "./overlay-hook.mjs";

// The node:test preload adds `--import` of this file to NODE_OPTIONS, so each
// Node process that a test starts loads it. The URL carries the overlay path
// and the active mutant, because the test environment has no Fallow
// variable. This process is not a test process, so it reports no hits.
const params = new URL(import.meta.url).searchParams;
installFallowMutation().setActive(params.get("active"));
const overlayPath = params.get("overlay");
registerOverlayHook(
  readOverlay(overlayPath),
  () => {},
  (message) => recordOverlayError(overlayPath, message),
);
