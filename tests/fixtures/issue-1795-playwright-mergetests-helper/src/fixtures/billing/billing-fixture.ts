import { billingBaseFixture } from './billing-base-fixture';

// Function wrapping an IMPORTED base const via `.extend({})` (no type argument).
export function billingTest() {
  return billingBaseFixture.extend({});
}
