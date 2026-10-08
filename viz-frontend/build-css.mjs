// Writes ../crates/cli/viz-assets/viz.css: the census font faces, inlined as
// data URLs so the report renders offline in the fallow.tools type, followed
// by src/styles.css. The fonts are the latin subsets of Barlow, Barlow Semi
// Condensed and JetBrains Mono (SIL Open Font License 1.1, see src/fonts/).

import { readFileSync, writeFileSync } from "node:fs";

const FACES = [
  ["Barlow", 400, "barlow-latin-400-normal.woff2"],
  ["Barlow", 500, "barlow-latin-500-normal.woff2"],
  ["Barlow", 600, "barlow-latin-600-normal.woff2"],
  ["Barlow Semi Condensed", 600, "barlow-semi-condensed-latin-600-normal.woff2"],
  ["Barlow Semi Condensed", 700, "barlow-semi-condensed-latin-700-normal.woff2"],
  ["JetBrains Mono", 500, "jetbrains-mono-latin-500-normal.woff2"],
];

const fontDir = new URL("./src/fonts/", import.meta.url);
const faces = FACES.map(([family, weight, file]) => {
  const data = readFileSync(new URL(file, fontDir)).toString("base64");
  return `@font-face {
  font-family: "${family}";
  font-style: normal;
  font-weight: ${weight};
  font-display: swap;
  src: url(data:font/woff2;base64,${data}) format("woff2");
}`;
});

const styles = readFileSync(new URL("./src/styles.css", import.meta.url), "utf8");
const header =
  "/* Fonts: Barlow, Barlow Semi Condensed, JetBrains Mono. SIL Open Font License 1.1. */";
writeFileSync(
  new URL("../crates/cli/viz-assets/viz.css", import.meta.url),
  `${header}\n${faces.join("\n")}\n${styles}`,
);
