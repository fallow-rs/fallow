import { expect, test } from "vitest";
import { greet } from "../src/index";

test("greet", () => {
  expect(greet("world")).toMatchSnapshot();
});
