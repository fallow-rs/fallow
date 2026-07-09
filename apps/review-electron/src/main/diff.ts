import { execFile } from "node:child_process";
import { promisify } from "node:util";

const run = promisify(execFile);

export type FileDiff = { patch: string; binary: boolean };

/** Refs and revision expressions git accepts: SHAs, branch names, `HEAD`, `HEAD~2`, `origin/main`. */
const SAFE_REF = /^[A-Za-z0-9][A-Za-z0-9._/^~-]*$/;

/**
 * Reject anything that isn't a well-formed git ref, falling back to `HEAD`.
 * Guards against option injection (a ref beginning with `-`, e.g.
 * `--output=/tmp/x`, would otherwise be parsed by git as a flag).
 */
export const sanitizeRef = (ref: string): string => (SAFE_REF.test(ref) ? ref : "HEAD");

/**
 * Unified `git diff <base> -- <file>` for a changed file (base = the review's
 * merge-base). New-since-base files show as all-additions; binary files are
 * flagged. Errors degrade to an empty patch (the UI shows "no textual diff").
 */
export const getFileDiff = async (root: string, base: string, file: string): Promise<FileDiff> => {
  const ref = sanitizeRef(base || "HEAD");
  try {
    const { stdout } = await run("git", ["diff", ref, "--", file], {
      cwd: root,
      maxBuffer: 32 * 1024 * 1024,
    });
    return { patch: stdout, binary: /^Binary files /m.test(stdout) };
  } catch {
    return { patch: "", binary: false };
  }
};

/**
 * Full `git diff <base>` across every changed file (no path filter), for the
 * "all files" diff shown when no single file is selected. The renderer splits
 * the multi-file patch into per-file sections.
 */
export const getAllDiffs = async (root: string, base: string): Promise<{ patch: string }> => {
  const ref = sanitizeRef(base || "HEAD");
  try {
    const { stdout } = await run("git", ["diff", ref], {
      cwd: root,
      maxBuffer: 64 * 1024 * 1024,
    });
    return { patch: stdout };
  } catch {
    return { patch: "" };
  }
};
