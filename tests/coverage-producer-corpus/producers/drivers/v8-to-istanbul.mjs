/**
 * `v8-to-istanbul`, the converter behind c8 and behind nyc in V8 mode.
 *
 * Geometry this row pins: function records carry real names where V8 knows
 * them, a named function expression is anchored at the `function` keyword
 * rather than at its identifier, and the implicit else of a bare `if` is
 * recorded with `column: -1`.
 */

import v8ToIstanbul from "v8-to-istanbul";

import { collectV8Coverage } from "../lib/execute.mjs";

export const id = "v8-to-istanbul";

/**
 * @param {{ source: string, sourcePath: string, filename: string }} probe
 * @returns {Promise<Record<string, unknown> | null>} One `FileCoverage`, or
 *   `null` when Node cannot execute the probe.
 */
export const emit = async ({ source, sourcePath, filename }) => {
  const coverage = collectV8Coverage(sourcePath);
  if (coverage === null) {
    return null;
  }
  const converter = v8ToIstanbul(filename, 0, { source });
  await converter.load();
  converter.applyCoverage(coverage.functions);
  const map = converter.toIstanbul();
  const entry = Object.values(map)[0];
  return entry ?? null;
};
