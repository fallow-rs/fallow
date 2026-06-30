import { style } from "@vanilla-extract/css";

// LIVE: imported by main.tsx
export const container = style({
  display: "flex",
  flexDirection: "column",
});

// DEAD: exported, imported nowhere
export const deadBox = style({
  padding: "16px",
});
