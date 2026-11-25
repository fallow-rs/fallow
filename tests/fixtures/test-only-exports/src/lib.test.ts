import { describe, expect, it } from "vitest";
import {
  expectedUnusedTestOnly,
  internalTestOnly,
  testOnly,
  usedInProd,
  type TestOnlyShape,
} from "./lib";
import { publicApi } from "./index";

describe("lib", () => {
  it("works", () => {
    const shape: TestOnlyShape = { id: "a" };
    expect([testOnly(), usedInProd(), internalTestOnly(), expectedUnusedTestOnly(), publicApi(), shape.id]).toBeTruthy();
  });
});
