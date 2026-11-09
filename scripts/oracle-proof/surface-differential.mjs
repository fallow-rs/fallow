// The surface differential of crates/mcp/tests/integration/surface_differential,
// ported onto commits from before it existed.

import { cpSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";

const TESTS = "crates/mcp/tests/integration/surface_differential";
const MAIN = "crates/mcp/tests/integration/main.rs";
const REGISTRATION =
  "// The surface differential builds `file://` URIs from Unix paths.\n#[cfg(unix)]\nmod surface_differential;\n";

const registerTestModule = (tree) => {
  const path = join(tree, MAIN);
  const text = readFileSync(path, "utf8");
  if (/^mod surface_differential;$/m.test(text)) return;
  const anchor = "\nmod resources;\n";
  if (!text.includes(anchor)) {
    throw new Error(`${MAIN}: the \`mod resources;\` line is missing`);
  }
  writeFileSync(
    path,
    text.replace(anchor, () => `${anchor}${REGISTRATION}`),
  );
};

/** Install the current surface differential in `tree`. Returns a note per change. */
const port = (tree, source) => {
  cpSync(join(source, TESTS), join(tree, TESTS), { recursive: true });
  registerTestModule(tree);
  return [`copied ${TESTS} from the source checkout`];
};

const TEST_FILTER = "surface_differential::";
const CARGO_TEST = ["test", "-p", "fallow-mcp", "--test", "integration"];
const testOnly = (name) => [
  "cargo",
  [...CARGO_TEST, `surface_differential::oracle::${name}`, "--", "--exact"],
];

/**
 * The surface differential: the CLI, MCP, the language server and the Node
 * binding path give the same facts, `trace` agrees with the report, and the
 * cache controls have the same effect on each surface.
 */
export const surfaceDifferential = {
  name: "surface-differential",
  description: "the CLI, MCP, LSP and Node binding path give the same facts for one project",
  fixes: [
    {
      // Before 257e1e054 (#3223), `--trace-dependency` credited a tool that a
      // sibling workspace configures, while the report flagged it.
      commit: "257e1e054",
      scope: "the trace check",
      reason: "the commit is older than the clone family order fix",
      test: testOnly("traces_agree_with_the_report_on_every_surface"),
    },
    {
      // Before 63672da47 (#3390), `audit`, `dupes --changed-since` and
      // `dupes --baseline` ordered clone families by duplicated lines.
      commit: "63672da47",
      scope: "the clone family check",
      reason: "the other checks do not test this fix",
      test: testOnly("clone_families_agree_on_every_surface_and_scope"),
    },
  ],
  port,
  backports: [],
  // The test binary first, then the binaries that it runs, so that the
  // test build cannot replace them with an older link.
  build: [
    "sh",
    [
      "-c",
      `cargo ${[...CARGO_TEST, "--no-run"].join(" ")} && cargo build -p fallow-cli -p fallow-lsp`,
    ],
  ],
  test: ["cargo", [...CARGO_TEST, TEST_FILTER]],
};
