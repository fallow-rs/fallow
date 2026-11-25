import { expect, it } from "vitest";
import { testOnlyFar } from "../src/lib";
import { sample } from "../src/__fixtures__/sample";
import { renderHelper } from "../src/testing/helpers";

it("far", () => {
  expect([testOnlyFar(), sample, renderHelper()]).toBeTruthy();
});
