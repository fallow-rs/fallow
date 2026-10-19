import assert from "node:assert/strict";
import { mkdirSync, mkdtempSync, readFileSync, readdirSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";

import {
  listFixtureFiles,
  pickFixtures,
  readSelectorPaths,
  selectorFor,
  writeSeeds,
} from "./fuzz-seed-extract.mjs";

const selectorPaths = readSelectorPaths(
  readFileSync("fuzz/fuzz_targets/common/extract_paths.rs", "utf8"),
);

test("reads every selector path from the Rust list", () => {
  assert.ok(selectorPaths.length > 10);
  assert.ok(selectorPaths.every((path) => path.includes(".")));
});

test("maps a fixture file to the selector with the same parser", () => {
  const nameFor = (file) => selectorPaths[selectorFor(file, selectorPaths)];
  assert.equal(nameFor("a/src/types.d.ts"), "src/index.d.ts");
  assert.equal(nameFor("a/src/app.component.ts"), "src/app.component.ts");
  assert.equal(nameFor("a/src/main.ts"), "src/index.ts");
  assert.equal(nameFor("a/App.vue"), "src/App.vue");
  assert.equal(nameFor("a/styles.module.css"), "src/styles.module.css");
  assert.equal(selectorFor("a/README.md", selectorPaths), -1);
  assert.equal(selectorFor("a/Makefile", selectorPaths), -1);
});

test("committed fixture seeds use a selector byte inside the list", () => {
  for (const target of ["fuzz_extract", "fuzz_extract_cache"]) {
    const dir = `fuzz/corpus/${target}`;
    for (const name of readdirSync(dir).filter((file) => file.startsWith("fixture_"))) {
      const selector = readFileSync(`${dir}/${name}`)[0];
      assert.ok(selector < selectorPaths.length, `${dir}/${name} has selector ${selector}`);
    }
  }
});

const fixtureTree = () => {
  const dir = mkdtempSync(join(tmpdir(), "fuzz-seed-"));
  const write = (file, content) => {
    mkdirSync(join(dir, file, ".."), { recursive: true });
    writeFileSync(join(dir, file), content);
  };
  write("a/src/main.ts", "export const a = 1;");
  write("a/src/b.ts", "export const b = 2;");
  write("a/App.vue", "<template><div /></template>");
  write("a/node_modules/pkg/index.ts", "export const skipped = 1;");
  write("a/big.ts", "x".repeat(4000));
  write("a/README.md", "# readme");
  return dir;
};

test("lists fixture files in path order without skipped directories", () => {
  const dir = fixtureTree();
  const files = listFixtureFiles(dir).map((file) =>
    file
      .slice(dir.length + 1)
      .split("\\")
      .join("/"),
  );
  assert.deepEqual(files, ["a/App.vue", "a/README.md", "a/big.ts", "a/src/b.ts", "a/src/main.ts"]);
});

test("picks small files with a known selector and writes one seed each", () => {
  const dir = fixtureTree();
  const picked = pickFixtures(dir, selectorPaths);
  const ts = selectorPaths.indexOf("src/index.ts");
  const vue = selectorPaths.indexOf("src/App.vue");
  assert.deepEqual([...picked.keys()].toSorted(), [ts, vue].toSorted());
  assert.equal(picked.get(ts).length, 2);

  const corpus = join(dir, "corpus");
  mkdirSync(corpus);
  writeFileSync(join(corpus, "fixture_stale.bin"), "old");
  writeFileSync(join(corpus, "hand.bin"), "kept");
  assert.equal(writeSeeds(corpus, picked), 3);
  const names = readdirSync(corpus);
  assert.ok(names.includes("hand.bin"));
  assert.ok(!names.includes("fixture_stale.bin"));
  const seeds = names.filter((name) => name.startsWith("fixture_"));
  assert.equal(seeds.length, 3);
  assert.ok(seeds.every((name) => [ts, vue].includes(readFileSync(join(corpus, name))[0])));
});
