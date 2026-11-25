import { expect, it } from "vitest";
import { viaBarrel } from "../src/barrel";
import { testOnlyFar } from "../src/lib";
import { sample } from "../src/__fixtures__/sample";
import { renderHelper } from "../src/testing/helpers";
import { toolSeam } from "../src/tool";

it("far", () => {
  expect([testOnlyFar(), sample, renderHelper(), viaBarrel(), toolSeam()]).toBeTruthy();
});
