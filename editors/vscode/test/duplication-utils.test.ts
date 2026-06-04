import { describe, expect, it } from "vitest";
import {
  MIN_LINES_DEFAULT,
  MIN_LINES_FLOOR,
  MIN_OCCURRENCES_FLOOR,
  clampMinLines,
  clampMinOccurrences,
} from "../src/duplication-utils.js";

describe("clampMinOccurrences", () => {
  it("passes through valid values at or above the floor", () => {
    expect(clampMinOccurrences(2)).toBe(2);
    expect(clampMinOccurrences(3)).toBe(3);
    expect(clampMinOccurrences(10)).toBe(10);
  });

  it("clamps below-floor values up to the floor", () => {
    expect(clampMinOccurrences(1)).toBe(MIN_OCCURRENCES_FLOOR);
    expect(clampMinOccurrences(0)).toBe(MIN_OCCURRENCES_FLOOR);
    expect(clampMinOccurrences(-5)).toBe(MIN_OCCURRENCES_FLOOR);
  });

  it("truncates non-integers", () => {
    expect(clampMinOccurrences(3.9)).toBe(3);
    expect(clampMinOccurrences(2.1)).toBe(2);
  });

  it("falls back to the floor for non-finite values", () => {
    expect(clampMinOccurrences(Number.NaN)).toBe(MIN_OCCURRENCES_FLOOR);
    expect(clampMinOccurrences(Number.POSITIVE_INFINITY)).toBe(MIN_OCCURRENCES_FLOOR);
  });
});

describe("clampMinLines", () => {
  it("passes through positive integer values", () => {
    expect(clampMinLines(1)).toBe(1);
    expect(clampMinLines(40)).toBe(40);
  });

  it("clamps below-floor values up to the floor", () => {
    expect(clampMinLines(0)).toBe(MIN_LINES_FLOOR);
    expect(clampMinLines(-5)).toBe(MIN_LINES_FLOOR);
  });

  it("truncates non-integers", () => {
    expect(clampMinLines(40.9)).toBe(40);
  });

  it("falls back to the default for non-finite values", () => {
    expect(clampMinLines(Number.NaN)).toBe(MIN_LINES_DEFAULT);
    expect(clampMinLines(Number.POSITIVE_INFINITY)).toBe(MIN_LINES_DEFAULT);
  });
});
