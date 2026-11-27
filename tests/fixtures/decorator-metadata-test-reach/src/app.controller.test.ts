import { expect, it } from "vitest";

import { AppController } from "./app.controller";

it("names the controller", () => {
  expect(new AppController(undefined as never).name()).toBe("undefined");
});
