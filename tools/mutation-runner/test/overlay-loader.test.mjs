import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
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
