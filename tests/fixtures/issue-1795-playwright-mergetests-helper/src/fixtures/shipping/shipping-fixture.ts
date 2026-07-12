import { shippingBaseFixture } from './shipping-base-fixture';

// Function wrapping an IMPORTED base const via `.extend({})`, consumed directly
// as `shippingTest()(title, cb)` (no mergeTests). Exercises the imported-base
// `.extend` alias fact in isolation (fix a2, the row-2 multi-file correction).
export function shippingTest() {
  return shippingBaseFixture.extend({});
}
