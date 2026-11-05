// The cache oracle of crates/engine/tests/integration/cache_model, ported
// onto commits from before it existed.

import { cpSync, existsSync, readdirSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";

const TESTS = "crates/engine/tests/integration/cache_model";
const READ_TRACE = "crates/graph/src/read_trace.rs";
const RESOLVE_DIR = "crates/graph/src/resolve";
const RESOLUTION_INPUTS = "crates/core/src/resolution_inputs.rs";
const TEST_MODULE = "#[cfg(test)]\nmod tests {";

/** The substitutions that route the resolver file system calls through the read trace. */
const TRACE_CALLS = [
  [/\bstd::fs::read_to_string\(/g, "crate::read_trace::read_to_string("],
  [/(?<![\w:])fs::read_to_string\(/g, "crate::read_trace::read_to_string("],
  [/dunce::canonicalize\(/g, "crate::read_trace::canonicalize("],
  [/\.is_file\(\)/g, ".traced_is_file()"],
  [/\.is_dir\(\)/g, ".traced_is_dir()"],
  [/\.exists\(\)/g, ".traced_exists()"],
  [/oxc_resolver::Resolver::new\(/g, "crate::read_trace::Resolver::new("],
  [/: oxc_resolver::Resolver,/g, ": crate::read_trace::Resolver,"],
];

const editFile = (path, edit) => writeFileSync(path, edit(readFileSync(path, "utf8")));

const insertOnce = (text, anchor, insertion, path) => {
  if (!text.includes(anchor)) {
    throw new Error(`${path}: the anchor ${JSON.stringify(anchor)} is missing`);
  }
  return text.replace(anchor, insertion);
};

/** Move `Resolver` out of a `use oxc_resolver::{...}` list into a read-trace import. */
const traceResolverImport = (text) =>
  text
    .replace(/use oxc_resolver::\{([^}]*)\};/g, (statement, list) => {
      const names = list
        .split(",")
        .map((name) => name.trim())
        .filter(Boolean);
      if (!names.includes("Resolver")) return statement;
      const rest = names.filter((name) => name !== "Resolver");
      const kept = rest.length > 0 ? `use oxc_resolver::{${rest.join(", ")}};\n` : "";
      return `${kept}use crate::read_trace::Resolver;`;
    })
    .replace("use oxc_resolver::Resolver;", "use crate::read_trace::Resolver;");

/** Rewrite the non-test part of one resolver source file. */
export const traceResolverSource = (text) => {
  const cut = text.indexOf(TEST_MODULE);
  const head = cut < 0 ? text : text.slice(0, cut);
  const tail = cut < 0 ? "" : text.slice(cut);
  let traced = TRACE_CALLS.reduce(
    (current, [pattern, repl]) => current.replace(pattern, repl),
    head,
  );
  traced = traceResolverImport(traced);
  if (traced.includes("traced_") && !traced.includes("use crate::read_trace::TracedPath;")) {
    traced = traced.replace(/^(use [^\n]*;\n)/m, "$1use crate::read_trace::TracedPath;\n");
  }
  return traced + tail;
};

/** Add the `read-trace` feature and its hooks to a commit from before #4237. */
const portReadTrace = (tree, source) => {
  cpSync(join(source, READ_TRACE), join(tree, READ_TRACE));
  const resolveDir = join(tree, RESOLVE_DIR);
  for (const name of readdirSync(resolveDir)) {
    if (!name.endsWith(".rs") || name.endsWith("tests.rs")) continue;
    editFile(join(resolveDir, name), traceResolverSource);
  }
  editFile(join(tree, "crates/graph/src/lib.rs"), (text) =>
    insertOnce(text, "pub mod project;\n", "pub mod project;\npub mod read_trace;\n", "lib.rs"),
  );
  editFile(join(tree, "crates/graph/Cargo.toml"), (text) =>
    insertOnce(
      text,
      "[dev-dependencies]",
      "[features]\nread-trace = []\n\n[dev-dependencies]",
      "crates/graph/Cargo.toml",
    ),
  );
  editFile(join(tree, "crates/engine/Cargo.toml"), (text) =>
    insertOnce(
      text,
      "[dev-dependencies]\n",
      '[dev-dependencies]\nfallow-graph = { workspace = true, features = ["read-trace"] }\n',
      "crates/engine/Cargo.toml",
    ),
  );
};

const registerTestModule = (tree) => {
  editFile(join(tree, "crates/engine/tests/integration/main.rs"), (text) => {
    if (/^mod cache_model;$/m.test(text)) return text;
    const first = text.search(/^mod /m);
    if (first < 0) throw new Error("crates/engine/tests/integration/main.rs has no mod line");
    return `${text.slice(0, first)}mod cache_model;\n${text.slice(first)}`;
  });
};

/**
 * Install the current cache model tests in `tree`, and add the read-trace
 * hooks when the commit does not have them. Returns a note per change.
 */
const port = (tree, source) => {
  const notes = [`copied ${TESTS} from the source checkout`];
  cpSync(join(source, TESTS), join(tree, TESTS), { recursive: true });
  registerTestModule(tree);
  if (!existsSync(join(tree, READ_TRACE))) {
    portReadTrace(tree, source);
    notes.push("added the read-trace feature and its resolver hooks");
  }
  return notes;
};

const PNP_ANCHOR = '    "node_modules/.yarn-integrity",\n];';

/**
 * `.pnp.cjs` and `.pnp.data.json` came into the graph cache key in #4237.
 * The read-set oracle finds that gap on each older commit, so without this
 * backport no older fix commit passes.
 */
const pnpCacheKey = {
  name: "Plug'n'Play files in the graph cache key (#4237)",
  applies: (tree) => {
    const path = join(tree, RESOLUTION_INPUTS);
    return existsSync(path) && readFileSync(path, "utf8").includes(PNP_ANCHOR);
  },
  apply: (tree) =>
    editFile(join(tree, RESOLUTION_INPUTS), (text) =>
      text.replace(
        PNP_ANCHOR,
        '    "node_modules/.yarn-integrity",\n    ".pnp.cjs",\n    ".pnp.data.json",\n];',
      ),
    ),
};

const CARGO_TEST = ["test", "-p", "fallow-engine", "--test", "integration", "cache_model"];
const REGRESSION_TEST = [
  ...CARGO_TEST.slice(0, -1),
  "cache_model::sequences::regression_sequences_match_cache_free_runs",
  "--",
  "--exact",
];

/**
 * The cache oracle: the graph cache key covers each path that import
 * resolution reads, and a warm run equals a cache-free run after each step of
 * an edit sequence.
 */
export const cacheModel = {
  name: "cache-model",
  description: "read-set oracle and warm equals cold for the graph, extract and dupes caches",
  fixes: [
    { commit: "fcabf4647", scope: "every cache model test" },
    {
      commit: "7ce45bd27",
      scope: "the #3477 regression sequences",
      reason:
        "the commit does not have the later install-state and Plug'n'Play keys, so the other tests fail on it",
      test: ["cargo", REGRESSION_TEST],
      env: { FALLOW_CACHE_MODEL_REGRESSIONS: "(#3477)" },
    },
  ],
  port,
  backports: [pnpCacheKey],
  build: ["cargo", [...CARGO_TEST, "--no-run"]],
  test: ["cargo", CARGO_TEST],
};
