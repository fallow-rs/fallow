import { checkout } from "./checkout";

if (process.env.FEATURE_TEST_ONLY) {
  checkout();
}
