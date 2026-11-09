// The platform oracle of crates/cli/tests/integration/platform, ported onto
// commits from before it existed.

import { cpSync, existsSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";

const TESTS = "crates/cli/tests/integration/platform";
const INTEGRATION_MAIN = "crates/cli/tests/integration/main.rs";
/** The test target that the port adds when the commit has no integration binary. */
const OWN_TARGET = "crates/cli/tests/platform_oracle.rs";
const OWN_TARGET_SOURCE = `//! The platform oracle, as its own test target on a commit that has one
//! test binary per file.

// An old commit may not use each lint or helper, so \`expect\` would be an
// unfulfilled lint expectation there.
#![allow(dead_code, clippy::unwrap_used, clippy::expect_used)]

mod common;
#[path = "integration/platform/mod.rs"]
mod platform;
`;

/**
 * The modules that call library code. A commit from before that code keeps
 * the module only when each anchor is in its tree.
 */
const OPTIONAL_MODULES = [
  {
    name: "seams",
    anchors: [
      ["crates/types/src/path_util.rs", "pub fn display_relative"],
      ["crates/types/src/identity.rs", "pub fn stamp_dead_code_finding_ids"],
      ["crates/engine/src/baseline/mod.rs", "pub fn from_results_with_identity"],
      ["crates/engine/src/baseline/mod.rs", "pub fn filter_new_clone_groups"],
      ["crates/engine/src/baseline/mod.rs", "pub fn apply_dead_code_baseline"],
      ["crates/api/src/lib.rs", "build_duplication_codeclimate"],
    ],
  },
  {
    name: "health_rows",
    anchors: [
      ["crates/api/src/lib.rs", "build_health_groups_markdown"],
      ["crates/cli/Cargo.toml", "fallow-api"],
    ],
  },
];

const hasAnchor = (tree, [file, text]) => {
  const path = join(tree, file);
  return existsSync(path) && readFileSync(path, "utf8").includes(text);
};

const registerModule = (tree) => {
  const path = join(tree, INTEGRATION_MAIN);
  if (!existsSync(path)) {
    writeFileSync(join(tree, OWN_TARGET), OWN_TARGET_SOURCE);
    return `added the test target ${OWN_TARGET}`;
  }
  const text = readFileSync(path, "utf8");
  if (/^mod platform;$/m.test(text)) return null;
  const first = text.search(/^mod \w+_tests;$/m);
  if (first < 0) throw new Error(`${INTEGRATION_MAIN} has no test module line`);
  writeFileSync(path, `${text.slice(0, first)}mod platform;\n${text.slice(first)}`);
  return null;
};

/**
 * Install the current platform oracle in `tree`. Drop each module whose
 * library code the commit does not have. Returns a note per change.
 */
const port = (tree, source) => {
  const notes = [`copied ${TESTS} from the source checkout`];
  cpSync(join(source, TESTS), join(tree, TESTS), { recursive: true });
  const modPath = join(tree, TESTS, "mod.rs");
  let modText = readFileSync(modPath, "utf8");
  for (const module of OPTIONAL_MODULES) {
    if (module.anchors.every((anchor) => hasAnchor(tree, anchor))) continue;
    modText = modText.replace(`mod ${module.name};\n`, "");
    notes.push(`left out the ${module.name} module: the commit does not have its library code`);
  }
  writeFileSync(modPath, modText);
  const registered = registerModule(tree);
  if (registered) notes.push(registered);
  return notes;
};

const integrationTest = (filter) => [
  "cargo",
  ["test", "-p", "fallow-cli", "--test", "integration", filter],
];
const integrationBuild = [
  "cargo",
  ["test", "-p", "fallow-cli", "--test", "integration", "--no-run"],
];

/**
 * The platform oracle: CRLF line endings, backslash separators and a drive
 * root do not change findings, ids, clone fingerprints or baselines.
 */
export const platform = {
  name: "platform",
  description:
    "CRLF line endings, backslash separators and drive roots do not change findings, ids, fingerprints or baselines",
  fixes: [
    {
      commit: "bbb3e0233",
      scope: "the JSON output of dupes in the CRLF form",
      reason:
        "the commit has one test binary per file and fewer output formats, and its CodeClimate clone fingerprints still have the bug that the oracle keeps as an ignored test",
      build: ["cargo", ["test", "-p", "fallow-cli", "--test", "platform_oracle", "--no-run"]],
      test: [
        "cargo",
        [
          "test",
          "-p",
          "fallow-cli",
          "--test",
          "platform_oracle",
          "platform::oracle::dupes_match_across_line_endings",
        ],
      ],
      env: { FALLOW_PLATFORM_FORMATS: "json" },
    },
    {
      commit: "aab072645",
      scope: "the grouped health rows",
      reason: "the commit does not have the baseline code of the other seam tests",
      test: integrationTest("platform::health_rows"),
    },
  ],
  port,
  build: integrationBuild,
  test: integrationTest("platform::"),
};
