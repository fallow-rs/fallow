/**
 * `istanbul-lib-instrument`, the instrumenter behind Jest and behind Vitest's
 * `--coverage.provider istanbul`.
 *
 * Geometry this row pins: a decorated class member records `decl.start` at the
 * decorator and `loc.start` at the body brace; class methods and accessors are
 * recorded as `(anonymous_N)`; a named function expression is anchored at its
 * identifier rather than at the `function` keyword.
 */

import { createInstrumenter } from "istanbul-lib-instrument";

import { collectInstrumentedCoverage, withExecutedCounters } from "../lib/execute.mjs";

const TYPESCRIPT_PLUGINS = ["typescript", "decorators-legacy"];

export const id = "istanbul-lib-instrument";

/**
 * @param {{ source: string, filename: string, typescript: boolean }} probe
 * @returns {Record<string, unknown> | null} One `FileCoverage`, or `null` when
 *   the producer cannot instrument the probe.
 */
export const emit = ({ source, filename, typescript }) => {
  const instrumenter = createInstrumenter({
    coverageVariable: "__coverage__",
    esModules: true,
    parserPlugins: typescript ? TYPESCRIPT_PLUGINS : [],
    produceSourceMap: false,
  });
  const code = instrumenter.instrumentSync(source, filename);
  const template = instrumenter.lastFileCoverage();
  const executed = collectInstrumentedCoverage(code, filename);
  return withExecutedCounters(template, executed?.[filename]);
};
