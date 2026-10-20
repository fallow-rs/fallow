import { createRequire } from "node:module";

import { expect, test } from "vitest";

const require = createRequire(import.meta.url);
const { shippingFee } = require("./shipping.cjs");

test("gives free shipping at the minimum subtotal", () => {
  expect(shippingFee(50)).toBe(0);
});
