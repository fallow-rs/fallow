// Generated from tools/mutation-runner/src/runtime/overlay-hook.mjs. Do not edit.
import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import { registerHooks } from "node:module";
import { fileURLToPath } from "node:url";

import { realPath } from "./child.mjs";

const SCRIPT_FORMATS = new Set(["module", "commonjs", "module-typescript", "commonjs-typescript"]);
const digest = (bytes) => createHash("sha256").update(bytes).digest("hex");

const indexSources = (overlay) => {
  const index = new Map();
  for (const [path, entry] of overlay) {
    const bytes = readFileSync(path);
    const key = digest(bytes);
    const matches = index.get(key) ?? [];
    matches.push({ bytes, entry });
    index.set(key, matches);
  }
  return index;
};

/**
 * Serves the instrumented file of each overlay entry in place of its source
 * file. `onLoad(entry)` runs for each served file.
 *
 * A synchronous hook runs in this thread for `import` and `require`. It keeps
 * the format of the original file, so Node strips the types of a TypeScript
 * file as it does without the overlay.
 */
export const registerOverlayHook = (overlay, onLoad, onIncomplete) => {
  let sources;
  const overlayEntry = (url, result) => {
    if (!url.startsWith("file:")) return undefined;
    const path = realPath(fileURLToPath(url));
    const direct = overlay.get(path);
    if (direct !== undefined) return direct;
    if (!SCRIPT_FORMATS.has(result.format) || result.source == null) return undefined;
    sources ??= indexSources(overlay);
    const bytes = ArrayBuffer.isView(result.source)
      ? Buffer.from(result.source.buffer, result.source.byteOffset, result.source.byteLength)
      : Buffer.from(result.source);
    const matches = (sources.get(digest(bytes)) ?? []).filter((match) => match.bytes.equals(bytes));
    if (matches.length > 1) {
      const candidates = matches.map(({ entry }) => entry.source).join(", ");
      onIncomplete(`Ambiguous copied-source execution at ${path}: matches ${candidates}`);
    }
    return matches.length === 1 ? matches[0].entry : undefined;
  };
  registerHooks({
    load(url, context, nextLoad) {
      const result = nextLoad(url, context);
      const entry = overlayEntry(url, result);
      if (entry === undefined) return result;
      onLoad(entry);
      return { ...result, source: readFileSync(entry.target, "utf8"), shortCircuit: true };
    },
  });
};
