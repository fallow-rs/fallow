import { expect, test } from "vitest";
import { bannerText } from "../vite-plugins/banner";

test("banner", () => {
  expect(bannerText("app")).toBe("/* app */");
});
