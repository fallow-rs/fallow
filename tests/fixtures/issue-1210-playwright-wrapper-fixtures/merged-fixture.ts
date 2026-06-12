import { mergeTests as merge } from "@playwright/test";
import { testPrimary } from "./primary-fixture";
import { testSecondary } from "./secondary-fixture";

export const mergedTest = merge(testPrimary, testSecondary);
