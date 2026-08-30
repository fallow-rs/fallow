/**
 * `ast-v8-to-istanbul`, the converter behind Vitest's default V8 provider.
 *
 * Geometry this row pins: it matches nested callbacks that never executed, and
 * it serializes `loc.end.column` as JSON `null` for a record whose end column
 * it cannot place, which deserializes to `0` and inverts the body span.
 */

import { pathToFileURL } from "node:url";

import { parse } from "acorn";
import convert from "ast-v8-to-istanbul";

import { collectV8Coverage } from "../lib/execute.mjs";

export const id = "ast-v8-to-istanbul";

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
  const ast = parse(source, {
    ecmaVersion: "latest",
    locations: true,
    ranges: true,
    sourceType: "module",
  });
  const map = await convert({
    ast,
    code: source,
    coverage: { functions: coverage.functions, url: pathToFileURL(filename).href },
    wrapperLength: 0,
  });
  const entry = Object.values(map)[0];
  return entry ?? null;
};
