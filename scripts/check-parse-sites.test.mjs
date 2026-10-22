import assert from "node:assert/strict";
import test from "node:test";

import { countParserCalls, findParseSiteViolations, main } from "./check-parse-sites.mjs";

const BOUNDED = `
fn parse(source: &str) -> Option<usize> {
    fallow_extract::parse_bounded(source, || {
        let allocator = Allocator::default();
        Parser::new(&allocator, source, SourceType::ts()).parse().program.body.len()
    })
}
`;

const TEST_ONLY = `
pub fn helper() {}

#[cfg(all(test, not(miri)))]
mod tests {
    fn parse(source: &str) {
        let _ = Parser::new(&allocator, source, SourceType::ts()).parse();
    }
}
`;

test("counts production calls and skips test modules and comments", () => {
  assert.equal(countParserCalls(BOUNDED), 1);
  assert.equal(countParserCalls(TEST_ONLY), 0);
  assert.equal(countParserCalls("// Parser::new(&allocator, source, kind)\n"), 0);
});

test("does not count a CSS parser that takes a mutable input", () => {
  assert.equal(countParserCalls("let mut parser = Parser::new(&mut input);\n"), 0);
});

test("a new call site must be listed", () => {
  const violations = findParseSiteViolations([["crates/x/src/a.rs", BOUNDED]], {});
  assert.equal(violations.length, 1);
  assert.match(violations[0], /not in scripts\/check-parse-sites\.mjs/);
});

test("a bounded entry must call parse_bounded", () => {
  const unbounded = "fn f() { Parser::new(&allocator, source, kind); }\n";
  const sites = { "crates/x/src/a.rs": { calls: 1, kind: "bounded" } };
  const violations = findParseSiteViolations([["crates/x/src/a.rs", unbounded]], sites);
  assert.deepEqual(violations, [
    "crates/x/src/a.rs: listed as bounded, but it does not call parse_bounded.",
  ]);
});

test("a count change and a stale entry are reported", () => {
  const sites = {
    "crates/x/src/a.rs": { calls: 2, kind: "bounded" },
    "crates/x/src/gone.rs": { calls: 1, kind: "nested" },
  };
  const violations = findParseSiteViolations([["crates/x/src/a.rs", BOUNDED]], sites);
  assert.equal(violations.length, 2);
});

test("the repository matches the list", () => {
  assert.equal(main(), 0);
});
