import { isAbsolute, resolve } from "node:path";

/**
 * The Cargo target directory: `CARGO_TARGET_DIR` when it is set, else
 * `<repoRoot>/target`. A relative `CARGO_TARGET_DIR` resolves against the
 * repository root, where Cargo runs for these scripts.
 */
export const cargoTargetDir = (repoRoot, env = process.env) => {
  const configured = env.CARGO_TARGET_DIR;
  if (!configured) {
    return resolve(repoRoot, "target");
  }
  return isAbsolute(configured) ? configured : resolve(repoRoot, configured);
};

/** The path of the built `fallow` binary for a Cargo profile directory. */
export const cargoFallowBin = (repoRoot, profile = "debug", env = process.env) =>
  resolve(cargoTargetDir(repoRoot, env), profile, "fallow");
