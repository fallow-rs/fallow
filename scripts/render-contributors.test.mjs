import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { existsSync } from "node:fs";
import { mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";
import { assertLocalResolution } from "./assert-local-resolution.mjs";
import { generateContributors } from "./update-contributors.mjs";

assertLocalResolution({ dependency: "@resvg/resvg-js" });
const { Resvg } = await import("@resvg/resvg-js");
const script = fileURLToPath(new URL("./render-contributors.mjs", import.meta.url));
const GREEN_PIXEL =
  "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR4nGNk+M/wHwAEBgIAe2BNVAAAAABJRU5ErkJggg==";

test("the renderer CLI preserves embedded avatar pixels and SVG artwork in a standalone PNG", async (t) => {
  const directory = await mkdtemp(join(tmpdir(), "fallow contributor png % "));
  t.after(() => rm(directory, { recursive: true, force: true }));
  await writeFile(
    join(directory, "contributors.svg"),
    `<svg xmlns="http://www.w3.org/2000/svg" width="4" height="2"><image width="2" height="2" href="data:image/png;base64,${GREEN_PIXEL}"/><rect x="2" width="2" height="2" fill="#ff0000"/></svg>`,
  );
  const result = spawnSync(process.execPath, [script, directory], { encoding: "utf8" });
  assert.equal(result.status, 0, result.stderr);
  assert.equal(
    existsSync(join(directory, "contributors.png")),
    true,
    "the CLI must render a PNG gallery",
  );
  const png = await readFile(join(directory, "contributors.png"));
  const decoded = new Resvg(
    `<svg xmlns="http://www.w3.org/2000/svg" width="4" height="2"><image width="4" height="2" href="data:image/png;base64,${png.toString("base64")}"/></svg>`,
    { font: { loadSystemFonts: false } },
  ).render();
  assert.deepEqual(
    [...decoded.pixels.subarray(0, 4)],
    [0, 255, 0, 255],
    "the embedded avatar must retain its green pixel",
  );
  assert.deepEqual(
    [...decoded.pixels.subarray(12, 16)],
    [255, 0, 0, 255],
    "the SVG artwork must retain its red pixel",
  );
});

test("invalid SVG fails without replacing a previously rendered gallery", async (t) => {
  const directory = await mkdtemp(join(tmpdir(), "fallow invalid contributor png "));
  t.after(() => rm(directory, { recursive: true, force: true }));
  const previous = Buffer.from(GREEN_PIXEL, "base64");
  await writeFile(join(directory, "contributors.png"), previous);
  await writeFile(join(directory, "contributors.svg"), "invalid SVG");
  const result = spawnSync(process.execPath, [script, directory], { encoding: "utf8" });
  assert.equal(result.status, 1);
  assert.deepEqual(await readFile(join(directory, "contributors.png")), previous);
});

test("a GIF response header with PNG avatar bytes retains the person's pixels and credit", async (t) => {
  const directory = await mkdtemp(join(tmpdir(), "fallow avatar mime "));
  t.after(() => rm(directory, { recursive: true, force: true }));
  await generateContributors({
    repository: "owner/repo",
    outputDirectory: directory,
    fetchImpl: async (input) => {
      const url = new URL(input);
      if (url.origin === "https://avatars.githubusercontent.com") {
        return new Response(Buffer.from(GREEN_PIXEL, "base64"), {
          headers: { "content-type": "image/gif" },
        });
      }
      if (url.pathname.endsWith("/contributors"))
        return Response.json([
          {
            login: "gif-avatar",
            id: 1,
            type: "User",
            avatar_url: "https://avatars.githubusercontent.com/u/1",
          },
        ]);
      if (url.pathname.endsWith("/issues")) return Response.json([]);
      throw new Error("Unexpected request");
    },
  });
  const result = spawnSync(process.execPath, [script, directory], { encoding: "utf8" });
  assert.equal(result.status, 0, result.stderr);
  const png = await readFile(join(directory, "contributors.png"));
  const decoded = new Resvg(
    `<svg xmlns="http://www.w3.org/2000/svg" width="56" height="56"><image width="56" height="56" href="data:image/png;base64,${png.toString("base64")}"/></svg>`,
    { font: { loadSystemFonts: false } },
  ).render();
  const center = (24 * decoded.width + 24) * 4;
  assert.deepEqual(
    [...decoded.pixels.subarray(center, center + 4)],
    [0, 255, 0, 255],
    "the avatar must remain visible despite the incorrect response header",
  );
  assert.match(
    await readFile(join(directory, "CONTRIBUTORS.md"), "utf8"),
    /\[gif-avatar\]\(https:\/\/github\.com\/gif-avatar\)/,
  );
});
