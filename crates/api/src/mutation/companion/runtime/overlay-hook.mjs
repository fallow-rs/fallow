// Generated from tools/mutation-runner/src/runtime/overlay-hook.mjs. Do not edit.
import { readFileSync } from "node:fs";
import { registerHooks } from "node:module";
import { fileURLToPath } from "node:url";

import { realPath } from "./child.mjs";

/**
 * Serves the instrumented file of each overlay entry in place of its source
 * file. `onLoad(entry)` runs for each served file.
 *
 * A synchronous hook runs in this thread for `import` and `require`. It keeps
 * the format of the original file, so Node strips the types of a TypeScript
 * file as it does without the overlay.
 */
export const registerOverlayHook = (overlay, onLoad = () => {}) => {
  const overlayEntry = (url) => {
    if (!url.startsWith("file:")) return undefined;
    return overlay.get(realPath(fileURLToPath(url)));
  };
  registerHooks({
    load(url, context, nextLoad) {
      const result = nextLoad(url, context);
      const entry = overlayEntry(url);
      if (entry === undefined) return result;
      onLoad(entry);
      return { ...result, source: readFileSync(entry.target, "utf8"), shortCircuit: true };
    },
  });
};
