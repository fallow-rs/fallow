import { describe, expect, it } from "vitest";
import { computeTipPosition } from "./hint";

/** Build a DOMRect-like box from a top-left corner and a size. */
const makeRect = (left: number, top: number, width: number, height: number): DOMRect => ({
  left,
  top,
  width,
  height,
  right: left + width,
  bottom: top + height,
  x: left,
  y: top,
  toJSON: () => ({}),
});

const VIEWPORT = { width: 1000, height: 800 };
const GAP = 6;

describe("computeTipPosition", () => {
  it("places the tip below and left-aligned when it fits", () => {
    const trigger = makeRect(100, 200, 60, 20);
    const pos = computeTipPosition(trigger, { width: 220, height: 60 }, VIEWPORT, GAP);
    expect(pos).toEqual({ left: 100, top: 226 });
  });

  it("shifts left when the tip would overflow the right edge", () => {
    // Trigger near the right edge; a 240px tip at left=900 would run off.
    const trigger = makeRect(900, 200, 60, 20);
    const pos = computeTipPosition(trigger, { width: 240, height: 60 }, VIEWPORT, GAP);
    // left = viewport.width - 8 - 240 = 752; top still below.
    expect(pos).toEqual({ left: 752, top: 226 });
  });

  it("flips above the trigger when it would overflow the bottom edge", () => {
    // Trigger near the bottom; below would push past viewport.height - 8.
    const trigger = makeRect(100, 770, 60, 20);
    const pos = computeTipPosition(trigger, { width: 220, height: 60 }, VIEWPORT, GAP);
    // top = trigger.top - gap - height = 770 - 6 - 60 = 704.
    expect(pos).toEqual({ left: 100, top: 704 });
  });

  it("clamps to the 8px margin in the bottom-right corner", () => {
    // Trigger in the far corner with a tip too big to fit either way.
    const trigger = makeRect(995, 795, 4, 4);
    const pos = computeTipPosition(trigger, { width: 400, height: 900 }, VIEWPORT, GAP);
    // Right-overflow shift: 1000 - 8 - 400 = 592.
    // Bottom-overflow flip: 795 - 6 - 900 = -111, clamped to 8.
    expect(pos).toEqual({ left: 592, top: 8 });
  });
});
