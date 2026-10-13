import { expect, test } from "vitest";

import Page, { heading } from "../app/blog/page";
import { Route } from "./routes/index";

test("the page and the route load", () => {
  expect(heading(2)).toBe("posts");
  expect(Page()).toBe("posts");
  expect(Route).toBeDefined();
});
