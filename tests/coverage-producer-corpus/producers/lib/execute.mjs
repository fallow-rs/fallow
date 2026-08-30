/**
 * Execution helpers shared by the pinned coverage-producer drivers.
 *
 * Two kinds of producer live in this corpus. An instrumenter rewrites the
 * source and the counters come from running that rewritten code. A V8
 * converter reads `ScriptCoverage` and the counters come from running the
 * original source under Node's own coverage collector. Both are real
 * executions of the probe: no counter in a recorded map is synthesized.
 */

import { execFileSync } from "node:child_process";
import {
  mkdtempSync,
  readFileSync,
  readdirSync,
  realpathSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { pathToFileURL } from "node:url";
import { createContext, runInContext } from "node:vm";

const COVERAGE_VARIABLE = "__coverage__";

/**
 * Run instrumented code in a throwaway context and return its coverage global.
 *
 * @param {string} code Instrumented source.
 * @param {string} filename Absolute path the instrumenter recorded.
 * @returns {Record<string, unknown> | null} `__coverage__`, or `null` when the
 *   instrumented output is not executable (TypeScript that kept its syntax).
 */
export const collectInstrumentedCoverage = (code, filename) => {
  const sandbox = { console: { log: () => {} } };
  sandbox.globalThis = sandbox;
  const context = createContext(sandbox);
  try {
    runInContext(code, context, { filename });
  } catch {
    return null;
  }
  const coverage = sandbox[COVERAGE_VARIABLE];
  return coverage === undefined ? null : coverage;
};

/**
 * Execute `sourcePath` in a fresh Node process under `NODE_V8_COVERAGE` and
 * return the raw `ScriptCoverage` for that file.
 *
 * @param {string} sourcePath Absolute path of the probe source.
 * @returns {{ url: string, functions: unknown[] } | null} The script's V8
 *   coverage, or `null` when Node could not execute the probe.
 */
export const collectV8Coverage = (sourcePath) => {
  const coverageDirectory = mkdtempSync(join(tmpdir(), "fallow-producer-v8-"));
  try {
    execFileSync(process.execPath, [sourcePath], {
      env: { ...process.env, NODE_V8_COVERAGE: coverageDirectory },
      stdio: "ignore",
    });
  } catch {
    return null;
  }
  try {
    const wanted = pathToFileURL(sourcePath).href;
    for (const entry of readdirSync(coverageDirectory)) {
      const report = JSON.parse(readFileSync(join(coverageDirectory, entry), "utf8"));
      const script = report.result?.find((candidate) => candidate.url === wanted);
      if (script !== undefined) {
        return { functions: script.functions, url: script.url };
      }
    }
    return null;
  } finally {
    rmSync(coverageDirectory, { force: true, recursive: true });
  }
};

/**
 * Write `source` to a scratch file, hand the path to `use`, then delete it.
 *
 * @template T
 * @param {string} source Probe source.
 * @param {string} basename File name the probe must keep, because a producer
 *   infers its parser from the extension.
 * @param {(path: string) => T} use Receives the absolute scratch path.
 * @returns {T}
 */
export const withScratchFile = (source, basename, use) => {
  const directory = mkdtempSync(join(tmpdir(), "fallow-producer-src-"));
  const path = join(directory, basename);
  writeFileSync(path, source);
  try {
    // Node reports V8 coverage under the realpath of the module it loaded, and
    // the platform temporary directory is a symlink on macOS. Handing out the
    // symlinked spelling would make every `ScriptCoverage` lookup miss.
    return use(realpathSync(path));
  } finally {
    rmSync(directory, { force: true, recursive: true });
  }
};

/**
 * Copy the executed counters of `recorded` onto the pristine `template`.
 *
 * An instrumenter returns the map template before execution and the executed
 * program owns its own copy. Only the counter arrays differ, so the recorded
 * map keeps the producer's own geometry and gains its own real counts.
 *
 * @param {Record<string, unknown>} template Producer-emitted `FileCoverage`.
 * @param {Record<string, unknown> | null | undefined} recorded Executed copy.
 * @returns {Record<string, unknown>} `template` with executed counters.
 */
export const withExecutedCounters = (template, recorded) => {
  if (recorded === null || recorded === undefined) {
    return template;
  }
  return { ...template, b: recorded.b, f: recorded.f, s: recorded.s };
};
