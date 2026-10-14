import { expect, it, vi } from "vitest";

import { total } from "./consumer";

vi.mock("./service", () => ({
  price: vi.fn<(value: number) => number>(() => 10),
}));

it("uses the mocked price", () => {
  expect(total(1)).toBe(9);
});
