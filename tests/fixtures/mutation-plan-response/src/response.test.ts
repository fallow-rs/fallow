import { expect, test, vi } from "vitest";

import { back, cached, remember } from "./response";

test("response shaping", () => {
  const res = { redirect: vi.fn(), setHeader: vi.fn(), cookie: vi.fn() };
  back(res, "/home");
  cached(res, 59);
  remember(res, "a");
  expect(res.redirect).toHaveBeenCalledWith("/home");
  expect(res.setHeader).toHaveBeenCalledWith("Cache-Control", "max-age=60");
  expect(res.cookie).toHaveBeenCalledWith("name", "a!");
});
