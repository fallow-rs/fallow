/**
 * `oxc-coverage-instrument` under `compat: "istanbul"`, its preset for
 * matching `istanbul-lib-instrument` wherever the typed model permits.
 *
 * Geometry this row pins: where the preset moves an anchor relative to the
 * producer's own default shape.
 */

import { emitOxc } from "../lib/oxc.mjs";

export const id = "oxc-coverage-instrument-istanbul";

/**
 * @param {{ source: string, filename: string, typescript: boolean }} probe
 * @returns {Record<string, unknown> | null} One `FileCoverage`.
 */
export const emit = (probe) => emitOxc(probe, "istanbul");
