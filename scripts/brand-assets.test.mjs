import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";

const EXPECTED_STANDALONE_VIEW_BOX = "139.5 63.86 1075.72 1075.72";
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
  assert.match(
    svg,
    new RegExp(`viewBox="${EXPECTED_STANDALONE_VIEW_BOX.replaceAll(".", "\\.")}"`, "u"),
  );

  const canonical = assertRgbaPng("assets/icon.png");
  const vscode = assertRgbaPng("editors/vscode/icon.png");
  assert.deepEqual(vscode, canonical);
});

const VIZ_BUNDLE = "crates/cli/viz-assets/viz.js";
/** Share of the favicon side the mark must span, so the tab icon reads edge to edge. */
const MIN_FAVICON_FILL = 0.8;
const CENTER_TOLERANCE = 1;

const parseViewBox = (value) => {
  const [x, y, width, height] = value.split(" ").map(Number);
  return { x, y, width, height };
};

/** Bounds of the inlined mark path after its `translate(0,1254) scale(0.1,-0.1)` transform. */
const markBounds = (path) => {
  const tokens = path.match(/[MmCcLlZz]|-?\d+(?:\.\d+)?/gu);
  const points = [];
  let command = "";
  let x = 0;
  let y = 0;
  for (let index = 0; index < tokens.length;) {
    const token = tokens[index];
    if (/[A-Za-z]/u.test(token)) {
      command = token;
      index += 1;
      continue;
    }
    const n = (offset) => Number(tokens[index + offset]);
    if (command === "M" || command === "L") {
      [x, y] = [n(0), n(1)];
      index += 2;
      command = "L";
    } else if (command === "m" || command === "l") {
      [x, y] = [x + n(0), y + n(1)];
      index += 2;
      command = "l";
    } else if (command === "c") {
      points.push([x + n(0), y + n(1)], [x + n(2), y + n(3)]);
      [x, y] = [x + n(4), y + n(5)];
      index += 6;
    } else {
      throw new Error(`unsupported path command ${command}`);
    }
    points.push([x, y]);
  }
  const xs = points.map(([px]) => px * 0.1);
  const ys = points.map(([, py]) => 1254 - py * 0.1);
  return {
    minX: Math.min(...xs),
    maxX: Math.max(...xs),
    minY: Math.min(...ys),
    maxY: Math.max(...ys),
  };
};

const assertCenteredSquareAround = (box, bounds, label) => {
  assert.equal(box.width, box.height, `${label} viewBox is square`);
  assert.ok(box.x <= bounds.minX && box.x + box.width >= bounds.maxX, `${label} contains the mark`);
  assert.ok(
    box.y <= bounds.minY && box.y + box.height >= bounds.maxY,
    `${label} contains the mark`,
  );
  const centerX = (bounds.minX + bounds.maxX) / 2;
  const centerY = (bounds.minY + bounds.maxY) / 2;
  assert.ok(
    Math.abs(box.x + box.width / 2 - centerX) <= CENTER_TOLERANCE,
    `${label} centered on x`,
  );
  assert.ok(
    Math.abs(box.y + box.height / 2 - centerY) <= CENTER_TOLERANCE,
    `${label} centered on y`,
  );
};

test("the report favicon fills its canvas without shrinking the in-product mark", () => {
  const bundle = readFileSync(VIZ_BUNDLE, "utf8");
  const favicon = bundle.match(/viewBox="([^"]+)"><style>path\{fill/u)?.[1];
  const mark = bundle.match(/`mark`\),\w+\.setAttribute\(`viewBox`,`([^`]+)`\)/u)?.[1];
  const path = bundle.match(/M9990 9649[^`"']*/u)?.[0];
  assert.ok(favicon, "shipped bundle sets a favicon viewBox");
  assert.ok(mark, "shipped bundle sets the in-product mark viewBox");
  assert.ok(path, "shipped bundle inlines the mark path");

  const bounds = markBounds(path);
  const faviconBox = parseViewBox(favicon);
  const markBox = parseViewBox(mark);
  assertCenteredSquareAround(faviconBox, bounds, "favicon");
  assertCenteredSquareAround(markBox, bounds, "mark");

  const markWidth = bounds.maxX - bounds.minX;
  assert.ok(
    markWidth / faviconBox.width >= MIN_FAVICON_FILL,
    `favicon mark spans ${(markWidth / faviconBox.width).toFixed(2)} of its canvas`,
  );
  assert.ok(markBox.width > faviconBox.width, "in-product mark keeps its wider safe area");
});
