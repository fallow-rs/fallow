import assert from "node:assert/strict";
import test from "node:test";

import {
  findMiriCfgViolations,
  main,
  miriPackages,
  modulePathFor,
  stripLiterals,
} from "./check-miri-cfg.mjs";

const LIB_WITH_GATED_TESTS = `
pub fn parse() {}

#[cfg(all(test, not(miri)))]
mod tests;
`;

const TESTS_HELPER = `
pub(crate) fn parse_ts(source: &str) {}
`;

const crateWith = (files) => [
  { path: "crates/extract/src/lib.rs", modulePath: [], source: LIB_WITH_GATED_TESTS },
  { path: "crates/extract/src/tests.rs", modulePath: ["tests"], source: TESTS_HELPER },
  ...files,
];

const siblingModule = (cfg) => ({
  path: "crates/extract/src/og_image.rs",
  modulePath: ["og_image"],
  source: `
pub fn names() {}

${cfg}
mod tests {
    use super::*;

    #[test]
    fn parses() {
        let info = crate::tests::parse_ts(
            "const x = { a: 1 };",
        );
    }
}
`,
});

test("rejects a cfg(test) module that calls a not(miri) helper", () => {
  const violations = findMiriCfgViolations(crateWith([siblingModule("#[cfg(test)]")]));

  assert.equal(violations.length, 1);
  assert.equal(violations[0].path, "crates/extract/src/og_image.rs");
  assert.equal(violations[0].line, 10);
  assert.equal(violations[0].module, "tests");
});

test("accepts the same module when it is gated with not(miri)", () => {
  assert.deepEqual(
    findMiriCfgViolations(crateWith([siblingModule("#[cfg(all(test, not(miri)))]")])),
    [],
  );
});

test("accepts a multi-line not(miri) attribute", () => {
  const cfg = "#[cfg(all(\n    test,\n    not(miri),\n))]";
  assert.deepEqual(findMiriCfgViolations(crateWith([siblingModule(cfg)])), []);
});

test("accepts code inside the gated module and inside not(miri) items", () => {
  const files = crateWith([
    {
      path: "crates/extract/src/tests/more.rs",
      modulePath: ["tests", "more"],
      source: 'fn helper() { crate::tests::parse_ts(""); }\n',
    },
    {
      path: "crates/extract/src/css.rs",
      modulePath: ["css"],
      source: `
#[cfg(test)]
mod tests {
    #[cfg(not(miri))]
    fn slow() {
        crate::tests::parse_ts("");
    }

    #[cfg(not(miri))]
    use crate::tests::parse_ts;
}
`,
    },
    {
      path: "crates/extract/src/visitor/tests.rs",
      modulePath: ["visitor", "tests"],
      source: "#![cfg(all(test, not(miri)))]\nuse crate::tests::parse_ts;\n",
    },
  ]);

  assert.deepEqual(findMiriCfgViolations(files), []);
});

test("resolves super paths into a gated sibling module", () => {
  const files = [
    {
      path: "crates/graph/src/resolve/mod.rs",
      modulePath: ["resolve"],
      source: `
#[cfg(all(test, not(miri)))]
mod fixtures {
    pub fn ctx() {}
}

#[cfg(test)]
mod tests {
    fn uses() {
        super::fixtures::ctx();
    }
}
`,
    },
  ];

  const violations = findMiriCfgViolations(files);

  assert.equal(violations.length, 1);
  assert.equal(violations[0].module, "resolve::fixtures");
});

test("ignores paths inside strings, raw strings and comments", () => {
  const files = crateWith([
    {
      path: "crates/extract/src/sfc.rs",
      modulePath: ["sfc"],
      source: `
#[cfg(test)]
mod tests {
    // crate::tests::parse_ts is not used here.
    const SOURCE: &str = r#"crate::tests::parse_ts("}")"#;
    const OTHER: &str = "crate::tests::parse_ts";
    const BRACE: char = '}';
}
`,
    },
  ]);

  assert.deepEqual(findMiriCfgViolations(files), []);
});

test("stripLiterals keeps line breaks and lifetimes", () => {
  const stripped = stripLiterals("fn a<'a>(x: &'a str) {\n    \"{\"\n}\n");

  assert.equal(stripped.split("\n").length, 4);
  assert.match(stripped, /<'a>/);
  assert.doesNotMatch(stripped, /"\{"/);
});

test("modulePathFor maps crate files to module paths", () => {
  assert.deepEqual(modulePathFor("lib.rs"), []);
  assert.deepEqual(modulePathFor("cache/mod.rs"), ["cache"]);
  assert.deepEqual(modulePathFor("visitor/tests.rs"), ["visitor", "tests"]);
});

test("miriPackages reads the crates from the Miri job", () => {
  const workflow = `
      - run: cargo +"$T" miri test -p fallow-types --lib
      - run: |
          cargo +"$T" miri test -p fallow-extract --lib css::
          cargo +"$T" miri test -p fallow-extract --lib suppress::
`;

  assert.deepEqual(miriPackages(workflow), ["fallow-types", "fallow-extract"]);
});

test("the repository passes the check", () => {
  const original = console.log;
  console.log = () => {};
  try {
    assert.equal(main([]), 0);
  } finally {
    console.log = original;
  }
});
