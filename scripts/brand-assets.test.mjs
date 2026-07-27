import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";

const EXPECTED_VIEW_BOX = "139.5 63.86 1075.72 1075.72";
const PNG_SIGNATURE = "89504e470d0a1a0a";

const assertRgbaPng = (path) => {
  const png = readFileSync(path);
  assert.equal(png.subarray(0, 8).toString("hex"), PNG_SIGNATURE);
  assert.equal(png.readUInt32BE(16), 1024);
  assert.equal(png.readUInt32BE(20), 1024);
  assert.equal(png[24], 8);
  assert.equal(png[25], 6);
  return png;
};

test("standalone brand icons reserve a twelve-and-a-half percent transparent safe area", () => {
  const svg = readFileSync("assets/icon.svg", "utf8");
  assert.match(svg, new RegExp(`viewBox="${EXPECTED_VIEW_BOX.replaceAll(".", "\\.")}"`, "u"));

  const canonical = assertRgbaPng("assets/icon.png");
  const vscode = assertRgbaPng("editors/vscode/icon.png");
  assert.deepEqual(vscode, canonical);
});

test("the report favicon uses the standalone safe area without shrinking the in-product mark", () => {
  const chrome = readFileSync("viz-frontend/src/chrome.ts", "utf8");
  assert.match(
    chrome,
    new RegExp(`FAVICON_VIEW_BOX = "${EXPECTED_VIEW_BOX.replaceAll(".", "\\.")}"`, "u"),
  );
  assert.match(chrome, /MARK_VIEW_BOX = "173\.11 97\.47 1008\.49 1008\.49"/u);
});
