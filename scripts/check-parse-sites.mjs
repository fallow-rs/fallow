#!/usr/bin/env node
// Every Oxc parse of user source must run inside `fallow_extract::parse_bounded`.
// That function sizes the stack from the source, so deep nesting cannot
// overflow it and abort the process. This check counts the parser calls in
// production code and compares them with the list below, so a new call site
// needs a decision.
import { readdirSync, readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

import { stripLiterals } from "./check-miri-cfg.mjs";
import { runCliMain } from "./cli-main.mjs";

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
// An Oxc parser call takes an allocator first. The CSS parser in css_metrics.rs
// takes `&mut ParserInput`.
const PARSER_CALL = /\bParser::new\(\s*(?:&(?!mut\b)|[a-z_][a-z0-9_]*\s*,)/g;
const TEST_MODULE_ATTRIBUTE = /^\s*#\[cfg\((?:all\()?test\b/;
const MOD_LINE = /^\s*(?:pub(?:\([^)]*\))?\s+)?mod\s+\w+/;
const BOUNDED_CALL = /\bparse_bounded\(/;

/**
 * Files with Oxc parser calls in production code. A file that parses through
 * `parse_bounded_or_default` has no call of its own. `bounded`: the file wraps
 * each call in `parse_bounded`. `nested`: the call runs only inside the
 * bounded parse of a whole file, so it runs on the sized stack.
 */
export const PARSE_SITES = {
  "crates/core/src/plugins/config_parser.rs": { calls: 1, kind: "bounded" },
  "crates/engine/src/duplication_detector/tokenize/mod.rs": { calls: 4, kind: "bounded" },
  "crates/engine/src/search_visibility.rs": { calls: 2, kind: "bounded" },
  "crates/extract/src/alchemy.rs": { calls: 1, kind: "nested" },
  "crates/extract/src/astro.rs": { calls: 3, kind: "nested" },
  "crates/extract/src/css_in_js/object.rs": { calls: 1, kind: "bounded" },
  "crates/extract/src/federation_runtime.rs": { calls: 1, kind: "nested" },
  "crates/extract/src/inventory.rs": { calls: 1, kind: "bounded" },
  "crates/extract/src/mdx.rs": { calls: 1, kind: "nested" },
  "crates/extract/src/nesting.rs": { calls: 1, kind: "bounded" },
  "crates/extract/src/parse.rs": { calls: 2, kind: "nested" },
  "crates/extract/src/sfc.rs": { calls: 2, kind: "nested" },
  "crates/extract/src/sfc_template/component_contracts.rs": { calls: 1, kind: "nested" },
  "crates/extract/src/similar_code.rs": { calls: 1, kind: "bounded" },
  "crates/extract/src/template_usage.rs": { calls: 3, kind: "nested" },
  // fallow-mutation has no fallow dependencies. The engine calls it only
  // inside parse_bounded (crates/engine/src/mutation/layout.rs).
  "crates/mutation/src/lib.rs": { calls: 1, kind: "nested" },
};

/**
 * Production source of a Rust file: comments and literals blanked, and the
 * code from the first `#[cfg(test)]` module on removed.
 */
export const productionSource = (source) => {
  const lines = stripLiterals(source).split("\n");
  const testStart = lines.findIndex(
    (line, index) => TEST_MODULE_ATTRIBUTE.test(line) && MOD_LINE.test(lines[index + 1] ?? ""),
  );
  return (testStart === -1 ? lines : lines.slice(0, testStart)).join("\n");
};

/** Number of Oxc parser calls in the production part of `source`. */
export const countParserCalls = (source) =>
  [...productionSource(source).matchAll(PARSER_CALL)].length;

const isTestFile = (relative) =>
  relative.split("/").includes("tests") || /(?:^|_)tests?\.rs$/.test(path.basename(relative));

const rustFiles = (root) => {
  const files = [];
  const walk = (directory) => {
    for (const entry of readdirSync(directory, { withFileTypes: true })) {
      const full = path.join(directory, entry.name);
      if (entry.isDirectory()) {
        walk(full);
      } else if (entry.name.endsWith(".rs")) {
        files.push(path.relative(root, full).split(path.sep).join("/"));
      }
    }
  };
  for (const crate of readdirSync(path.join(root, "crates"), { withFileTypes: true })) {
    const src = path.join(root, "crates", crate.name, "src");
    if (crate.isDirectory()) {
      try {
        walk(src);
      } catch {
        // A crate without a src directory has no Rust sources to check.
      }
    }
  }
  return files.filter((file) => !isTestFile(file)).toSorted();
};

/**
 * Compares the parser calls in `files` (path to source) with `sites` and
 * returns one message per difference.
 */
export const findParseSiteViolations = (files, sites = PARSE_SITES) => {
  const violations = [];
  const seen = new Set();
  for (const [file, source] of files) {
    const calls = countParserCalls(source);
    if (calls === 0) continue;
    seen.add(file);
    const site = sites[file];
    if (!site) {
      violations.push(
        `${file}: ${calls} Oxc parser call(s) not in scripts/check-parse-sites.mjs. ` +
          "Run the parse inside fallow_extract::parse_bounded, then add the file.",
      );
      continue;
    }
    if (site.calls !== calls) {
      violations.push(`${file}: ${calls} Oxc parser call(s), the list has ${site.calls}.`);
    }
    if (site.kind === "bounded" && !BOUNDED_CALL.test(productionSource(source))) {
      violations.push(`${file}: listed as bounded, but it does not call parse_bounded.`);
    }
  }
  for (const file of Object.keys(sites)) {
    if (!seen.has(file)) {
      violations.push(`${file}: listed, but it has no Oxc parser call. Remove the entry.`);
    }
  }
  return violations;
};

export const main = (root = ROOT) => {
  const files = rustFiles(root).map((file) => [file, readFileSync(path.join(root, file), "utf8")]);
  const violations = findParseSiteViolations(files);
  if (violations.length === 0) {
    console.log("parse site check passed");
    return 0;
  }
  for (const violation of violations) {
    console.error(`parse-sites: ${violation}`);
  }
  return 1;
};

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  runCliMain(main);
}
