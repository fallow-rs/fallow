// Check that a PGO build found profile records for its functions.
//
// Usage:
//   node pgo-profile-match.mjs --log <cargo-build.log> --functions <count>
//     [--max-missing-ratio 0.2] [--title <text>] [--summary <file>]
//
// The build must pass `-Cllvm-args=-pgo-warn-missing-function`. LLVM then
// prints one warning for each function that has no record in the profile.
// Cargo shows these warnings only for workspace crates. `--functions` is the
// `Total functions` value of `llvm-profdata show` for the profile.
//
// A profile from another target, another host toolchain, or another package
// set matches almost no function, because Cargo hashes these into the crate
// symbol names. Such a profile still changes the code, so a size or identity
// check cannot find it. A matching build also has some missing functions:
// the linker removes the records of unused functions from the trained binary.
// On aarch64-apple-darwin, a matching build had 4% of the profile function
// count as missing functions, and a build of another package set had 51%.

import { appendFileSync, readFileSync, realpathSync } from "node:fs";
import { pathToFileURL } from "node:url";
import { parseArgs } from "node:util";

const MISSING_FUNCTION = /no profile data available for function/u;
const DEFAULT_MAX_MISSING_RATIO = "0.2";

/** Counts the LLVM warnings for functions without a profile record. */
export const countMissingFunctions = (log) =>
  log.split(/\r?\n/u).filter((line) => MISSING_FUNCTION.test(line)).length;

/** Returns the failure message, or null when the profile matches the build. */
export const matchFailure = ({ missing, functions, maxMissingRatio }) => {
  if (!(functions > 0)) return "the profile has no functions";
  const ratio = missing / functions;
  if (ratio <= maxMissingRatio) return null;
  return `${missing} functions have no profile record, ${(ratio * 100).toFixed(1)}% of the ${functions} profile functions; the limit is ${(maxMissingRatio * 100).toFixed(1)}%. The profile does not match this build`;
};

export const main = (argv = process.argv.slice(2)) => {
  const { values } = parseArgs({
    args: argv,
    options: {
      log: { type: "string" },
      functions: { type: "string" },
      "max-missing-ratio": { type: "string", default: DEFAULT_MAX_MISSING_RATIO },
      title: { type: "string", default: "Profile match" },
      summary: { type: "string" },
    },
  });
  const functions = Number(values.functions);
  const maxMissingRatio = Number(values["max-missing-ratio"]);
  if (!values.log || !Number.isInteger(functions) || !Number.isFinite(maxMissingRatio)) {
    console.error(
      "usage: pgo-profile-match.mjs --log <file> --functions <count> [--max-missing-ratio <ratio>]",
    );
    return 2;
  }
  const missing = countMissingFunctions(readFileSync(values.log, "utf8"));
  const failure = matchFailure({ missing, functions, maxMissingRatio });
  const summary = [
    `## ${values.title}`,
    "",
    `Functions without a profile record: ${missing} of ${functions} profile functions`,
    "",
    failure === null ? "Profile match: pass" : `Profile match: fail\n\n- ${failure}`,
    "",
  ].join("\n");
  process.stdout.write(summary);
  if (values.summary) appendFileSync(values.summary, summary);
  return failure === null ? 0 : 1;
};

// import.meta.url holds the real path, encoded as a URL. Compare it with the
// same form of argv[1], so a symlink, a space or a Windows path still runs main.
const isEntryPoint = () =>
  process.argv[1] !== undefined &&
  import.meta.url === pathToFileURL(realpathSync(process.argv[1])).href;

if (isEntryPoint()) {
  process.exitCode = main();
}
