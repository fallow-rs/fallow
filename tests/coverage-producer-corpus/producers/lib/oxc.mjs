/**
 * Shared body of the two `oxc-coverage-instrument` rows.
 *
 * The package is this project's own instrumenter, so the manifest flags both
 * rows `self_conformance`. Their agreement with each other proves less than
 * either one agreeing with a third-party producer.
 */

import { instrument } from "oxc-coverage-instrument";

import { collectInstrumentedCoverage, withExecutedCounters } from "./execute.mjs";

/**
 * @param {{ source: string, filename: string, typescript: boolean }} probe
 * @param {"istanbul" | undefined} compat Compatibility preset, or `undefined`
 *   for the producer's own default shape.
 * @returns {Record<string, unknown> | null} One `FileCoverage`.
 */
export const emitOxc = ({ source, filename, typescript }, compat) => {
  const result = instrument(source, filename, {
    compat,
    coverageVariable: "__coverage__",
    sourceType: typescript ? "ts" : "module",
  });
  const template = JSON.parse(result.coverageMap);
  const executed = collectInstrumentedCoverage(result.code, filename);
  return withExecutedCounters(template, executed?.[filename]);
};
