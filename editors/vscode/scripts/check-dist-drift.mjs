// Guards against shipping a stale `dist/extension.js`: a commit that edits
// `src/` but forgets to rebuild the bundle (the marketplace VSIX bundles
// `dist/extension.js`, so stale dist means shipped src changes never reach
// users). This recurred across #902/#903/#907/#908.
//
// A byte-exact `git diff --exit-code dist/` cannot be used as the gate: the
// rolldown minifier is non-deterministic ACROSS environments (the committed
// dist is built on the maintainer's machine, CI rebuilds on ubuntu), producing
// a few bytes of jitter even from identical source. Two builds in the SAME
// environment are byte-identical, but cross-environment they are not.
//
// So this gate compares the byte SIZE of a fresh rebuild against the committed
// bundle and fails only when the delta exceeds a generous tolerance. That
// catches a forgotten rebuild of a real feature (hundreds-to-thousands of
// bytes of added/removed code) while tolerating the documented minifier jitter.
//
// Usage: build the bundle first (`pnpm run build`), then run this against the
// committed size captured from git.

import { execFileSync } from "node:child_process";
import { statSync } from "node:fs";
import { resolve } from "node:path";

const DIST = "dist/extension.js";
const repoRoot = resolve(import.meta.dirname, "..");
const distPath = resolve(repoRoot, DIST);

// Absolute byte tolerance for cross-environment minifier jitter. The observed
// same-version cross-env jitter is single-digit bytes; this leaves ample
// headroom while staying far below a real feature's footprint (a single new
// command/view adds thousands of minified bytes).
const TOLERANCE_BYTES = 2048;

/** Byte size of the committed `dist/extension.js` at HEAD, or null if absent. */
const committedSize = () => {
  try {
    const raw = execFileSync("git", ["cat-file", "-s", `HEAD:editors/vscode/${DIST}`], {
      cwd: repoRoot,
      encoding: "utf8",
    });
    return Number.parseInt(raw.trim(), 10);
  } catch {
    return null;
  }
};

const rebuiltSize = () => statSync(distPath).size;

const committed = committedSize();
if (committed === null || Number.isNaN(committed)) {
  console.error(
    `dist-drift: could not read committed size of ${DIST} from HEAD. ` +
      "Is the bundle committed?",
  );
  process.exit(1);
}

const rebuilt = rebuiltSize();
const delta = Math.abs(rebuilt - committed);

if (delta > TOLERANCE_BYTES) {
  console.error(
    `dist-drift: ${DIST} is stale. Committed ${committed} bytes, fresh build ` +
      `${rebuilt} bytes (delta ${delta} > ${TOLERANCE_BYTES} tolerance). ` +
      "Run `pnpm run build` in editors/vscode and commit the updated dist/.",
  );
  process.exit(1);
}

console.log(
  `dist-drift: OK (committed ${committed} bytes, rebuilt ${rebuilt} bytes, ` +
    `delta ${delta} <= ${TOLERANCE_BYTES}).`,
);
