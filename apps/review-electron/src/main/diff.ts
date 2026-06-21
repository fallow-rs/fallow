import { execFile } from "node:child_process";
import { promisify } from "node:util";

const run = promisify(execFile);

export type FileDiff = { patch: string; binary: boolean };

/**
 * Unified `git diff <base> -- <file>` for a changed file (base = the review's
 * merge-base). New-since-base files show as all-additions; binary files are
 * flagged. Errors degrade to an empty patch (the UI shows "no textual diff").
 */
export const getFileDiff = async (root: string, base: string, file: string): Promise<FileDiff> => {
  const ref = base || "HEAD";
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
