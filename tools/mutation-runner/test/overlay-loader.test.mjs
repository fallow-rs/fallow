import assert from "node:assert/strict";
import { execFileSync, spawnSync } from "node:child_process";
import { mkdtempSync, realpathSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import { pathToFileURL } from "node:url";

test("the overlay preserves a virtual file module supplied by a project loader", (t) => {
  const directory = mkdtempSync(join(tmpdir(), "fallow-overlay-loader-"));
  t.after(() => rmSync(directory, { recursive: true, force: true }));
  const source = join(directory, "original.mjs");
  const target = join(directory, "instrumented.mjs");
  writeFileSync(source, "export const value = 1;\n");
  writeFileSync(target, "export const value = 2;\n");
  const hook = new URL("../src/runtime/overlay-hook.mjs", import.meta.url).href;
  const virtual = new URL("virtual.mjs", pathToFileURL(`${directory}/`)).href;
  const code = `
    import { registerHooks } from "node:module";
    registerHooks({
      resolve(specifier, context, nextResolve) {
        return specifier === ${JSON.stringify(virtual)}
          ? { url: specifier, shortCircuit: true }
          : nextResolve(specifier, context);
      },
      load(url, context, nextLoad) {
        return url === ${JSON.stringify(virtual)}
          ? { format: "module", shortCircuit: true, source: "export const value = 7;" }
          : nextLoad(url, context);
      },
    });
    const { registerOverlayHook } = await import(${JSON.stringify(hook)});
    const overlay = new Map([[${JSON.stringify(source)}, { source: "original.mjs", target: ${JSON.stringify(target)} }]]);
    const loaded = [];
    registerOverlayHook(overlay, (entry) => loaded.push(entry.source), (message) => { throw new Error(message); });
    const supplied = await import(${JSON.stringify(virtual)});
    const planned = await import(${JSON.stringify(pathToFileURL(source).href)});
    console.log(JSON.stringify({ virtual: supplied.value, planned: planned.value, loaded }));
  `;
  const output = execFileSync(process.execPath, ["--input-type=module", "-e", code], {
    encoding: "utf8",
  });
  assert.deepEqual(JSON.parse(output), { virtual: 7, planned: 2, loaded: ["original.mjs"] });
});

// A project loader that compiles TypeScript itself returns format `module`.
// This loader removes the `: number` annotations, the only TypeScript syntax
// of the original files below.
const runBehindCompilingLoader = (directory, sourceText, targetText) => {
  // The overlay keys are real paths, as `buildOverlay` makes them.
  const source = join(realpathSync(directory), "original.ts");
  const target = join(realpathSync(directory), "instrumented.ts");
  writeFileSync(source, sourceText);
  writeFileSync(target, targetText);
  const hook = new URL("../src/runtime/overlay-hook.mjs", import.meta.url).href;
  const code = `
    import { registerHooks } from "node:module";
    registerHooks({
      load(url, context, nextLoad) {
        const result = nextLoad(url, context);
        if (!url.endsWith(".ts")) return result;
        const text = String(result.source).replaceAll(": number", "");
        return { format: "module", shortCircuit: true, source: text };
      },
    });
    const { registerOverlayHook } = await import(${JSON.stringify(hook)});
    const overlay = new Map([[${JSON.stringify(source)}, { source: "original.ts", target: ${JSON.stringify(target)} }]]);
    const loaded = [];
    const incomplete = [];
    registerOverlayHook(overlay, (entry) => loaded.push(entry.source), (message) => incomplete.push(message));
    const planned = await import(${JSON.stringify(pathToFileURL(source).href)});
    console.log(JSON.stringify({ planned: planned.value, loaded, incomplete }));
  `;
  const child = spawnSync(process.execPath, ["--input-type=module", "-e", code], {
    encoding: "utf8",
  });
  assert.equal(child.status, 0, child.stderr);
  assert.equal(child.stderr, "");
  return JSON.parse(child.stdout);
};

test("the overlay serves a TypeScript mutant behind a loader that compiles TypeScript", (t) => {
  const directory = mkdtempSync(join(tmpdir(), "fallow-overlay-ts-"));
  t.after(() => rmSync(directory, { recursive: true, force: true }));
  const result = runBehindCompilingLoader(
    directory,
    "export const value: number = 1;\n",
    "export const value: number = 2;\n",
  );
  assert.deepEqual(result, { planned: 2, loaded: ["original.ts"], incomplete: [] });
});

test("the overlay reports a TypeScript mutant that type stripping cannot serve", (t) => {
  const directory = mkdtempSync(join(tmpdir(), "fallow-overlay-ts-enum-"));
  t.after(() => rmSync(directory, { recursive: true, force: true }));
  const result = runBehindCompilingLoader(
    directory,
    "export const value: number = 1;\n",
    "enum Kind { A = 2 }\nexport const value: number = Kind.A;\n",
  );
  assert.equal(result.planned, 1);
  assert.deepEqual(result.loaded, []);
  assert.equal(result.incomplete.length, 1);
  assert.match(result.incomplete[0], /original\.ts/);
  assert.match(result.incomplete[0], /enum/);
});
