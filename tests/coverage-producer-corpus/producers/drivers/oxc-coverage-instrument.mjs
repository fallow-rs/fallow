/**
 * `oxc-coverage-instrument` in its own default shape.
 *
 * Geometry this row pins: an accessor is recorded as `get area` / `set area`,
 * which raw V8 coverage also does and `istanbul-lib-instrument` does not.
 */

import { emitOxc } from "../lib/oxc.mjs";

export const id = "oxc-coverage-instrument";

/**
 * @param {{ source: string, filename: string, typescript: boolean }} probe
 * @returns {Record<string, unknown> | null} One `FileCoverage`.
 */
export const emit = (probe) => emitOxc(probe, undefined);
