// The permutation oracle of crates/cli/tests/integration/permutation, ported
// onto commits from before it existed.

import { cpSync, existsSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";

const TESTS = "crates/cli/tests/integration/permutation";
const HOOK = "crates/config/src/permutation.rs";
const WALK = "crates/core/src/discover/walk.rs";
const WORKSPACES = "crates/config/src/workspace/mod.rs";

const editFile = (tree, path, edit) => {
  const file = join(tree, path);
  writeFileSync(file, edit(readFileSync(file, "utf8")));
};

/** Replace the first `anchor` with `replacement`, or fail when it is missing. */
const replaceOnce = (path, anchor, replacement) => (text) => {
  if (!text.includes(anchor)) {
    throw new Error(`${path}: the anchor ${JSON.stringify(anchor)} is missing`);
  }
  return text.replace(anchor, () => replacement);
};

/**
 * The hook calls of the current source, as `[path, anchor, replacement]`.
 * `String.replace` with a string changes the first match only, so the test
 * helpers at the end of `walk.rs` keep their own sort.
 */
const HOOK_CALLS = [
  [
    WALK,
    "use fallow_types::discover::{DiscoveredFile, FileId};\n",
    "use fallow_config::{PermutationSite, sort_permuted};\nuse fallow_types::discover::{DiscoveredFile, FileId};\n",
  ],
  [
    WALK,
    "    raw.sort_unstable_by(|a, b| a.0.cmp(&b.0));\n",
    "    sort_permuted(&mut raw, PermutationSite::Files, |a, b| a.0.cmp(&b.0));\n",
  ],
  [
    WALK,
    "    config_candidates.sort_unstable();\n",
    "    sort_permuted(&mut config_candidates, PermutationSite::Configs, Ord::cmp);\n",
  ],
  [
    WALK,
    "    dotdir_candidates.sort_unstable();\n",
    "    sort_permuted(&mut dotdir_candidates, PermutationSite::Dotdirs, Ord::cmp);\n",
  ],
  [
    WORKSPACES,
    "    if !workspaces.is_empty() {\n        mark_internal_dependencies(&mut workspaces);\n",
    "    crate::permutation::permute(&mut workspaces, crate::permutation::PermutationSite::Workspaces);\n    if !workspaces.is_empty() {\n        mark_internal_dependencies(&mut workspaces);\n",
  ],
  [
    "crates/config/src/lib.rs",
    "mod package_manager;\n",
    "mod package_manager;\npub mod permutation;\npub use permutation::{PermutationSite, sort_permuted};\n",
  ],
  [
    "crates/config/Cargo.toml",
    "[dev-dependencies]\n",
    "[features]\npermutation-hook = []\n\n[dev-dependencies]\n",
  ],
];

const CLI_DEV_DEPENDENCY =
  'fallow-config = { workspace = true, features = ["permutation-hook"] }\n';

/** Add the `permutation-hook` feature and its calls to a commit from before it. */
const portHook = (tree, source) => {
  cpSync(join(source, HOOK), join(tree, HOOK));
  for (const [path, anchor, replacement] of HOOK_CALLS) {
    editFile(tree, path, replaceOnce(path, anchor, replacement));
  }
  editFile(tree, "crates/cli/Cargo.toml", (text) =>
    text.replace("[dev-dependencies]\n", `[dev-dependencies]\n${CLI_DEV_DEPENDENCY}`),
  );
};

const registerTestModule = (tree) => {
  editFile(tree, "crates/cli/tests/integration/main.rs", (text) => {
    if (/^mod permutation;$/m.test(text)) return text;
    const anchor = "\nmod absent_component_prop_tests;\n";
    if (!text.includes(anchor)) {
      throw new Error("crates/cli/tests/integration/main.rs: the first test module is missing");
    }
    return text.replace(anchor, () => `\nmod permutation;${anchor}`);
  });
};

/**
 * Install the current permutation oracle in `tree`, and add the hook when the
 * commit does not have it. Returns a note per change.
 */
const port = (tree, source) => {
  const notes = [`copied ${TESTS} from the source checkout`];
  cpSync(join(source, TESTS), join(tree, TESTS), { recursive: true });
  registerTestModule(tree);
  if (!existsSync(join(tree, HOOK))) {
    portHook(tree, source);
    notes.push("added the permutation-hook feature and its discovery calls");
  }
  return notes;
};

const CARGO_TEST = ["test", "-p", "fallow-cli", "--test", "integration", "permutation::oracle"];

/**
 * The permutation oracle: a seeded permutation of the source walk and of the
 * workspace list does not change the output of any command or format.
 */
export const permutation = {
  name: "permutation",
  description: "the discovery and workspace order does not change any output format",
  // Before 778aacf3e (#4287), the first workspace in the list gave its
  // `$lib` or `~/` alias to the files of each app, so the workspace order
  // decided which file an import resolved to.
  fixes: [{ commit: "778aacf3e", scope: "every permutation oracle test" }],
  port,
  backports: [],
  build: ["cargo", [...CARGO_TEST, "--no-run"]],
  test: ["cargo", CARGO_TEST],
};
