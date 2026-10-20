import {
  cpSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  symlinkSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const PACKAGE_ROOT = fileURLToPath(new URL("../..", import.meta.url));
const FIXTURES = join(PACKAGE_ROOT, "test/fixtures");
const MUTATION_LIB = fileURLToPath(
  new URL("../../../../crates/mutation/src/lib.rs", import.meta.url),
);

/** The Vitest majors that the suite runs against, and their npm package names. */
export const VITEST_MAJORS = [
  { packageName: "vitest", version: "5.0.4" },
  { packageName: "vitest4", version: "4.1.11" },
];

/**
 * Reads the value of a `&str` constant from crates/mutation/src/lib.rs, so the
 * fixture uses the same text as the instrumentation crate.
 */
export const readMutationConstant = (name) => {
  const source = readFileSync(MUTATION_LIB, "utf8");
  const match = new RegExp(`const ${name}: &str = "((?:[^"\\\\]|\\\\.)*)";`, "u").exec(source);
  if (match === null) throw new Error(`constant ${name} not found in ${MUTATION_LIB}`);
  return match[1].replace(/\\(.)/gu, "$1");
};

const PRELUDE = readMutationConstant("PRELUDE");

const INSTRUMENTED_MATH = `// @ts-nocheck
${PRELUDE}
export function add(a: number, b: number): number {
  return (__fallow_m_a("m1") ? (a - b) : (__fallow_m_c("m1"), (a + b)));
}

export function isPositive(n: number): boolean {
  return (__fallow_m_a("m2") ? (n >= 0) : (__fallow_m_c("m2"), (n > 0)));
}

export const LIMIT = (__fallow_m_a("m3") ? (5 - 5) : (__fallow_m_c("m3"), (5 + 5)));
`;

const INSTRUMENTED_FAULTS = `// @ts-nocheck
${PRELUDE}
export function settle(n: number): number {
  return (__fallow_m_a("m4") ? (Promise.reject(new Error("late unhandled")), n) : (__fallow_m_c("m4"), (n)));
}

export async function pause(): Promise<void> {
  await new Promise((resolve) => setTimeout(resolve, (__fallow_m_a("m5") ? (20000) : (__fallow_m_c("m5"), (1)))));
}

export function shout(text: string): string {
  return (__fallow_m_a("m6") ? (null) : (__fallow_m_c("m6"), (text))).toUpperCase();
}

export const READY = (__fallow_m_a("m7") ? ((() => { throw new Error("load failure"); })()) : (__fallow_m_c("m7"), (true)));
`;

const INSTRUMENTED_UNUSED = `// @ts-nocheck
${PRELUDE}
export const unused = (): number => (__fallow_m_a("m9") ? (0) : (__fallow_m_c("m9"), (1)));
`;

const RUN_FILES = {
  "files/src/math.ts": INSTRUMENTED_MATH,
  "files/src/faults.ts": INSTRUMENTED_FAULTS,
  "files/src/unused.ts": INSTRUMENTED_UNUSED,
};

/** The whole component file with only the script block instrumented. */
const INSTRUMENTED_BADGE = `<template>
  <span>{{ label(count) }}</span>
</template>

<script>
// @ts-nocheck
${PRELUDE}
export const label = (count) => ((__fallow_m_a("m10") ? (count >= 1) : (__fallow_m_c("m10"), (count > 1))) ? "many" : "one");
</script>

<style>
.badge {
  color: red;
}
</style>
`;

const INSTRUMENTED_SHIPPING = `${PRELUDE}
const FREE_SHIPPING_MINIMUM = 50;

const shippingFee = (subtotal) => ((__fallow_m_a("m20") ? (subtotal > FREE_SHIPPING_MINIMUM) : (__fallow_m_c("m20"), (subtotal >= FREE_SHIPPING_MINIMUM))) ? 0 : 5);

module.exports = { shippingFee };
`;

const PATH_MAP = {
  "src/faults.ts": "files/src/faults.ts",
  "src/math.ts": "files/src/math.ts",
  "src/unused.ts": "files/src/unused.ts",
};

/** The optional run files, each with its path-map key. */
const OPTIONAL_FILES = {
  components: { source: "src/Badge.vue", text: INSTRUMENTED_BADGE },
  commonjs: { source: "require/shipping.cjs", text: INSTRUMENTED_SHIPPING },
};

/** The run files and the path map, with each optional file that is asked for. */
const fixtureFiles = (options) => {
  const runFiles = { ...RUN_FILES };
  const pathMap = { ...PATH_MAP };
  for (const [option, { source, text }] of Object.entries(OPTIONAL_FILES)) {
    if (!options[option]) continue;
    runFiles[`files/${source}`] = text;
    pathMap[source] = `files/${source}`;
  }
  return { runFiles, pathMap };
};

const write = (root, relativePath, text) => {
  const path = join(root, relativePath);
  mkdirSync(dirname(path), { recursive: true });
  writeFileSync(path, text);
};

/**
 * Copies the Vitest fixture project to a temp directory and writes a run
 * directory with hand-written instrumented files. A temp copy keeps the Vitest
 * cache out of the repository. The temp directory is behind a symlink on
 * macOS, which also covers the real-path comparison of the overlay.
 *
 * `packageName` selects the installed Vitest major. The project sees it as
 * `node_modules/vitest`. `fixture` selects the fixture directory; a fixture
 * other than `vitest-basic` gets no `node_modules`. `components` adds the
 * instrumented component file `src/Badge.vue` to the run directory and the
 * path map. `commonjs` adds the instrumented CommonJS file
 * `require/shipping.cjs` in the same way.
 */
export const createFixtureProject = ({
  packageName = "vitest",
  fixture = "vitest-basic",
  components = false,
  commonjs = false,
} = {}) => {
  const workspace = mkdtempSync(join(tmpdir(), `fallow-mutation-${fixture}-`));
  const root = join(workspace, "project");
  const runDir = join(workspace, "run");
  cpSync(join(FIXTURES, fixture), root, {
    recursive: true,
    filter: (source) => !source.split(/[\\/]/u).includes("node_modules"),
  });
  if (fixture === "vitest-basic") {
    mkdirSync(join(root, "node_modules"));
    symlinkSync(
      join(PACKAGE_ROOT, "node_modules", packageName),
      join(root, "node_modules", "vitest"),
      "dir",
    );
  }
  const { runFiles, pathMap } = fixtureFiles({ components, commonjs });
  for (const [path, text] of Object.entries(runFiles)) write(runDir, path, text);
  return {
    root,
    runDir,
    pathMap,
    /** Gives each mutant id in the run directory the suffix, for example `m1` to `m1b`. */
    renameMutants: (suffix) => {
      for (const path of Object.keys(runFiles).map((file) => join(runDir, file))) {
        writeFileSync(path, readFileSync(path, "utf8").replace(/"(m\d+)"/gu, `"$1${suffix}"`));
      }
    },
    cleanup: () => rmSync(workspace, { recursive: true, force: true }),
  };
};
