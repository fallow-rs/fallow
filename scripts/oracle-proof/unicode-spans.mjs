// The Unicode span oracle of the LSP remove-export quick fix and the
// infrastructure file parsers, ported onto commits from before it existed.

import { cpSync, existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { basename, dirname, join } from "node:path";

const LSP_PARENT = "crates/lsp/src/code_actions/mod.rs";
const LSP_ORACLE = "crates/lsp/src/code_actions/quick_fix_unicode_span_oracle.rs";
const LSP_ANCHOR = "mod quick_fix;\n";
const LSP_MANIFEST = "crates/lsp/Cargo.toml";
const RESULTS = "crates/types/src/results.rs";
const CORE_PARENT = "crates/core/src/discover/infrastructure.rs";
const CORE_ORACLE = "crates/core/src/discover/infrastructure_unicode_span_oracle.rs";
const CORE_ANCHOR = "#[cfg(test)]\nmod tests {";
/** The `#[cfg(test)]` module line for the oracle file next to `path`. */
const moduleLine = (path) =>
  `#[cfg(test)]\n#[path = "${basename(path)}"]\nmod unicode_span_oracle;\n\n`;
/** The parsers took no `CommandRefContext` before the `ignoreCommandEntries` option. */
const CONTEXT_FREE_PARSER = "fn extract_dockerfile_file_refs(content: &str) -> Vec<String>";

const editFile = (path, edit) => writeFileSync(path, edit(readFileSync(path, "utf8")));

const copyFrom = (tree, source, path) => {
  mkdirSync(dirname(join(tree, path)), { recursive: true });
  cpSync(join(source, path), join(tree, path));
};

const registerModule = (tree, path, oracle, anchor) =>
  editFile(join(tree, path), (text) => {
    if (/^mod unicode_span_oracle;$/m.test(text)) return text;
    if (!text.includes(anchor)) {
      throw new Error(`${path}: the anchor ${JSON.stringify(anchor)} is missing`);
    }
    return text.replace(anchor, `${moduleLine(oracle)}${anchor}`);
  });

/**
 * Install the LSP and core oracle modules in `tree`, and adapt them to the
 * APIs of the commit. Returns a note per change.
 */
const port = (tree, source) => {
  const notes = [`copied ${LSP_ORACLE} and ${CORE_ORACLE} from the source checkout`];
  copyFrom(tree, source, LSP_ORACLE);
  copyFrom(tree, source, CORE_ORACLE);
  registerModule(tree, LSP_PARENT, LSP_ORACLE, LSP_ANCHOR);
  registerModule(tree, CORE_PARENT, CORE_ORACLE, CORE_ANCHOR);

  const manifest = join(tree, LSP_MANIFEST);
  if (!/^proptest = /m.test(readFileSync(manifest, "utf8"))) {
    editFile(manifest, (text) =>
      text.replace("[dev-dependencies]\n", "[dev-dependencies]\nproptest = { workspace = true }\n"),
    );
    notes.push("added proptest to the LSP dev-dependencies");
  }

  const results = readFileSync(join(tree, RESULTS), "utf8");
  const start = results.indexOf("pub struct UnusedExport {");
  if (start < 0) throw new Error(`${RESULTS}: the UnusedExport struct is missing`);
  const unusedExport = results.slice(start);
  if (!unusedExport.slice(0, unusedExport.indexOf("\n}")).includes("pub deprecated:")) {
    editFile(join(tree, LSP_ORACLE), (text) =>
      text.replace(/^ *deprecated(_reason)?: [^\n]*,\n/gm, ""),
    );
    notes.push("built UnusedExport without the deprecated fields");
  }

  if (readFileSync(join(tree, CORE_PARENT), "utf8").includes(CONTEXT_FREE_PARSER)) {
    editFile(join(tree, CORE_ORACLE), (text) =>
      text.replaceAll("(content, CommandRefContext::NONE)", "(content)"),
    );
    notes.push("called the infrastructure parsers without a CommandRefContext");
  }
  if (!existsSync(join(tree, CORE_ORACLE))) throw new Error(`${CORE_ORACLE} was not copied`);
  return notes;
};

// No target filter: `fallow-lsp` was a binary crate without a library
// target before it gained `src/lib.rs`.
const LSP_TEST = ["test", "-p", "fallow-lsp", "code_actions::unicode_span_oracle"];
const CORE_TEST = ["test", "-p", "fallow-core", "--lib", "infrastructure::unicode_span_oracle"];

/**
 * The Unicode span oracle: each edit and each span stays on a character
 * boundary over text with astral characters, combining marks and Unicode
 * whitespace, and an applied edit changes only its target.
 */
export const unicodeSpans = {
  name: "unicode-spans",
  description: "byte and UTF-16 offsets over Unicode text for LSP edits and infrastructure parsers",
  fixes: [
    {
      commit: "618f1ddd7",
      scope: "the LSP remove-export quick fix",
      reason: "the fix is in the LSP, so the infrastructure parser tests do not apply",
      test: ["cargo", LSP_TEST],
    },
    {
      commit: "139dc1662",
      scope: "the infrastructure file parsers",
      reason: "the fix is in the Dockerfile parser, so the LSP tests do not apply",
      test: ["cargo", CORE_TEST],
    },
  ],
  port,
  backports: [],
  build: ["cargo", ["test", "-p", "fallow-lsp", "-p", "fallow-core", "--no-run"]],
  test: ["cargo", ["test", "-p", "fallow-lsp", "-p", "fallow-core", "unicode_span_oracle"]],
};
