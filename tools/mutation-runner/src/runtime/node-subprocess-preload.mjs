import { reportChildHitsAtExit } from "./child-coverage.mjs";
import { readOverlay, recordOverlayError } from "./child.mjs";
import { installFallowMutation } from "./global.mjs";
import { registerOverlayHook } from "./overlay-hook.mjs";

// The node:test preload and the Vitest setup file add `--import` of this file
// to NODE_OPTIONS, so each Node process that a test starts loads it. The URL
// carries the overlay path and the active mutant, because the test
// environment has no Fallow variable. In a dry run, the URL also carries the
// token of the test that started this process, and this process gives its
// hits to that test at exit (see `child-coverage.mjs`). This process runs no
// test, so each hit is a module-scope hit of the runtime, and all of them
// belong to the test.
const params = new URL(import.meta.url).searchParams;
const mutation = installFallowMutation();
mutation.setActive(params.get("active"));
const overlayPath = params.get("overlay");
const loaded = new Set();
registerOverlayHook(
  readOverlay(overlayPath),
  (entry) => loaded.add(entry.source),
  (message) => recordOverlayError(overlayPath, message),
);
reportChildHitsAtExit(overlayPath, params.get("test"), () => {
  const { hits, staticHits } = mutation.finishTest();
  return { hits: [...new Set([...hits, ...staticHits])], loaded: [...loaded] };
});
